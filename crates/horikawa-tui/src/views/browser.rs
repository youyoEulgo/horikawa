use crossterm::event::KeyCode;
use ratatui::{
    prelude::*,
    style::Color,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::view::ViewMode;
use crate::popup;
use horikawa_protocol::AppCommand;

fn title_for_path(path: &str) -> String {
    const MAX_CHARS: usize = 50;
    const TAIL_CHARS: usize = 47;

    if path.chars().count() > MAX_CHARS {
        let tail: String = path
            .chars()
            .rev()
            .take(TAIL_CHARS)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!(" ...{} ", tail)
    } else {
        format!(" {} ", path)
    }
}

pub fn handle(app: &mut crate::App, code: KeyCode) {
    let visible = crate::App::browser_visible_rows();
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
            app.browser.select_previous(visible);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.browser.select_next(visible);
        }
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            if let Ok(Some(file_path)) = app.browser.enter_selected() {
                app.send_command(AppCommand::PlayPath {
                    path: file_path.to_string_lossy().to_string(),
                });
            }
        }
        KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => {
            let _ = app.browser.go_up();
        }
        KeyCode::Char('a') => {
            if let Some(entry) = app.browser.selected_entry() {
                let path = entry.path.clone();
                let is_dir = entry.is_dir;
                let is_audio = entry.is_audio;
                if (is_dir && entry.name != "..") || is_audio {
                    app.send_command(AppCommand::AddPath {
                        path: path.to_string_lossy().to_string(),
                    });
                }
            }
        }
        KeyCode::Char('s') => {
            // Save selected dir as M3U (popup to name)
            if let Some(entry) = app.browser.selected_entry() {
                let (dir, default_name) = if entry.is_dir && entry.name != ".." {
                    (entry.path.clone(), entry.name.clone())
                } else if entry.name == ".." {
                    app.set_status("Cannot save parent directory");
                    return;
                } else {
                    // File: use its parent directory
                    (
                        entry.path.parent().unwrap_or(&entry.path).to_path_buf(),
                        entry
                            .path
                            .parent()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| entry.name.clone()),
                    )
                };
                app.popup_state = Some(popup::PopupState::new_input(
                    "Save M3U Playlist".to_string(),
                    default_name,
                    popup::PendingAction::SaveM3uFromBrowser(dir),
                ));
            }
        }
        KeyCode::Char('S') => {
            // Save selected dir as .horikawa (popup to name)
            if let Some(entry) = app.browser.selected_entry() {
                let (dir, default_name) = if entry.is_dir && entry.name != ".." {
                    (entry.path.clone(), entry.name.clone())
                } else if entry.name == ".." {
                    app.set_status("Cannot save parent directory");
                    return;
                } else {
                    (
                        entry.path.parent().unwrap_or(&entry.path).to_path_buf(),
                        entry
                            .path
                            .parent()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| entry.name.clone()),
                    )
                };
                app.popup_state = Some(popup::PopupState::new_input(
                    "Save Dir Playlist".to_string(),
                    default_name,
                    popup::PendingAction::SaveDirPlFromBrowser(dir),
                ));
            }
        }
        KeyCode::Char('R') => {
            let _ = app.browser.refresh();
            app.set_status("Refreshed");
        }
        KeyCode::Char('.') => {
            let _ = app.browser.toggle_hidden();
            app.set_status("Toggled hidden files");
        }
        KeyCode::Home | KeyCode::Char('g') => {
            app.browser.select_first(visible);
        }
        KeyCode::End | KeyCode::Char('G') => {
            app.browser.select_last(visible);
        }
        KeyCode::Char('~') => {
            if let Some(home) = dirs::home_dir() {
                let _ = app.browser.navigate_to(&home);
            }
        }
        _ => {}
    }
}



pub fn draw(frame: &mut Frame, app: &mut crate::App, area: Rect) {
    let path_str = app.browser.current_dir().display().to_string();
    let title = title_for_path(&path_str);

    let items: Vec<ListItem> = app
        .browser
        .visible_entries()
        .iter()
        .map(|entry| {
            let icon = if entry.is_dir {
                "\u{f07b} "
            } else if entry.is_audio {
                "\u{f001} "
            } else {
                "  "
            };

            let style = if entry.is_dir {
                Style::default().fg(Color::Blue)
            } else if entry.is_audio {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            ListItem::new(format!(" {} {}", icon, entry.name)).style(style)
        })
        .collect();

    let all_items = items;
    let scroll = app.browser.scroll_offset();
    let total = all_items.len();
    let visible_height = area.height.saturating_sub(2) as usize;
    let end = (scroll + visible_height).min(total);
    let slice = &all_items[scroll..end];

    let mut state = ListState::default();
    let rel = app.browser.selected_index().saturating_sub(scroll);
    state.select(Some(rel.min(visible_height.saturating_sub(1))));

    let browser_widget = List::new(slice.to_vec())
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol(">> ");

    frame.render_stateful_widget(browser_widget, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::title_for_path;

    #[test]
    fn title_for_path_truncates_utf8_safely() {
        let title = title_for_path("/run/media/eulgo/系统/Users/Eulgo/Documents/BaiduSyncdisk/music");
        assert!(title.starts_with(" ..."));
        assert!(title.ends_with(" "));
        assert!(title.contains("系统"));
    }
}
