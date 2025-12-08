use std::io::{self, Stdout};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
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

use super::App;

const SAKURA_PINK: Color = Color::Rgb(255, 183, 197);
const SAKURA_DEEP: Color = Color::Rgb(255, 105, 180);
const SAKURA_SOFT: Color = Color::Rgb(255, 218, 233);
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

    pub fn poll_key(&self) -> Result<Option<KeyCode>> {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    return Ok(Some(key.code));
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

    frame.render_widget(
        Block::default().style(Style::default().bg(SAKURA_BG)),
        area,
    );

    // Split horizontally: player (left) and playlist (right)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .margin(1)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
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

    // Right side: playlist
    draw_playlist(frame, app, main_chunks[1]);
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

    let status_color = if app.loading { SAKURA_SOFT } else { SAKURA_DEEP };

    let header = Line::from(vec![
        Span::styled("grit ", Style::default().fg(SAKURA_PINK).add_modifier(Modifier::BOLD)),
        Span::styled(status, Style::default().fg(status_color)),
        Span::styled(" ", Style::default()),
        Span::styled(&app.playlist_name, Style::default().fg(SAKURA_FG)),
        Span::styled(" ", Style::default()),
        Span::styled(format!("[{}]", backend_str), Style::default().fg(SAKURA_DIM)),
    ]);

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(SAKURA_PINK));

    frame.render_widget(Paragraph::new(header).block(block), area);
}

fn draw_now_playing(frame: &mut Frame, app: &App, area: Rect) {
    let content = if app.error.is_some() {
        vec![
            Line::from(""),
            Line::from(Span::styled("uh oh!", Style::default().fg(Color::Rgb(255, 100, 100)).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(Span::styled("failed to load track", Style::default().fg(SAKURA_DIM))),
        ]
    } else {
        let (title, artists) = app
            .current_track()
            .map(|t| (t.name.clone(), t.artists.join(", ")))
            .unwrap_or(("Nothing playing".into(), String::new()));

        vec![
            Line::from(Span::styled("now playing", Style::default().fg(SAKURA_DIM))),
            Line::from(""),
            Line::from(Span::styled(title, Style::default().fg(SAKURA_FG).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(artists, Style::default().fg(SAKURA_SOFT))),
        ]
    };

    frame.render_widget(Paragraph::new(content), area);
}

fn draw_progress(frame: &mut Frame, app: &App, area: Rect) {
    if app.error.is_some() {
        // Show empty progress on error
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Rgb(80, 80, 85)).bg(Color::Rgb(50, 50, 55)))
            .ratio(0.0)
            .label(Span::styled("— / —", Style::default().fg(SAKURA_DIM)));
        frame.render_widget(gauge, area);
    } else {
        let pos = App::format_time(app.position_secs);
        let dur = App::format_time(app.duration_secs);
        let label = format!("{} / {}", pos, dur);

        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(SAKURA_DEEP).bg(Color::Rgb(50, 50, 55)))
            .ratio(app.progress())
            .label(Span::styled(label, Style::default().fg(SAKURA_FG)));

        frame.render_widget(gauge, area);
    }
}

fn draw_next_up(frame: &mut Frame, app: &App, area: Rect) {
    let content = if app.shuffle {
        vec![
            Line::from(Span::styled("shuffle", Style::default().fg(SAKURA_PINK))),
            Line::from(""),
            Line::from(Span::styled("next track is random", Style::default().fg(SAKURA_DIM))),
        ]
    } else {
        let (title, artists) = app
            .next_track()
            .map(|t| (t.name.clone(), t.artists.join(", ")))
            .unwrap_or(("—".into(), String::new()));

        vec![
            Line::from(Span::styled("next up", Style::default().fg(SAKURA_DIM))),
            Line::from(""),
            Line::from(Span::styled(format!("{} - {}", title, artists), Style::default().fg(SAKURA_DIM))),
        ]
    };

    frame.render_widget(Paragraph::new(content), area);
}

fn draw_playlist(frame: &mut Frame, app: &App, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize; // Account for border

    // Calculate scroll offset to keep selected item visible
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

            let prefix = if is_current { "▶ " } else { "  " };
            let name = if track.name.len() > 25 {
                format!("{}...", &track.name[..22])
            } else {
                track.name.clone()
            };

            let style = if is_selected {
                Style::default().fg(SAKURA_BG).bg(SAKURA_PINK)
            } else if is_current {
                Style::default().fg(SAKURA_DEEP).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(SAKURA_FG)
            };

            ListItem::new(format!("{}{}", prefix, name)).style(style)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(" playlist ", Style::default().fg(SAKURA_PINK)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(SAKURA_DIM));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn draw_controls(frame: &mut Frame, _app: &App, area: Rect) {
    let controls = Line::from(vec![
        Span::styled("[space]", Style::default().fg(SAKURA_PINK)),
        Span::styled(" pause ", Style::default().fg(SAKURA_DIM)),
        Span::styled("[↑/↓]", Style::default().fg(SAKURA_PINK)),
        Span::styled(" select ", Style::default().fg(SAKURA_DIM)),
        Span::styled("[enter]", Style::default().fg(SAKURA_PINK)),
        Span::styled(" play ", Style::default().fg(SAKURA_DIM)),
        Span::styled("[n/p]", Style::default().fg(SAKURA_PINK)),
        Span::styled(" skip ", Style::default().fg(SAKURA_DIM)),
        Span::styled("[q]", Style::default().fg(SAKURA_PINK)),
        Span::styled(" quit", Style::default().fg(SAKURA_DIM)),
    ]);

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(SAKURA_PINK));

    frame.render_widget(Paragraph::new(controls).block(block), area);
}
