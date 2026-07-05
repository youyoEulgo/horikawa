//! Help view handler and rendering.

use crossterm::event::KeyCode;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use horikawa_core::command;
use crate::view::ViewMode;

/// Shortcuts popup content for the Help view.
pub(crate) const SHORTCUTS: &str = r#"Help Shortcuts

  Help:
  j/↓    Scroll Down    k/↑  Scroll Up
  PgDn   Page Down      PgUp Page Up

  Other:
  /      Cmd            Esc/? Close          H    Shortcuts
  q      Quit
  "#;

pub(crate) fn handle(app: &mut crate::App, code: KeyCode) {
    match code {
        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('?') => {
            app.view_mode = ViewMode::Playlist;
            app.help_scroll = 0;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.help_scroll = app.help_scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.help_scroll = app.help_scroll.saturating_add(1);
        }
        KeyCode::PageUp => {
            app.help_scroll = app.help_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            app.help_scroll = app.help_scroll.saturating_add(10);
        }
        KeyCode::Home => {
            app.help_scroll = 0;
        }
        _ => {}
    }
}

pub(crate) fn draw(frame: &mut Frame, app: &mut crate::App, area: Rect) {
    let help_text = command::help_text();
    let line_count = help_text.lines().count() as u16;
    let visible_height = area.height.saturating_sub(2);

    let max_scroll = line_count.saturating_sub(visible_height);
    if app.help_scroll > max_scroll {
        app.help_scroll = max_scroll;
    }

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help (↑↓ scroll, ? or Esc to close) ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.help_scroll, 0));

    frame.render_widget(help, area);
}
