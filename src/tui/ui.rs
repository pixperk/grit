use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::io::{self, Stdout};

use super::App;

const SAKURA_PINK: Color = Color::Rgb(255, 183, 197);
const SAKURA_SOFT: Color = Color::Rgb(255, 218, 233);
const SEA_GREEN: Color = Color::Rgb(95, 158, 160);
const SEA_GREEN_BRIGHT: Color = Color::Rgb(120, 190, 192);
const SEA_GREEN_DIM: Color = Color::Rgb(75, 125, 127);
const SAKURA_BG: Color = Color::Rgb(30, 30, 35);
const SAKURA_FG: Color = Color::Rgb(240, 240, 245);
const SAKURA_DIM: Color = Color::Rgb(120, 120, 130);

pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Tui {
    pub fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        Ok(Self { terminal })
    }

    pub fn draw(&mut self, app: &App) -> Result<()> {
        self.terminal.draw(|f| render(f, app))?;
        Ok(())
    }

    pub fn poll_key(&self) -> Result<Option<KeyEvent>> {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    return Ok(Some(key));
                }
            }
        }
        Ok(None)
    }

    pub fn restore(&mut self) -> Result<()> {
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    frame.render_widget(Block::default().style(Style::default().bg(SAKURA_BG)), area);

    // Split horizontally: player (left) and playlist (right)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .margin(1)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Left side: player controls
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(main_chunks[0]);

    draw_header(frame, app, left_chunks[0]);
    draw_now_playing(frame, app, left_chunks[1]);
    draw_progress(frame, app, left_chunks[2]);
    draw_next_up(frame, app, left_chunks[3]);
    draw_controls(frame, app, left_chunks[5]);

    if app.show_lyrics {
        draw_lyrics(frame, app, main_chunks[1]);
    } else {
        draw_playlist(frame, app, main_chunks[1]);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let backend_str = match app.backend {
        super::PlayerBackend::Mpv => "yt",
        super::PlayerBackend::Spotify => "spotify",
    };

    let status = if app.loading {
        "◌"
    } else if app.is_paused {
        "⏸"
    } else {
        "▶"
    };

    let status_color = if app.loading { SAKURA_SOFT } else { SEA_GREEN };

    let header = Line::from(vec![
        Span::styled(
            "grit ",
            Style::default()
                .fg(SAKURA_PINK)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(status, Style::default().fg(status_color)),
        Span::styled(" ", Style::default()),
        Span::styled(&app.playlist_name, Style::default().fg(SAKURA_FG)),
        Span::styled(" ", Style::default()),
        Span::styled(
            format!("[{}]", backend_str),
            Style::default().fg(SAKURA_DIM),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(SAKURA_PINK));

    frame.render_widget(Paragraph::new(header).block(block), area);
}

fn draw_now_playing(frame: &mut Frame, app: &App, area: Rect) {
    let content = if app.loading {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "loading...",
                Style::default().fg(SEA_GREEN).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "fetching track",
                Style::default().fg(SEA_GREEN_DIM),
            )),
        ]
    } else if let Some(error) = &app.error {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "uh oh!",
                Style::default()
                    .fg(Color::Rgb(255, 100, 100))
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                error.as_str(),
                Style::default().fg(SAKURA_DIM),
            )),
        ]
    } else {
        let (title, artists) = app
            .current_track()
            .map(|t| (t.name.clone(), t.artists.join(", ")))
            .unwrap_or(("Nothing playing".into(), String::new()));

        vec![
            Line::from(Span::styled(
                "now playing",
                Style::default().fg(SEA_GREEN_DIM),
            )),
            Line::from(""),
            Line::from(Span::styled(
                title,
                Style::default().fg(SAKURA_FG).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(artists, Style::default().fg(SEA_GREEN_BRIGHT))),
        ]
    };

    frame.render_widget(Paragraph::new(content), area);
}

