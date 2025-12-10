use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct LyricLine {
    pub time_secs: f64,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub plain: Option<String>,
}

#[derive(Deserialize)]
struct LrcLibResponse {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
}

impl Lyrics {
    pub fn current_line_index(&self, position_secs: f64) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }

        let mut current = 0;
        for (i, line) in self.lines.iter().enumerate() {
            if line.time_secs <= position_secs {
                current = i;
            } else {
                break;
            }
        }
        Some(current)
    }
}

fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();

    for line in lrc.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('[') {
            continue;
        }

        if let Some(bracket_end) = line.find(']') {
            let timestamp = &line[1..bracket_end];
            let text = line[bracket_end + 1..].trim().to_string();

            if let Some(time_secs) = parse_timestamp(timestamp) {
                if !text.is_empty() {
                    lines.push(LyricLine { time_secs, text });
                }
            }
        }
    }

    lines.sort_by(|a, b| a.time_secs.partial_cmp(&b.time_secs).unwrap());
    lines
}

fn parse_timestamp(ts: &str) -> Option<f64> {
    let parts: Vec<&str> = ts.split(':').collect();
    if parts.len() != 2 {
        return None;
    }

    let minutes: f64 = parts[0].parse().ok()?;
    let seconds: f64 = parts[1].parse().ok()?;

    Some(minutes * 60.0 + seconds)
}

pub async fn fetch_lyrics(
    track_name: &str,
    artist_name: &str,
    duration_secs: u64,
) -> Result<Lyrics> {
    let client = Client::new();

    let url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}&duration={}",
        urlencoding::encode(track_name),
        urlencoding::encode(artist_name),
        duration_secs
    );

    let response = client
        .get(&url)
        .header("User-Agent", "grit/1.0")
        .send()
        .await?;

    if !response.status().is_success() {
        return Ok(Lyrics::default());
    }

    let data: LrcLibResponse = response.json().await?;

    let lines = data
        .synced_lyrics
        .as_ref()
        .map(|s| parse_lrc(s))
        .unwrap_or_default();

    Ok(Lyrics {
        lines,
        plain: data.plain_lyrics,
    })
}

pub fn clean_yt_title(title: &str) -> (String, Option<String>) {
    let patterns = [
        "(official video)",
        "(official music video)",
        "(official audio)",
        "(lyric video)",
        "(lyrics video)",
        "(lyrics)",
        "(audio)",
        "(visualizer)",
        "(official visualizer)",
        "(music video)",
        "[official video]",
        "[official music video]",
        "[official audio]",
        "[lyric video]",
        "[lyrics video]",
        "[lyrics]",
        "[audio]",
        "[visualizer]",
        "[official visualizer]",
        "[music video]",
        "official video",
        "official music video",
        "official audio",
        "lyric video",
        "lyrics video",
        "music video",
        "(hd)",
        "(hq)",
        "[hd]",
        "[hq]",
        "(4k)",
        "[4k]",
        "(remastered)",
        "[remastered]",
        "(remaster)",
        "[remaster]",
        "(live)",
        "[live]",
        "(acoustic)",
        "[acoustic]",
    ];

    let mut cleaned = title.to_lowercase();
    for p in patterns {
        cleaned = cleaned.replace(p, "");
    }

    cleaned = cleaned
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect();

    while cleaned.contains("  ") {
        cleaned = cleaned.replace("  ", " ");
    }
    cleaned = cleaned.trim().to_string();

    let parts: Vec<&str> = cleaned.split(" - ").collect();
    if parts.len() >= 2 {
        let artist = parts[0].trim().to_string();
        let track = parts[1..].join(" - ").trim().to_string();
        (track, Some(artist))
    } else {
        (cleaned, None)
    }
}

pub async fn fetch_lyrics_for_yt(title: &str, duration_secs: u64) -> Result<Lyrics> {
    let (track, artist) = clean_yt_title(title);
    let artist_str = artist.as_deref().unwrap_or("");
    fetch_lyrics(&track, artist_str, duration_secs).await
}

pub struct LyricsFetcher {
    tx: mpsc::Sender<Lyrics>,
    rx: mpsc::Receiver<Lyrics>,
    current_track_id: Option<String>,
}

impl LyricsFetcher {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1);
        Self {
            tx,
            rx,
            current_track_id: None,
        }
    }

    pub fn try_recv(&mut self) -> Option<Lyrics> {
        self.rx.try_recv().ok()
    }

    pub fn fetch_for_track(
        &mut self,
        track_id: &str,
        track_name: &str,
        artist: &str,
        duration_secs: u64,
    ) {
        if self.current_track_id.as_deref() == Some(track_id) {
            return;
        }
        self.current_track_id = Some(track_id.to_string());
        let tx = self.tx.clone();
        let name = track_name.to_string();
        let artist = artist.to_string();
        tokio::spawn(async move {
            let lyrics = fetch_lyrics(&name, &artist, duration_secs)
                .await
                .unwrap_or_default();
            let _ = tx.send(lyrics).await;
        });
    }

    pub fn fetch_for_yt(&mut self, track_id: &str, title: &str, duration_secs: u64) {
        if self.current_track_id.as_deref() == Some(track_id) {
            return;
        }
        self.current_track_id = Some(track_id.to_string());
        let tx = self.tx.clone();
        let title = title.to_string();
        tokio::spawn(async move {
            let lyrics = fetch_lyrics_for_yt(&title, duration_secs)
                .await
                .unwrap_or_default();
            let _ = tx.send(lyrics).await;
        });
    }

    pub fn reset(&mut self) {
        self.current_track_id = None;
        while self.rx.try_recv().is_ok() {}
    }
}
