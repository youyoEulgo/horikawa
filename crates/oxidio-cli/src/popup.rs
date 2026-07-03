//! Popup dialog component for the TUI.
//!
//! Provides reusable input and confirmation popups that overlay
//! the current view. When a popup is active, it steals all keyboard
//! input until confirmed or cancelled.

use crossterm::event::KeyCode;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Popup mode determines the popup's appearance and behavior.
pub enum PopupMode {
    /// Text input popup — user types a value.
    Input {
        /// Popup title (shown in the border).
        title: String,
        /// Current text buffer.
        buffer: String,
        /// Cursor position in characters (not bytes).
        cursor: usize,
    },
    /// Confirmation popup — user presses Enter/Esc or y/n.
    Confirm {
        /// Popup title (shown in the border).
        title: String,
        /// Confirmation message shown in the popup body.
        message: String,
    },
}

/// Action to execute when the popup is confirmed.
///
/// New variants can be added as needed without changing the popup machinery.
pub enum PendingAction {
    /// No action — popup is informational only.
    None,
    /// Scan a directory and save its audio files as an M3U playlist.
    SaveM3uFromBrowser(std::path::PathBuf),
    /// Save a directory reference as a .oxidio directory playlist.
    SaveDirPlFromBrowser(std::path::PathBuf),
    /// Save the current playlist tracks as an M3U file.
    SaveM3uFromPlaylist,
    /// Delete a playlist entry from the playlists view.
    DeletePlaylist(super::PlaylistEntry),
    /// Rename a playlist entry.
    RenamePlaylist(super::PlaylistEntry),
}

/// Complete popup state.
pub struct PopupState {
    pub mode: PopupMode,
    pub action: PendingAction,
    /// Preferred height in terminal rows (0 = use default).
    pub preferred_height: u16,
    /// If true, the popup is informational (Enter/Esc both close).
    pub is_info: bool,
}

/// Result returned from `handle_popup_key` after processing a keystroke.
pub enum PopupResult {
    /// Popup is still active; caller should re-store `PopupState`.
    StillActive,
    /// User confirmed the popup.
    /// `Some(name)` for Input mode, `None` for Confirm mode.
    Confirmed(Option<String>),
    /// User cancelled the popup.
    Cancelled,
}

impl PopupState {
    /// Creates a new text-input popup.
    pub fn new_input(title: String, default_text: String, action: PendingAction) -> Self {
        let cursor = default_text.chars().count();
        Self { mode: PopupMode::Input { title, buffer: default_text, cursor }, action, preferred_height: 0, is_info: false }
    }

    /// Creates a new confirmation popup.
    pub fn new_confirm(title: String, message: String, action: PendingAction) -> Self {
        Self { mode: PopupMode::Confirm { title, message }, action, preferred_height: 0, is_info: false }
    }

    /// Creates a confirmation popup with a custom height.
    pub fn new_confirm_tall(title: String, message: String, action: PendingAction, height: u16) -> Self {
        Self { mode: PopupMode::Confirm { title, message }, action, preferred_height: height, is_info: true }
    }
}

/// Returns a Rect centered in `parent`, percentage width, absolute height.
pub fn centered_rect(width_pct: u16, height: u16, parent: Rect) -> Rect {
    let popup_width = ((parent.width as f32) * (width_pct as f32) / 100.0) as u16;
    let popup_height = height.min(parent.height);
    let x = parent.x + (parent.width.saturating_sub(popup_width)) / 2;
    let y = parent.y + (parent.height.saturating_sub(popup_height)) / 2;
    Rect::new(x, y, popup_width.min(parent.width), popup_height)
}

