use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    prelude::*,
    style::Color,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::view::ViewMode;
use crate::popup;
use crate::PlaylistEntry;
use horikawa_protocol::AppCommand;
pub fn handle(app: &mut crate::App, code: KeyCode, modifiers: KeyModifiers) {
    if app.handle_view_jump(code) {
        return;
    }
    match code {
        KeyCode::Char('q') => {
            app.quit();
        }
        KeyCode::Esc => {
            app.view_mode = ViewMode::Playlist;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if !app.playlist_entries.is_empty() {
                app.playlist_list_selected = if app.playlist_list_selected == 0 {
                    app.playlist_entries.len() - 1
                } else {
                    app.playlist_list_selected - 1
                };
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if !app.playlist_entries.is_empty() {
                app.playlist_list_selected =
                    (app.playlist_list_selected + 1) % app.playlist_entries.len();
            }
        }
        KeyCode::Enter => {
            if let Some(entry) = app.playlist_entries.get(app.playlist_list_selected) {
                app.last_loaded = Some(entry.clone());
                match entry {
                    PlaylistEntry::M3u(name) => {
                        app.send_command(AppCommand::LoadPlaylist { name: name.clone() });
                        app.set_status(format!("Loading playlist: {}", name));
                    }
                    PlaylistEntry::DirPl(name) => {
                        app.send_command(AppCommand::LoadDirPlaylist { name: name.clone() });
                        app.set_status(format!("Loading directory playlist: {}", name));
                    }
                }
                app.view_mode = ViewMode::Playlist;
            }
        }
        KeyCode::Char('d') => {
            if let Some(entry) = app.playlist_entries.get(app.playlist_list_selected) {
                app.popup_state = Some(popup::PopupState::new_confirm(
                    "Delete Playlist".to_string(),
                    format!("Delete '{}'?", entry.name()),
                    popup::PendingAction::DeletePlaylist(entry.clone()),
                ));
            }
        }
        KeyCode::Char('r') => {
            if let Some(entry) = app.playlist_entries.get(app.playlist_list_selected) {
                app.popup_state = Some(popup::PopupState::new_input(
                    "Rename Playlist".to_string(),
                    entry.name().to_string(),
                    popup::PendingAction::RenamePlaylist(entry.clone()),
                ));
            }
        }
        _ if app.handle_playback_key(code, modifiers) => {}
        _ => {}
    }
}


pub fn draw(frame: &mut Frame, app: &mut crate::App, area: Rect) {
    let count = app.playlist_entries.len();

    let items: Vec<ListItem> = app
        .playlist_entries
        .iter()
        .map(|entry| {
            let (tag, tag_color) = match entry {
                PlaylistEntry::M3u(_) => ("[M3U]", Color::Cyan),
                PlaylistEntry::DirPl(_) => ("[DIR]", Color::Green),
            };
            let label = format!("  {}  {}", tag, entry.name());
            ListItem::new(label).style(Style::default().fg(tag_color))
        })
        .collect();

    let title = format!(" Playlists ({}) ", count);

    let mut state = ListState::default();
    if !app.playlist_entries.is_empty() {
        state.select(Some(app.playlist_list_selected));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, area, &mut state);
}