fn draw_progress(frame: &mut Frame, app: &App, area: Rect) {
    if app.is_seeking() {
        let seek_pos = app.get_seek_position().unwrap_or(0.0);
        let pos = App::format_time(seek_pos);
        let dur = App::format_time(app.duration_secs);
        let label = format!(
            "seek: {} / {} (<-/-> to move, enter to confirm, esc to cancel)",
            pos, dur
        );
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(SAKURA_PINK).bg(Color::Rgb(50, 50, 55)))
            .ratio(app.seek_progress())
            .label(Span::styled(
                label,
                Style::default().fg(SAKURA_FG).add_modifier(Modifier::BOLD),
            ));
        frame.render_widget(gauge, area);
    } else if app.error.is_some() {
        let gauge = Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(Color::Rgb(80, 80, 85))
                    .bg(Color::Rgb(50, 50, 55)),
            )
            .ratio(0.0)
            .label(Span::styled("— / —", Style::default().fg(SAKURA_DIM)));
        frame.render_widget(gauge, area);
    } else {
        let pos = App::format_time(app.position_secs);
        let dur = App::format_time(app.duration_secs);
        let label = format!("{} / {}", pos, dur);

        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(SEA_GREEN).bg(Color::Rgb(50, 50, 55)))
            .ratio(app.progress())
            .label(Span::styled(label, Style::default().fg(SAKURA_FG)));

        frame.render_widget(gauge, area);
    }
}