/// Renders the popup overlay on top of the current view.
pub fn draw_popup(frame: &mut Frame, popup: &PopupState, area: Rect) {
    let reset = Style::reset();

    // Clear the popup area plus one extra column on each side to kill
    // CJK double-width character leftovers from the underlying view.
    // CJK chars only leak horizontally, not vertically, so we don't
    // need extra padding on the Y axis.
    let pad_x = area.x.saturating_sub(1);
    let pad_w = (area.width + 2).min(frame.area().width.saturating_sub(pad_x));
    let blank_line = " ".repeat(pad_w as usize);
    for y in area.y..area.y + area.height {
        frame.render_widget(
            Paragraph::new(Line::styled(&blank_line, reset)),
            Rect::new(pad_x, y, pad_w, 1),
        );
    }

    match &popup.mode {
        PopupMode::Input { title, buffer, cursor } => {
            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            let inner = block.inner(area);

            for y in inner.y..inner.y + inner.height {
                let fill = " ".repeat(inner.width as usize);
                frame.render_widget(
                    Paragraph::new(Line::styled(fill, reset)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
            }

            frame.render_widget(block, area);

            let chunks = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Length(2),
                    ratatui::layout::Constraint::Length(1),
                ])
                .split(inner);

            if buffer.is_empty() {
                let ph = Span::styled("Name your playlist...", Style::default().fg(Color::DarkGray));
                let c = Span::styled("▍", Style::default().fg(Color::Yellow));
                frame.render_widget(Paragraph::new(Line::from(vec![c, ph])), chunks[0]);
            } else {
                let mut s = buffer.clone();
                if *cursor < s.chars().count() {
                    let bp = s.char_indices().nth(*cursor).map(|(i, _)| i).unwrap_or(s.len());
                    s.insert(bp, '▍');
                } else {
                    s.push('▍');
                }
                frame.render_widget(Paragraph::new(s).style(Style::default().fg(Color::Yellow)), chunks[0]);
            }

            let hint = Paragraph::new("[Enter] Confirm  [Esc] Cancel")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, chunks[1]);
        }

        PopupMode::Confirm { title, message } => {
            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));
            let inner = block.inner(area);

            for y in inner.y..inner.y + inner.height {
                let fill = " ".repeat(inner.width as usize);
                frame.render_widget(
                    Paragraph::new(Line::styled(fill, reset)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
            }

            frame.render_widget(block, area);

            let chunks = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Min(2),
                    ratatui::layout::Constraint::Length(1),
                ])
                .split(inner);

            let msg = Paragraph::new(message.as_str())
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false });
            frame.render_widget(msg, chunks[0]);

            let hint_text = if popup.is_info {
                "[Enter/H/Esc] Close"
            } else {
                "[Enter/y] Yes  [Esc/n] No"
            };
            let hint = Paragraph::new(hint_text)
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, chunks[1]);
        }
    }
}

/// Handles a key event when the popup is active.
///
/// Returns:
/// - `PopupResult::StillActive` if the popup is still open
/// - `PopupResult::Confirmed(Some(name))` if input was confirmed
/// - `PopupResult::Confirmed(None)` if confirmation was accepted
/// - `PopupResult::Cancelled` if the user cancelled
pub fn handle_popup_key(popup: &mut PopupState, code: KeyCode) -> PopupResult {
    match &mut popup.mode {
        PopupMode::Input { buffer, cursor, .. } => {
            match code {
                KeyCode::Enter => {
                    let result = buffer.clone();
                    PopupResult::Confirmed(Some(result))
                }
                KeyCode::Esc => PopupResult::Cancelled,
                KeyCode::Backspace => {
                    if *cursor > 0 {
                        let char_idx = buffer
                            .char_indices()
                            .nth(*cursor - 1)
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        buffer.remove(char_idx);
                        *cursor -= 1;
                    }
                    PopupResult::StillActive
                }
                KeyCode::Delete => {
                    if *cursor < buffer.chars().count() {
                        let byte_pos = buffer
                            .char_indices()
                            .nth(*cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(buffer.len());
                        buffer.remove(byte_pos);
                    }
                    PopupResult::StillActive
                }
                KeyCode::Left => {
                    if *cursor > 0 {
                        *cursor -= 1;
                    }
                    PopupResult::StillActive
                }
                KeyCode::Right => {
                    if *cursor < buffer.chars().count() {
                        *cursor += 1;
                    }
                    PopupResult::StillActive
                }
                KeyCode::Home => {
                    *cursor = 0;
                    PopupResult::StillActive
                }
                KeyCode::End => {
                    *cursor = buffer.chars().count();
                    PopupResult::StillActive
                }
                KeyCode::Char(c) => {
                    let byte_pos = buffer
                        .char_indices()
                        .nth(*cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(buffer.len());
                    buffer.insert(byte_pos, c);
                    *cursor += 1;
                    PopupResult::StillActive
                }
                _ => PopupResult::StillActive,
            }
        }

        PopupMode::Confirm { .. } => match code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                PopupResult::Confirmed(None)
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => PopupResult::Cancelled,
            _ => PopupResult::StillActive,
        },
    }
}
