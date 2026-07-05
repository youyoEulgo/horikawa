use ratatui::{
    prelude::*,
    style::Color,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use horikawa_core::player::PlaybackState;

use crate::input::InputMode;
use crate::view::ViewMode;

pub fn draw_now_playing(frame: &mut Frame, app: &crate::App, area: Rect) {
    let state = app.player.state();
    let state_str = match state {
        PlaybackState::Playing => "\u{f04c}",
        PlaybackState::Paused => "\u{f04b}",
        PlaybackState::Stopped => "\u{f04d}",
    };

    // Get metadata if available
    let metadata = app.player.metadata();

    // Build track display string from metadata or filename
    let (title, artist_album) = if let Some(ref meta) = metadata {
        let title = meta.title.clone().unwrap_or_else(|| {
            app.player
                .current_track()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_else(|| "Unknown".to_string())
        });
        let artist_album = match (&meta.artist, &meta.album) {
            (Some(artist), Some(album)) => format!("{} - {}", artist, album),
            (Some(artist), None) => artist.clone(),
            (None, Some(album)) => album.clone(),
            (None, None) => String::new(),
        };
        (title, artist_album)
    } else {
        let title = app
            .player
            .current_track()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "No track".to_string());
        (title, String::new())
    };

    // Get position and duration
    let position = app.player.position();
    let duration = app.player.duration().unwrap_or(std::time::Duration::ZERO);

    // Format time as M:SS
    let format_time = |d: std::time::Duration| -> String {
        let secs = d.as_secs();
        format!("{}:{:02}", secs / 60, secs % 60)
    };

    // Calculate progress bar
    let progress_width = 20;
    let progress = if duration.as_secs() > 0 {
        (position.as_secs_f64() / duration.as_secs_f64()).min(1.0)
    } else {
        0.0
    };
    let filled = (progress * progress_width as f64).round() as usize;
    let bar = format!(
        "[{}{}]",
        "━".repeat(filled),
        "─".repeat(progress_width - filled)
    );

    let mut lines = vec![Line::from(Span::styled(
        format!(" {} {} ", state_str, title),
        Style::default().bold(),
    ))];

    // Only add artist/album line if there's content
    if !artist_album.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("   {} ", artist_album),
            Style::default().fg(Color::Gray),
        )));
    }

    // Show volume indicator
    let vol_pct = (app.volume * 100.0) as i32;
    let vol_str = if vol_pct == 0 {
        "Mute".to_string()
    } else {
        let blocks = (vol_pct as usize + 9) / 10;
        let bar: String = (0..10)
            .map(|i| if i < blocks { '█' } else { '░' })
            .collect();
        format!("{} {}%", bar, vol_pct)
    };

    lines.push(Line::from(format!(
        " {} {} / {}  {} ",
        bar,
        format_time(position),
        format_time(duration),
        vol_str
    )));

    let now_playing = Paragraph::new(lines).block(
        Block::default()
            .title(" Now Playing ")
            .borders(Borders::ALL),
    );

    frame.render_widget(now_playing, area);
}

pub fn draw_status_bar(frame: &mut Frame, app: &crate::App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    match app.input_mode {
        InputMode::Command => {
            let input_text = format!("/{}", app.input_buffer.content());
            let mut spans = vec![Span::styled(input_text, Style::default().fg(Color::Yellow))];
            if let Some(ref ghost) = app.command_ghost {
                spans.push(Span::styled(
                    ghost.clone(),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            frame.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);
        }
        InputMode::Search => {
            let text = format!("Search: {}", app.input_buffer.content());
            frame.render_widget(
                Paragraph::new(text).style(Style::default().fg(Color::Yellow)),
                chunks[0],
            );
        }
        InputMode::Normal => {
            let (text, style) = if let Some(ref msg) = app.status_message {
                (msg.clone(), Style::default().fg(Color::Green))
            } else {
                let hint = match app.view_mode {
                    ViewMode::Playlist => " [Space]Play [h/←]Previous [l/→]Next [+/-]Vol [m]Mute [H]Help [q]Quit ",
                    ViewMode::Browser => " [jk]Nav [l/Enter]Play [h/Backspace]Up [a]Add [s]SaveM3U [S]SaveDir [.]Hidden [~]Home [b/Esc]Close [H]Help [q]Quit ",
                    ViewMode::Playlists => " [jk]Nav [Enter]Load [d]Del [r]Rename [Space]Play [l/→]Next [h/←]Previous [+/-]Vol [p/Esc]Close [H]Help [q]Quit ",
                    ViewMode::Help => " [jk]Scroll [PgUp/PgDn]Page [Esc/?]Close [q]Quit ",
                    ViewMode::TrackInfo => " [Space]Play [l/→]Next [h/←]Previous [Ctrl+h/l/←→]Seek [+/-]Vol [m]Mute [i/Esc]Close [H]Help [q]Quit ",
                    ViewMode::Visualizer => " [Space]Play [l/→]Next [h/←]Previous [Ctrl+h/l/←→]Seek [s]Style [f]FFT/Vol [+/-]Vol [m]Mute [v/Esc]Close [H]Help [q]Quit ",
                    ViewMode::Settings => " [jk]Nav [Enter]Toggle [Space]Play [l/→]Next [h/←]Previous [+/-]Vol [Esc]Close [H]Help [q]Quit ",
                };
                (hint.to_string(), Style::default().fg(Color::DarkGray))
            };
            let hint = Paragraph::new(text).style(style).wrap(Wrap { trim: false });
            frame.render_widget(hint, area);
        }
    }

    // Show cursor in command/search mode
    if app.input_mode != InputMode::Normal {
        let cursor_x = area.x + 2 + app.input_buffer.cursor_char_pos() as u16;
        frame.set_cursor_position((cursor_x, area.y));
    }
}