fn draw_next_up(frame: &mut Frame, app: &App, area: Rect) {
    use crate::playback::events::RepeatMode;

    let content = if app.shuffle {
        let repeat_text = match app.repeat_mode {
            RepeatMode::None => "",
            RepeatMode::All => " | repeat all",
            RepeatMode::One => " | repeat one",
        };
        vec![
            Line::from(vec![
                Span::styled("shuffle", Style::default().fg(SEA_GREEN)),
                Span::styled(repeat_text, Style::default().fg(SEA_GREEN)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "next track is random",
                Style::default().fg(SEA_GREEN_DIM),
            )),
        ]
    } else if app.repeat_mode == RepeatMode::One {
        let (title, artists) = app
            .current_track()
            .map(|t| (t.name.clone(), t.artists.join(", ")))
            .unwrap_or(("—".into(), String::new()));

        vec![
            Line::from(Span::styled("repeat one", Style::default().fg(SEA_GREEN))),
            Line::from(""),
            Line::from(Span::styled(
                format!("{} - {}", title, artists),
                Style::default().fg(SEA_GREEN_DIM),
            )),
        ]
    } else {
        let (title, artists) = app
            .next_track()
            .map(|t| (t.name.clone(), t.artists.join(", ")))
            .unwrap_or(("—".into(), String::new()));

        let header = if app.repeat_mode == RepeatMode::All {
            "next up | repeat all"
        } else {
            "next up"
        };

        vec![
            Line::from(Span::styled(header, Style::default().fg(SAKURA_DIM))),
            Line::from(""),
            Line::from(Span::styled(
                format!("{} - {}", title, artists),
                Style::default().fg(SAKURA_DIM),
            )),
        ]
    };

    frame.render_widget(Paragraph::new(content), area);
}

fn draw_playlist(frame: &mut Frame, app: &App, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;

    let scroll_offset = if app.selected_index >= visible_height {
        app.selected_index - visible_height + 1
    } else {
        0
    };

    let items: Vec<ListItem> = app
        .tracks
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(i, track)| {
            let is_current = i == app.current_index;
            let is_selected = i == app.selected_index;
            let is_match = app.is_search_match(i);

            let prefix = if is_current { "▶ " } else { "  " };
            let name = if track.name.len() > 25 {
                format!("{}...", &track.name[..22])
            } else {
                track.name.clone()
            };

            let style = if is_selected {
                Style::default().fg(SAKURA_BG).bg(SAKURA_PINK)
            } else if is_match {
                Style::default()
                    .fg(Color::Rgb(255, 220, 100))
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default()
                    .fg(SEA_GREEN_BRIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(SAKURA_FG)
            };

            ListItem::new(format!("{}{}", prefix, name)).style(style)
        })
        .collect();

    let title = if let Some(ref query) = app.search_query {
        let match_info = if app.search_matches.is_empty() {
            "no matches".to_string()
        } else {
            format!(
                "{}/{}",
                app.search_match_index + 1,
                app.search_matches.len()
            )
        };
        format!(" /{}  [{}] ", query, match_info)
    } else {
        " playlist ".to_string()
    };

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(SAKURA_PINK)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SAKURA_DIM));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn draw_lyrics(frame: &mut Frame, app: &App, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;
    let current_idx = app.current_lyric_index();

    let auto_indicator = if app.lyrics_auto_scroll { "⟳" } else { "⏸" };
    let title = if app.lyrics_loading {
        " lyrics (loading...) ".to_string()
    } else if let Some(ref lyrics) = app.lyrics {
        if !lyrics.lines.is_empty() {
            format!(" lyrics (synced) {} ", auto_indicator)
        } else if lyrics.plain.is_some() {
            " lyrics ".to_string()
        } else {
            " lyrics (not found) ".to_string()
        }
    } else {
        " lyrics ".to_string()
    };

    let items: Vec<ListItem> = if app.lyrics_loading {
        vec![ListItem::new("Loading lyrics...").style(Style::default().fg(SAKURA_DIM))]
    } else if let Some(ref lyrics) = app.lyrics {
        if lyrics.lines.is_empty() {
            if let Some(ref plain) = lyrics.plain {
                let scroll = app
                    .lyrics_scroll
                    .min(plain.lines().count().saturating_sub(1));
                plain
                    .lines()
                    .skip(scroll)
                    .take(visible_height)
                    .map(|line| {
                        ListItem::new(line.to_string()).style(Style::default().fg(SAKURA_FG))
                    })
                    .collect()
            } else {
                vec![ListItem::new("No lyrics available").style(Style::default().fg(SAKURA_DIM))]
            }
        } else {
            let scroll = if app.lyrics_auto_scroll {
                current_idx
                    .map(|idx| idx.saturating_sub(visible_height / 3))
                    .unwrap_or(app.lyrics_scroll)
            } else {
                app.lyrics_scroll.min(lyrics.lines.len().saturating_sub(1))
            };

            lyrics
                .lines
                .iter()
                .enumerate()
                .skip(scroll)
                .take(visible_height)
                .map(|(i, line)| {
                    let is_current = current_idx == Some(i);
                    let style = if is_current {
                        Style::default()
                            .fg(SEA_GREEN_BRIGHT)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(SAKURA_DIM)
                    };
                    ListItem::new(line.text.clone()).style(style)
                })
                .collect()
        }
    } else {
        vec![ListItem::new("Press 'l' to load lyrics").style(Style::default().fg(SAKURA_DIM))]
    };

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(SAKURA_PINK)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SAKURA_DIM));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn draw_controls(frame: &mut Frame, app: &App, area: Rect) {
    let k = Style::default().fg(SAKURA_PINK);
    let d = Style::default().fg(SAKURA_DIM);

    let controls = if app.is_searching() {
        Line::from(vec![
            Span::styled("[type]", k),
            Span::styled(" filter  ", d),
            Span::styled("[ctrl+n/p]", k),
            Span::styled(" next/prev  ", d),
            Span::styled("[enter]", k),
            Span::styled(" play  ", d),
            Span::styled("[esc]", k),
            Span::styled(" cancel", d),
        ])
    } else if app.is_seeking() {
        Line::from(vec![
            Span::styled("[←→]", k),
            Span::styled(" ±5s  ", d),
            Span::styled("[enter]", k),
            Span::styled(" confirm  ", d),
            Span::styled("[esc]", k),
            Span::styled(" cancel", d),
        ])
    } else if app.search_blocked {
        Line::from(vec![
            Span::styled(
                "exit lyrics first ",
                Style::default().fg(Color::Rgb(255, 150, 150)),
            ),
            Span::styled("[l]", k),
        ])
    } else if app.show_lyrics {
        Line::from(vec![
            Span::styled("[↑↓]", k),
            Span::styled(" scroll  ", d),
            Span::styled("[a]", k),
            Span::styled(" auto  ", d),
            Span::styled("[n/p]", k),
            Span::styled(" skip  ", d),
            Span::styled("[←→]", k),
            Span::styled(" seek  ", d),
            Span::styled("[l]", k),
            Span::styled(" back  ", d),
            Span::styled("[q]", k),
            Span::styled(" quit", d),
        ])
    } else {
        Line::from(vec![
            Span::styled("[space]", k),
            Span::styled(" pause  ", d),
            Span::styled("[n/p]", k),
            Span::styled(" skip  ", d),
            Span::styled("[←→]", k),
            Span::styled(" seek  ", d),
            Span::styled("[g]", k),
            Span::styled(" goto  ", d),
            Span::styled("[/]", k),
            Span::styled(" search  ", d),
            Span::styled("[l]", k),
            Span::styled(" lyrics  ", d),
            Span::styled("[s]", k),
            Span::styled(" shuffle  ", d),
            Span::styled("[r]", k),
            Span::styled(" repeat  ", d),
            Span::styled("[q]", k),
            Span::styled(" quit", d),
        ])
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(SAKURA_PINK));

    frame.render_widget(Paragraph::new(controls).block(block), area);
}
