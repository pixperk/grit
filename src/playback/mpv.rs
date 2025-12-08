#[cfg(unix)]
mod unix {
    use anyhow::{Context, Result};
    use serde::Deserialize;
    use serde_json::json;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
    use tokio::net::UnixStream;
    use tokio::sync::mpsc;

    /// Events received from mpv
    #[derive(Debug, Clone, Deserialize)]
    pub struct MpvEvent {
        pub event: String,
        #[serde(default)]
        pub reason: Option<String>,
        #[serde(default)]
        pub id: Option<i64>,
        #[serde(default)]
        pub data: Option<serde_json::Value>,
    }

    /// Response from mpv (either event or command result)
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum MpvResponse {
        Event(MpvEvent),
        Result {
            error: String,
            #[serde(default)]
            data: Option<serde_json::Value>,
        },
    }

    pub struct MpvPlayer {
        socket_path: PathBuf,
        process: Child,
        writer: BufWriter<tokio::net::unix::OwnedWriteHalf>,
        event_rx: mpsc::Receiver<MpvEvent>,
    }

    impl MpvPlayer {
        /// Spawn mpv and connect to its IPC socket
        pub async fn spawn() -> Result<Self> {
            let socket_path = PathBuf::from(format!("/tmp/grit-mpv-{}.sock", std::process::id()));

            // Clean up old socket if exists
            let _ = std::fs::remove_file(&socket_path);

            // Spawn mpv in idle mode
            let process = Command::new("mpv")
                .args([
                    "--idle",
                    "--no-video",
                    "--no-terminal",
                    &format!("--input-ipc-server={}", socket_path.display()),
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("Failed to spawn mpv - is it installed?")?;

            // Wait for socket to appear
            let mut connected = false;
            for _ in 0..50 {
                if socket_path.exists() {
                    connected = true;
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            if !connected {
                anyhow::bail!("mpv socket did not appear at {}", socket_path.display());
            }

            // Connect to socket
            let stream = UnixStream::connect(&socket_path)
                .await
                .context("Failed to connect to mpv socket")?;

            let (reader, writer) = stream.into_split();
            let writer = BufWriter::new(writer);

            // Spawn task to read events
            let (event_tx, event_rx) = mpsc::channel(32);
            tokio::spawn(Self::read_events(BufReader::new(reader), event_tx));

            Ok(Self {
                socket_path,
                process,
                writer,
                event_rx,
            })
        }

        /// Background task that reads events from mpv
        async fn read_events(
            mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
            tx: mpsc::Sender<MpvEvent>,
        ) {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(resp) = serde_json::from_str::<MpvResponse>(&line) {
                            if let MpvResponse::Event(event) = resp {
                                if tx.send(event).await.is_err() {
                                    break; // Receiver dropped
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }

        /// Send a raw command to mpv
        async fn send_command(&mut self, cmd: Vec<serde_json::Value>) -> Result<()> {
            let msg = json!({ "command": cmd });
            let line = format!("{}\n", msg);
            self.writer.write_all(line.as_bytes()).await?;
            self.writer.flush().await?;
            Ok(())
        }

        /// Load and play a URL/file
        pub async fn load(&mut self, url: &str) -> Result<()> {
            self.send_command(vec![json!("loadfile"), json!(url)]).await
        }

        /// Pause playback
        pub async fn pause(&mut self) -> Result<()> {
            self.send_command(vec![json!("set_property"), json!("pause"), json!(true)])
                .await
        }

        /// Resume playback
        pub async fn resume(&mut self) -> Result<()> {
            self.send_command(vec![json!("set_property"), json!("pause"), json!(false)])
                .await
        }

        /// Stop playback
        pub async fn stop(&mut self) -> Result<()> {
            self.send_command(vec![json!("stop")]).await
        }

        /// Seek relative (positive = forward, negative = backward)
        pub async fn seek(&mut self, seconds: i64) -> Result<()> {
            self.send_command(vec![json!("seek"), json!(seconds), json!("relative")])
                .await
        }

        /// Seek to absolute position
        pub async fn seek_absolute(&mut self, seconds: f64) -> Result<()> {
            self.send_command(vec![json!("seek"), json!(seconds), json!("absolute")])
                .await
        }

        /// Set volume (0-100)
        pub async fn set_volume(&mut self, volume: u8) -> Result<()> {
            let vol = volume.min(100);
            self.send_command(vec![json!("set_property"), json!("volume"), json!(vol)])
                .await
        }

        /// Subscribe to time position updates
        pub async fn observe_time_pos(&mut self) -> Result<()> {
            self.send_command(vec![json!("observe_property"), json!(1), json!("time-pos")])
                .await
        }

        /// Subscribe to duration
        pub async fn observe_duration(&mut self) -> Result<()> {
            self.send_command(vec![json!("observe_property"), json!(2), json!("duration")])
                .await
        }

        /// Subscribe to pause state
        pub async fn observe_pause(&mut self) -> Result<()> {
            self.send_command(vec![json!("observe_property"), json!(3), json!("pause")])
                .await
        }

        /// Get next event (non-blocking)
        pub fn try_recv_event(&mut self) -> Option<MpvEvent> {
            self.event_rx.try_recv().ok()
        }

        /// Wait for next event
        pub async fn recv_event(&mut self) -> Option<MpvEvent> {
            self.event_rx.recv().await
        }

        /// Check if track ended (call after recv_event)
        pub fn is_track_end(event: &MpvEvent) -> bool {
            event.event == "end-file"
        }

        /// Check if track ended naturally (not stopped/error)
        pub fn is_track_finished(event: &MpvEvent) -> bool {
            event.event == "end-file" && event.reason.as_deref() == Some("eof")
        }

        /// Quit mpv gracefully
        pub async fn quit(&mut self) -> Result<()> {
            self.send_command(vec![json!("quit")]).await
        }
    }

    impl Drop for MpvPlayer {
        fn drop(&mut self) {
            let _ = self.process.kill();
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

#[cfg(unix)]
pub use unix::*;

#[cfg(not(unix))]
compile_error!("Playback is currently only supported on Unix systems (Linux/macOS). Windows support coming soon.");
