use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{prelude::*, style::Color, widgets::{Block, Borders, Paragraph, Wrap}};

use crate::view::ViewMode;
pub fn handle(app: &mut crate::App, code: KeyCode, modifiers: KeyModifiers) {
    if app.handle_view_jump(code) {
        return;
    }
    match code {
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        KeyCode::Esc => {
            app.view_mode = ViewMode::Playlist;
        }
        _ if app.handle_playback_key(code, modifiers) => {}
        _ => {}
    }
}



pub fn draw(frame: &mut Frame, app: &crate::App, area: Rect) {
    let mut lines = Vec::new();

    // Get the track path - either currently playing or selected
    let track_path = app.player.current_track().or_else(|| {
        app.playlist_state.selected().and_then(|idx| {
            let playlist = app.player.playlist();
            let playlist = playlist.read().unwrap();
            playlist.tracks().get(idx).cloned()
        })
    });

    if let Some(ref path) = track_path {
        // Get metadata
        let meta = app.player.metadata();

        // Title (always show)
        let title = meta
            .as_ref()
            .and_then(|m| m.title.clone())
            .or_else(|| path.file_stem().and_then(|n| n.to_str()).map(String::from))
            .unwrap_or_else(|| "Unknown".to_string());
        lines.push(Line::from(vec![
            Span::styled("Title:  ", Style::default().fg(Color::Gray)),
            Span::styled(title, Style::default().fg(Color::Cyan).bold()),
        ]));

        // Artist (always show)
        let artist = meta
            .as_ref()
            .and_then(|m| m.artist.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        lines.push(Line::from(vec![
            Span::styled("Artist: ", Style::default().fg(Color::Gray)),
            Span::styled(artist, Style::default().fg(Color::Yellow)),
        ]));

        // Album (always show)
        let album = meta
            .as_ref()
            .and_then(|m| m.album.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        lines.push(Line::from(vec![
            Span::styled("Album:  ", Style::default().fg(Color::Gray)),
            Span::styled(album, Style::default().fg(Color::Green)),
        ]));

        lines.push(Line::from(""));

        // Additional metadata (only if available)
        if let Some(ref meta) = meta {
            if let Some(album_artist) = &meta.album_artist {
                lines.push(Line::from(vec![
                    Span::styled("Album Artist: ", Style::default().fg(Color::Gray)),
                    Span::raw(album_artist.clone()),
                ]));
            }
            if let Some(track_num) = meta.track_number {
                lines.push(Line::from(vec![
                    Span::styled("Track #: ", Style::default().fg(Color::Gray)),
                    Span::raw(track_num.to_string()),
                ]));
            }
            if let Some(genre) = &meta.genre {
                lines.push(Line::from(vec![
                    Span::styled("Genre:  ", Style::default().fg(Color::Gray)),
                    Span::raw(genre.clone()),
                ]));
            }
            if let Some(year) = meta.year {
                lines.push(Line::from(vec![
                    Span::styled("Year:   ", Style::default().fg(Color::Gray)),
                    Span::raw(year.to_string()),
                ]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─── Audio Format ───",
            Style::default().fg(Color::DarkGray),
        )));

        // Audio format information
        if let Some(ref meta) = meta {
            // Codec
            if let Some(codec) = &meta.codec {
                lines.push(Line::from(vec![
                    Span::styled("Codec:       ", Style::default().fg(Color::Gray)),
                    Span::raw(codec.clone()),
                ]));
            }

            // Bitrate
            if let Some(bitrate) = meta.bitrate {
                lines.push(Line::from(vec![
                    Span::styled("Bitrate:     ", Style::default().fg(Color::Gray)),
                    Span::raw(format!("{} kbps", bitrate)),
                ]));
            }

            // Sample rate
            if let Some(sample_rate) = meta.sample_rate {
                lines.push(Line::from(vec![
                    Span::styled("Sample Rate: ", Style::default().fg(Color::Gray)),
                    Span::raw(format!("{} Hz", sample_rate)),
                ]));
            }

            // Channels
            if let Some(channels) = meta.channels {
                let ch_str = match channels {
                    1 => "Mono".to_string(),
                    2 => "Stereo".to_string(),
                    n => format!("{} channels", n),
                };
                lines.push(Line::from(vec![
                    Span::styled("Channels:    ", Style::default().fg(Color::Gray)),
                    Span::raw(ch_str),
                ]));
            }
        }

        // Duration
        if let Some(duration) = app.player.duration() {
            let secs = duration.as_secs();
            lines.push(Line::from(vec![
                Span::styled("Duration:    ", Style::default().fg(Color::Gray)),
                Span::raw(format!("{}:{:02}", secs / 60, secs % 60)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─── File ───",
            Style::default().fg(Color::DarkGray),
        )));

        // Filename
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown");
        lines.push(Line::from(vec![
            Span::styled("File: ", Style::default().fg(Color::Gray)),
            Span::raw(filename.to_string()),
        ]));

        // Full path
        lines.push(Line::from(vec![
            Span::styled("Path: ", Style::default().fg(Color::Gray)),
            Span::raw(path.display().to_string()),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "No track selected or playing",
            Style::default().fg(Color::DarkGray).italic(),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Select a track in the playlist and press 'i' to view its info,",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "or start playing a track first.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let info = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Track Info (press i or Esc to close) ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(info, area);
}

