use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    prelude::*,
    style::Color,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::view::ViewMode;
use horikawa_protocol::AppCommand;
pub fn handle(app: &mut crate::App, code: KeyCode, modifiers: KeyModifiers) {
    // Number of settings items
    const SETTINGS_COUNT: usize = 2;

    match code {
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        KeyCode::Esc => {
            app.view_mode = ViewMode::Playlist;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.settings_selected > 0 {
                app.settings_selected -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.settings_selected < SETTINGS_COUNT - 1 {
                app.settings_selected += 1;
            }
        }
        KeyCode::Enter => {
            // Toggle the selected setting (sends command to processor;
            // integrations worker handles the actual enable/disable)
            match app.settings_selected {
                0 => {
                    app.send_command(AppCommand::ToggleSetting {
                        key: "discord_enabled".to_string(),
                    });
                    app.settings.discord_enabled = !app.settings.discord_enabled;
                }
                1 => {
                    app.send_command(AppCommand::ToggleSetting {
                        key: "smtc_enabled".to_string(),
                    });
                    app.settings.smtc_enabled = !app.settings.smtc_enabled;
                }
                _ => {}
            }
        }
        _ if app.handle_playback_key(code, modifiers) => {}
        _ => {}
    }
}



pub fn draw(frame: &mut Frame, app: &crate::App, area: Rect) {
    // Build settings entries: ( name, enabled, locked )
    let settings_items: Vec<(&str, bool, bool)> = vec![
        ("Discord Rich Presence", app.settings.discord_enabled, false),
        (
            "System Media Controls (SMTC)",
            app.settings.smtc_enabled,
            false,
        ),
    ];

    let items: Vec<ListItem> = settings_items
        .iter()
        .enumerate()
        .map(|(idx, (name, enabled, locked))| {
            let checkbox = if *locked {
                if *enabled {
                    "[-]"
                } else {
                    "[-]"
                }
            } else if *enabled {
                "[x]"
            } else {
                "[ ]"
            };

            let label = if *locked {
                format!(" {} {} (locked by CLI)", checkbox, name)
            } else {
                format!(" {} {}", checkbox, name)
            };

            let style = if *locked {
                if idx == app.settings_selected {
                    Style::default().fg(Color::DarkGray).bold()
                } else {
                    Style::default().fg(Color::DarkGray)
                }
            } else if idx == app.settings_selected {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(label).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Settings ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().fg(Color::Yellow).bold());

    frame.render_widget(list, area);
}

