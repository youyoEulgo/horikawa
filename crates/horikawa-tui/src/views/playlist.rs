use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    prelude::*,
    style::Color,
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use horikawa_core::{RepeatMode, player::PlaybackState};
use horikawa_protocol::AppCommand;
use crate::view::{ViewMode, VisualizerStyle};
use crate::popup;
pub fn handle(app: &mut crate::App, code: KeyCode, modifiers: KeyModifiers) {
    if app.handle_view_jump(code) {
        return;
    }
    match code {
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        KeyCode::Char(' ') => {
            // Toggle play/pause
            match app.player.state() {
                PlaybackState::Playing | PlaybackState::Paused => {
                    app.send_command(AppCommand::TogglePlayback);
                }
                PlaybackState::Stopped => {
                    // Start playing selected track
                    app.play_selected();
                }
            }
        }
        KeyCode::Char('s') if !app.edit_mode => {
            // Save current playlist as M3U (with name prompt)
            app.popup_state = Some(popup::PopupState::new_input(
                "Save Playlist".to_string(),
                String::new(),
                popup::PendingAction::SaveM3uFromPlaylist,
            ));
        }
        KeyCode::Char('e') => {
            app.edit_mode = !app.edit_mode;
            if app.edit_mode {
                // Persistent — stays until edit mode is turned off
                app.status_message =
                    Some("Edit mode: Shift+J/K to move, d to delete, c to clear".into());
                app.status_clear_at = None;
            } else {
                app.status_message = None;
                app.set_status("Edit mode off");
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.playlist_select_previous();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.playlist_select_next();
        }
        // Edit mode: Shift+J/K to move tracks
        KeyCode::Char('J') if app.edit_mode && modifiers.contains(KeyModifiers::SHIFT) => {
            app.move_track_down();
        }
        KeyCode::Char('K') if app.edit_mode && modifiers.contains(KeyModifiers::SHIFT) => {
            app.move_track_up();
        }
        KeyCode::Char('d') if app.edit_mode => {
            app.delete_selected_track();
        }
        KeyCode::Enter => {
            app.play_selected();
        }
        _ if app.handle_playback_key(code, modifiers) => {}
        KeyCode::Char('c') if app.edit_mode => {
            app.send_command(AppCommand::ClearPlaylist);
            app.set_status("Playlist cleared");
        }
        KeyCode::Char('R') => {
            app.reload_last_playlist();
        }
        KeyCode::Char('r') => {
            // Cycle repeat mode
            let playlist_arc = app.player.playlist();
            let playlist = playlist_arc.read().unwrap();
            let new_mode = match playlist.repeat() {
                RepeatMode::Off => RepeatMode::One,
                RepeatMode::One => RepeatMode::All,
                RepeatMode::All => RepeatMode::Off,
            };
            drop(playlist);
            app.send_command(AppCommand::CycleRepeat);
            app.set_status(format!("Repeat: {:?}", new_mode));
        }
        KeyCode::Char('S') => {
            // Toggle shuffle
            let playlist_arc = app.player.playlist();
            let playlist = playlist_arc.read().unwrap();
            let new_shuffle = !playlist.shuffle();
            drop(playlist);
            app.send_command(AppCommand::ToggleShuffle);
            app.set_status(format!(
                "Shuffle: {}",
                if new_shuffle { "on" } else { "off" }
            ));
        }
        KeyCode::Char('i') => {
            // Show track info
            app.view_mode = ViewMode::TrackInfo;
        }
        KeyCode::Home | KeyCode::Char('g') => {
            app.playlist_state.select(Some(0));
        }
        KeyCode::End | KeyCode::Char('G') => {
            let playlist = app.player.playlist();
            let playlist = playlist.read().unwrap();
            if !playlist.is_empty() {
                app.playlist_state.select(Some(playlist.len() - 1));
            }
        }
        _ => {}
    }
}



pub fn draw(frame: &mut Frame, app: &mut crate::App, area: Rect) {
    // Store area for mouse hit detection
    app.playlist_area = Some(area);

    let playlist = app.player.playlist();
    let playlist = playlist.read().unwrap();

    let playing_index = playlist.current_index();

    // Handle scroll-to-playing without changing selection
    if app.scroll_to_playing {
        if let Some(playing_idx) = playing_index {
            // Calculate visible height (area height minus borders)
            let visible_height = area.height.saturating_sub(2) as usize;
            if visible_height > 0 {
                let current_offset = app.playlist_state.offset();

                // Check if playing track is visible
                let is_visible =
                    playing_idx >= current_offset && playing_idx < current_offset + visible_height;

                if !is_visible {
                    // Scroll to center the playing track
                    let new_offset = playing_idx.saturating_sub(visible_height / 2);
                    *app.playlist_state.offset_mut() = new_offset;
                }
            }
        }
        app.scroll_to_playing = false;
    }

    let items: Vec<ListItem> = playlist
        .tracks()
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");
            let prefix = if Some(i) == playing_index {
                "\u{f04b} "
            } else if app.edit_mode {
                "≡ "
            } else {
                "  "
            };
            ListItem::new(format!("{}{}", prefix, filename))
        })
        .collect();

    let title = format!(
        " Playlist ({}) {} {} ",
        playlist.len(),
        if playlist.shuffle() { "[S]" } else { "" },
        match playlist.repeat() {
            RepeatMode::Off => "",
            RepeatMode::One => "[R1]",
            RepeatMode::All => "[R∞]",
        }
    );

    let border_style = if app.edit_mode {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let highlight_style = if app.edit_mode {
        Style::default().bg(Color::Yellow).fg(Color::Black)
    } else {
        Style::default().bg(Color::DarkGray)
    };

    let playlist_widget = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(highlight_style)
        .highlight_symbol(">> ");

    frame.render_stateful_widget(playlist_widget, area, &mut app.playlist_state);
}

