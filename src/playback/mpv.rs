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
            #[allow(dead_code)]
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
        result_rx: mpsc::Receiver<Option<serde_json::Value>>,
    }

    /// Check if required dependencies are installed
    pub fn check_dependencies() -> Result<()> {
        // Check mpv
        if Command::new("mpv")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            anyhow::bail!(
                "mpv not found. Install it:\n\n  \
                 Ubuntu/Debian: sudo apt install mpv\n  \
                 Arch:          sudo pacman -S mpv\n  \
                 Fedora:        sudo dnf install mpv\n  \
                 macOS:         brew install mpv\n"
            );
        }

        // Check yt-dlp (needed for YouTube URLs)
        if Command::new("yt-dlp")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            anyhow::bail!(
                "yt-dlp not found (required for YouTube playback). Install it:\n\n  \
                 pip install yt-dlp\n  \
                 # or\n  \
                 pipx install yt-dlp\n"
            );
        }

        Ok(())
    }

    /// Fetch direct audio URL from YouTube using yt-dlp with timeout
    pub async fn fetch_audio_url(youtube_url: &str) -> Result<String> {
        use tokio::process::Command as TokioCommand;
        use tokio::time::{timeout, Duration};

        let fetch = TokioCommand::new("yt-dlp")
            .args([
                "-f", "bestaudio",
                "-g",  // Get URL only
                "--no-warnings",
                "--no-playlist",
                youtube_url,
            ])
            .output();

        // 15 second timeout for yt-dlp
        let output = timeout(Duration::from_secs(15), fetch)
            .await
            .context("yt-dlp timed out after 15 seconds")?
            .context("Failed to run yt-dlp")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("yt-dlp failed: {}", stderr.lines().next().unwrap_or("unknown error"));
        }

        let url = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        if url.is_empty() {
            anyhow::bail!("yt-dlp returned empty URL");
        }

        Ok(url)
    }

    impl MpvPlayer {
        /// Spawn mpv and connect to its IPC socket
        pub async fn spawn() -> Result<Self> {
            // Check dependencies first
            check_dependencies()?;

            let socket_path = PathBuf::from(format!("/tmp/grit-mpv-{}.sock", std::process::id()));

            // Clean up old socket if exists
            let _ = std::fs::remove_file(&socket_path);

            // Spawn mpv in idle mode (no --ytdl, we fetch URLs ourselves)
            let process = Command::new("mpv")
                .args([
                    "--idle=yes",
                    "--keep-open=yes",         // Don't quit on errors or end of file
                    "--no-video",
                    "--no-terminal",           // Disable mpv's terminal input/output
                    "--really-quiet",          // Suppress all messages
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

            // Spawn task to read events and results
            let (event_tx, event_rx) = mpsc::channel(32);
            let (result_tx, result_rx) = mpsc::channel(32);
            tokio::spawn(Self::read_events(BufReader::new(reader), event_tx, result_tx));

            Ok(Self {
                socket_path,
                process,
                writer,
                event_rx,
                result_rx,
            })
        }

        /// Background task that reads events from mpv
        async fn read_events(
            mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
            event_tx: mpsc::Sender<MpvEvent>,
            result_tx: mpsc::Sender<Option<serde_json::Value>>,
        ) {
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Ok(resp) = serde_json::from_str::<MpvResponse>(&line) {
                            match resp {
                                MpvResponse::Event(event) => {
                                    if event_tx.send(event).await.is_err() {
                                        break;
                                    }
                                }
                                MpvResponse::Result { data, .. } => {
                                    let _ = result_tx.send(data).await;
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
            // Use 'replace' mode to clear old track state
            self.send_command(vec![json!("loadfile"), json!(url), json!("replace")]).await?;

            // Wait for MPV to send playback-restart event
            let timeout = tokio::time::Instant::now() + tokio::time::Duration::from_secs(3);
            let mut got_restart = false;
            while tokio::time::Instant::now() < timeout {
                if let Ok(event) = self.event_rx.try_recv() {
                    if event.event == "playback-restart" {
                        got_restart = true;
                        break;
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }

            // Even after playback-restart, time-pos might not be immediately updated
            // Verify by querying time-pos until it's actually reset
            if got_restart {
                for _ in 0..20 {
                    if let Ok(Some(pos)) = self.get_position().await {
                        if pos < 1.0 {
                            // Position has been properly reset
                            return Ok(());
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
                }
            }

            Ok(())
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

        /// Subscribe to eof-reached (end of file)
        pub async fn observe_eof_reached(&mut self) -> Result<()> {
            self.send_command(vec![json!("observe_property"), json!(4), json!("eof-reached")])
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

        /// Get current playback position in seconds
        pub async fn get_position(&mut self) -> Result<Option<f64>> {
            self.send_command(vec![json!("get_property"), json!("time-pos")])
                .await?;
            // Wait for result with timeout
            tokio::select! {
                result = self.result_rx.recv() => {
                    if let Some(Some(data)) = result {
                        return Ok(data.as_f64());
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {}
            }
            Ok(None)
        }

        /// Check if track ended (call after recv_event)
        pub fn is_track_end(event: &MpvEvent) -> bool {
            event.event == "end-file"
        }

        /// Check if track ended naturally (not stopped/error)
        pub fn is_track_finished(event: &MpvEvent) -> bool {
            if event.event == "end-file" && event.reason.as_deref() == Some("eof") {
                return true;
            }
            // Also check for eof-reached property change
            if event.event == "property-change" && event.id == Some(4) {
                if let Some(data) = &event.data {
                    if let Some(eof_reached) = data.as_bool() {
                        return eof_reached;
                    }
                }
            }
            false
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
