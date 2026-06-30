//! Popup dialog component for the TUI.
//!
//! Provides reusable input and confirmation popups that overlay
//! the current view. When a popup is active, it steals all keyboard
//! input until confirmed or cancelled.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use crossterm::event::KeyCode;


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
    /// Scan a directory and save its audio files as an M3U playlist.
    /// The entered text becomes the playlist name.
    SaveM3uFromBrowser(std::path::PathBuf),
    /// Delete a playlist entry from the playlists view.
    DeletePlaylist(super::PlaylistEntry),
}


/// Complete popup state.
pub struct PopupState {
    pub mode: PopupMode,
    pub action: PendingAction,
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
        Self {
            mode: PopupMode::Input { title, buffer: default_text, cursor },
            action,
        }
    }

    /// Creates a new confirmation popup.
    pub fn new_confirm(title: String, message: String, action: PendingAction) -> Self {
        Self {
            mode: PopupMode::Confirm { title, message },
            action,
        }
    }
}


/// Returns a Rect centered in `parent` with the given percent width/height.
pub fn centered_rect(percent_x: u16, percent_y: u16, parent: Rect) -> Rect {
    let popup_width = (parent.width as f32 * percent_x as f32 / 100.0) as u16;
    let popup_height = (parent.height as f32 * percent_y as f32 / 100.0) as u16;
    let x = parent.x + (parent.width.saturating_sub(popup_width)) / 2;
    let y = parent.y + (parent.height.saturating_sub(popup_height)) / 2;
    Rect::new(x, y, popup_width.min(parent.width), popup_height.min(parent.height))
}


/// Renders the popup overlay on top of the current view.
pub fn draw_popup(frame: &mut Frame, popup: &PopupState, area: Rect) {
    // Erase everything underneath the popup
    frame.render_widget(Clear, area);

    match &popup.mode {
        PopupMode::Input { title, buffer, cursor } => {
            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));
            let inner = block.inner(area);
            frame.render_widget(block, area);

            // Split inner area: gap, input line, hint line
            let chunks = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Min(1),   // top gap
                    ratatui::layout::Constraint::Length(1),// input line
                    ratatui::layout::Constraint::Length(1),// hint line
                ])
                .split(inner);

            // Render the text with cursor indicator
            let display = if buffer.is_empty() {
                " ".to_string()
            } else {
                let mut s = buffer.clone();
                // Insert cursor marker
                if *cursor < s.chars().count() {
                    let byte_pos = s.char_indices()
                        .nth(*cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(s.len());
                    s.insert(byte_pos, '▍');
                } else {
                    s.push('▍');
                }
                s
            };

            let input_para = Paragraph::new(display)
                .style(Style::default().fg(Color::Yellow));
            frame.render_widget(input_para, chunks[1]);

            let hint = Paragraph::new("[Enter] Confirm  [Esc] Cancel")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, chunks[2]);
        }

        PopupMode::Confirm { title, message } => {
            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));
            let inner = block.inner(area);
            frame.render_widget(block, area);

            let chunks = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Min(1),   // top gap
                    ratatui::layout::Constraint::Length(1),// message line
                    ratatui::layout::Constraint::Length(1),// hint line
                ])
                .split(inner);

            let msg = Paragraph::new(message.as_str())
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false });
            frame.render_widget(msg, chunks[1]);

            let hint = Paragraph::new("[Enter/y] Yes  [Esc/n] No")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, chunks[2]);
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
                        // Find the char before cursor and remove it
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

        PopupMode::Confirm { .. } => {
            match code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    PopupResult::Confirmed(None)
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    PopupResult::Cancelled
                }
                _ => PopupResult::StillActive,
            }
        }
    }
}
