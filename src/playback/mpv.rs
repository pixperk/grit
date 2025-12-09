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

    pub fn check_dependencies() -> Result<()> {
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

    pub async fn fetch_audio_url(youtube_url: &str) -> Result<String> {
        use tokio::process::Command as TokioCommand;
        use tokio::time::{timeout, Duration};

        let fetch = TokioCommand::new("yt-dlp")
            .args([
                "-f", "bestaudio",
                "-g",
                "--no-warnings",
                "--no-playlist",
                youtube_url,
            ])
            .output();

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
        pub async fn spawn() -> Result<Self> {
            check_dependencies()?;

            let socket_path = PathBuf::from(format!("/tmp/grit-mpv-{}.sock", std::process::id()));
            let _ = std::fs::remove_file(&socket_path);

            let process = Command::new("mpv")
                .args([
                    "--idle=yes",
                    "--keep-open=yes",
                    "--no-video",
                    "--no-terminal",
                    "--really-quiet",
                    &format!("--input-ipc-server={}", socket_path.display()),
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("Failed to spawn mpv")?;

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

            let stream = UnixStream::connect(&socket_path)
                .await
                .context("Failed to connect to mpv socket")?;

            let (reader, writer) = stream.into_split();
            let writer = BufWriter::new(writer);

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

        async fn send_command(&mut self, cmd: Vec<serde_json::Value>) -> Result<()> {
            let msg = json!({ "command": cmd });
            let line = format!("{}\n", msg);
            self.writer.write_all(line.as_bytes()).await?;
            self.writer.flush().await?;
            Ok(())
        }

        pub async fn load(&mut self, url: &str) -> Result<()> {
            self.send_command(vec![json!("loadfile"), json!(url), json!("replace")]).await?;
            self.send_command(vec![json!("set_property"), json!("pause"), json!(false)]).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            while self.result_rx.try_recv().is_ok() {}
            Ok(())
        }

        pub async fn pause(&mut self) -> Result<()> {
            self.send_command(vec![json!("set_property"), json!("pause"), json!(true)]).await
        }

        pub async fn resume(&mut self) -> Result<()> {
            self.send_command(vec![json!("set_property"), json!("pause"), json!(false)]).await
        }

        pub async fn seek(&mut self, seconds: i64) -> Result<()> {
            self.send_command(vec![json!("seek"), json!(seconds), json!("relative")]).await
        }

        pub async fn seek_absolute(&mut self, seconds: f64) -> Result<()> {
            self.send_command(vec![json!("seek"), json!(seconds), json!("absolute")]).await
        }

        pub async fn observe_eof_reached(&mut self) -> Result<()> {
            self.send_command(vec![json!("observe_property"), json!(4), json!("eof-reached")]).await
        }

        pub fn try_recv_event(&mut self) -> Option<MpvEvent> {
            self.event_rx.try_recv().ok()
        }

        pub async fn get_position(&mut self) -> Result<Option<f64>> {
            self.send_command(vec![json!("get_property"), json!("time-pos")]).await?;
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

        pub fn is_track_finished(event: &MpvEvent) -> bool {
            if event.event == "end-file" && event.reason.as_deref() == Some("eof") {
                return true;
            }
            if event.event == "property-change" && event.id == Some(4) {
                if let Some(data) = &event.data {
                    if let Some(eof_reached) = data.as_bool() {
                        return eof_reached;
                    }
                }
            }
            false
        }

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