//! Horikawa CLI - Terminal UI music player
#![allow(unexpected_cfgs)]

mod browser;
mod discord;
mod input;
pub mod integrations;
pub mod media_controls;
mod popup;
mod settings;
mod view;

mod views;

use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};
use ratatui::{
    layout::Alignment,
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use browser::FileBrowser;
use input::{InputBuffer, InputMode};
use view::{ViewMode, VisualizerStyle};

use horikawa_core::{
    command::{get_next_word_chunk, get_suggestion, DirPlCmd, RepeatModeArg},
    library::LibraryScanner,
    player::PlaybackState,
    Command, Player, RepeatMode,
};
use horikawa_ctl::CommandSender;
use horikawa_protocol::{AppCommand, StateUpdate};

/// Entry type for the Playlists view.
#[derive(Clone)]
pub enum PlaylistEntry {
    /// M3U playlist file (.m3u)
    M3u(String),
    /// Directory playlist file (.horikawa)
    DirPl(String),
}

impl PlaylistEntry {
    /// Returns the display name of the playlist.
    fn name(&self) -> &str {
        match self {
            PlaylistEntry::M3u(n) | PlaylistEntry::DirPl(n) => n,
        }
    }
}

/// Application state.
pub struct App {
    pub player: Arc<Player>,
    pub command_sender: CommandSender,
    pub state_rx: tokio::sync::broadcast::Receiver<StateUpdate>,
    pub should_quit: bool,

    // View state
    pub view_mode: ViewMode,
    pub playlist_state: ListState,
    pub browser: FileBrowser,

    // Input state
    pub input_mode: InputMode,
    pub input_buffer: InputBuffer,
    pub command_ghost: Option<String>,

    // Edit mode
    pub edit_mode: bool,

    // Visualizer style
    pub visualizer_style: VisualizerStyle,

    // Visualizer data source: true = FFT spectrum, false = RMS volume
    pub spectrum_mode: bool,

    // Volume (0.0 to 1.5), synced from player via VolumeChanged
    pub volume: f32,

    // Flag to scroll to playing track without changing selection
    pub scroll_to_playing: bool,

    // Mouse click tracking for double-click detection
    pub last_click_time: Option<std::time::Instant>,
    pub last_click_row: Option<u16>,

    // Store playlist area for mouse hit detection
    pub playlist_area: Option<Rect>,

    // Help view scroll offset
    pub help_scroll: u16,

    // Track change detection (for auto-scroll on advance)
    pub last_track: Option<PathBuf>,

    // Status message (shown in status bar)
    pub status_message: Option<String>,
    pub status_clear_at: Option<std::time::Instant>,

    // Settings
    pub settings: settings::Settings,
    pub settings_selected: usize,

    // Playlists view state
    pub playlist_entries: Vec<PlaylistEntry>,
    pub playlist_list_selected: usize,

    // Last loaded playlist (for quick reload)
    pub last_loaded: Option<PlaylistEntry>,

    // Pending playlist list refresh (after delete)
    pub needs_playlist_refresh: bool,

    // Popup state (input or confirm dialog)
    pub popup_state: Option<popup::PopupState>,

    // macOS media controls (must run on main thread)
    #[cfg(target_os = "macos")]
    pub media_controls: Option<crate::media_controls::MediaControlsHandler>,
    #[cfg(target_os = "macos")]
    pub smtc_rx: Option<mpsc::Receiver<crate::media_controls::MediaControlCommand>>,
    #[cfg(target_os = "macos")]
    pub last_smtc_state: Option<PlaybackState>,
    #[cfg(target_os = "macos")]
    pub last_smtc_track: Option<PathBuf>,
}

impl App {
    /// Creates a new App instance.
    ///
    /// The Player and playlist must already be initialized (created and loaded
    /// in main). App reads initial state (volume, playlist index) from the player.
    ///
    /// @param player - Shared player instance (also held by the CommandProcessor)
    /// @param command_sender - Sender for the control channel
    /// @param state_rx - Broadcast receiver for state updates from the processor
    /// @param args - CLI arguments
    pub fn new(
        player: Arc<Player>,
        command_sender: CommandSender,
        state_rx: tokio::sync::broadcast::Receiver<StateUpdate>,
        start_path: Option<PathBuf>,
        browse: bool,
    ) -> Result<Self> {
        let start_path = start_path
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let browser = FileBrowser::new(start_path)?;

        // Determine starting view
        let view_mode = if browse {
            ViewMode::Browser
        } else {
            ViewMode::Playlist
        };

        // Read initial state from the shared player
        let volume = player.volume();
        let playlist_index = {
            let playlist_arc = player.playlist();
            let playlist = playlist_arc.read().unwrap();
            playlist.current_index()
        };

        let mut playlist_state = ListState::default();
        if playlist_index.is_some() {
            playlist_state.select(playlist_index);
        }

        Ok(Self {
            player,
            command_sender,
            state_rx,
            should_quit: false,
            view_mode,
            playlist_state,
            browser,
            input_mode: InputMode::Normal,
            input_buffer: InputBuffer::new(),
            command_ghost: None,
            edit_mode: false,
            visualizer_style: VisualizerStyle::default(),
            spectrum_mode: true,
            volume,
            scroll_to_playing: false,
            last_click_time: None,
            last_click_row: None,
            playlist_area: None,
            help_scroll: 0,
            last_track: None,
            status_message: None,
            status_clear_at: None,
            settings: settings::Settings::load(),
            settings_selected: 0,
            playlist_entries: Vec::new(),
            playlist_list_selected: 0,
            last_loaded: None,
            needs_playlist_refresh: false,
            popup_state: None,
            #[cfg(target_os = "macos")]
            media_controls: None,
            #[cfg(target_os = "macos")]
            smtc_rx: None,
            #[cfg(target_os = "macos")]
            last_smtc_state: None,
            #[cfg(target_os = "macos")]
            last_smtc_track: None,
        })
    }

    /// Sets a status message that auto-clears after a delay.
    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_clear_at = Some(std::time::Instant::now() + Duration::from_secs(3));
    }

    /// Sends a command to the control channel (non-blocking).
    fn send_command(&self, cmd: AppCommand) {
        if let Err(e) = self.command_sender.try_send(cmd) {
            tracing::warn!("Failed to send command: {}", e);
        }
    }

    /// Updates app state (clears expired messages, detects track changes, syncs settings).
    pub fn tick(&mut self) {
        // Clear expired status messages
        if let Some(clear_at) = self.status_clear_at {
            if std::time::Instant::now() >= clear_at {
                self.status_message = None;
                self.status_clear_at = None;
            }
        }

        // Drain broadcast receiver for settings display sync
        // (Discord/SMTC are managed by the integrations worker, but the TUI
        // needs to know current settings values for the settings view)
        loop {
            match self.state_rx.try_recv() {
                Ok(StateUpdate::SettingsChanged { settings }) => {
                    self.settings.discord_enabled = settings.discord_enabled;
                    self.settings.smtc_enabled = settings.smtc_enabled;
                }
                Ok(StateUpdate::StatusMessage { message }) => {
                    self.status_message = Some(message);
                    self.status_clear_at = Some(std::time::Instant::now() + Duration::from_secs(3));
                }
                Ok(StateUpdate::VolumeChanged { level }) => {
                    self.volume = level;
                }
                Ok(_) => {} // Ignore other updates
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }

        // Detect track changes from the CommandProcessor (auto-advance)
        {
            let current_track = self.player.current_track();
            let playlist_index = {
                let playlist_arc = self.player.playlist();
                let playlist = playlist_arc.read().unwrap();
                playlist.current_index()
            };

            // If the playing track changed (e.g. auto-advance by processor), scroll to it
            if playlist_index.is_some() && self.player.state() == PlaybackState::Playing {
                if current_track != self.last_track {
                    self.scroll_to_playing = true;
                }
            }
            self.last_track = current_track;
        }

        // macOS: update Now Playing media controls from the main thread
        self.tick_media_controls();

        // Delayed playlist list refresh after delete commands
        if self.needs_playlist_refresh {
            self.refresh_playlist_lists();
            self.needs_playlist_refresh = false;
        }
    }

    /// Updates macOS Now Playing controls (playback state + metadata)
    /// and forwards media key events as AppCommands.
    #[cfg(target_os = "macos")]
    fn tick_media_controls(&mut self) {
        use souvlaki::{MediaMetadata, MediaPlayback};

        // Handle SMTC enable/disable toggle
        let smtc_enabled = self.settings.smtc_enabled;
        if smtc_enabled && self.media_controls.is_none() {
            let (tx, rx) = mpsc::channel();
            self.media_controls = crate::media_controls::MediaControlsHandler::new(tx);
            self.smtc_rx = Some(rx);
            self.last_smtc_state = None;
            self.last_smtc_track = None;
        } else if !smtc_enabled && self.media_controls.is_some() {
            self.media_controls = None;
            self.smtc_rx = None;
        }

        // Drain media-key events from MPRemoteCommandCenter callbacks
        if let Some(ref rx) = self.smtc_rx {
            while let Ok(cmd) = rx.try_recv() {
                let app_cmd = match cmd {
                    // macOS Control Center sends Play when resuming from pause.
                    // Map Play→Resume when paused so the track doesn't restart.
                    crate::media_controls::MediaControlCommand::Play => {
                        if self.player.state() == PlaybackState::Paused {
                            AppCommand::Resume
                        } else {
                            AppCommand::Play
                        }
                    }
                    crate::media_controls::MediaControlCommand::Pause => AppCommand::Pause,
                    crate::media_controls::MediaControlCommand::Toggle => {
                        AppCommand::TogglePlayback
                    }
                    crate::media_controls::MediaControlCommand::Stop => AppCommand::Stop,
                    crate::media_controls::MediaControlCommand::Next => AppCommand::Next,
                    crate::media_controls::MediaControlCommand::Previous => AppCommand::Previous,
                };
                let _ = self.command_sender.try_send(app_cmd);
            }
        }

        if let Some(ref mut controls) = self.media_controls {
            let state = self.player.state();
            let current_track = self.player.current_track();

            // Force update on first run or state change
            let force = self.last_smtc_track.is_none();

            // Update playback state if changed
            if force || self.last_smtc_state != Some(state) {
                let playback = match state {
                    PlaybackState::Playing => MediaPlayback::Playing { progress: None },
                    PlaybackState::Paused => MediaPlayback::Paused { progress: None },
                    PlaybackState::Stopped => MediaPlayback::Stopped,
                };
                controls.set_playback(playback);
                self.last_smtc_state = Some(state);
            }

            // Update metadata if track changed or forced
            if force || self.last_smtc_track != current_track {
                if let Some(ref track_path) = current_track {
                    let metadata = self.player.metadata();

                    let title = metadata.as_ref().and_then(|m| m.title.clone()).or_else(|| {
                        track_path
                            .file_stem()
                            .map(|n| n.to_string_lossy().to_string())
                    });

                    let artist = metadata.as_ref().and_then(|m| m.artist.clone());
                    let album = metadata.as_ref().and_then(|m| m.album.clone());

                    controls.set_metadata(MediaMetadata {
                        title: title.as_deref(),
                        artist: artist.as_deref(),
                        album: album.as_deref(),
                        cover_url: None,
                        duration: self.player.duration(),
                    });
                } else {
                    // No track playing — clear Now Playing
                    controls.set_metadata(MediaMetadata {
                        title: None,
                        artist: None,
                        album: None,
                        cover_url: None,
                        duration: None,
                    });
                }
                self.last_smtc_track = current_track;
            }
        }
    }

    /// Stub for non-macOS platforms.
    #[cfg(not(target_os = "macos"))]
    fn tick_media_controls(&mut self) {}
    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        // Popup steals all input when active
        if let Some(mut popup) = self.popup_state.take() {
            // H closes informational popups
            if popup.is_info && code == KeyCode::Char('H') {
                return;
            }
            match popup::handle_popup_key(&mut popup, code) {
                popup::PopupResult::StillActive => {
                    self.popup_state = Some(popup);
                }
                popup::PopupResult::Confirmed(name) => {
                    self.execute_popup_action(popup.action, name);
                }
                popup::PopupResult::Cancelled => {}
            }
            return;
        }

        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(code, modifiers),
            InputMode::Command => self.handle_command_key(code),
            InputMode::Search => self.handle_search_key(code),
        }
    }

    /// Handles mouse events.
    pub fn handle_mouse(&mut self, column: u16, row: u16, kind: MouseEventKind) {
        match kind {
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                // Check if click is within the playlist area
                if self.view_mode == ViewMode::Playlist {
                    if let Some(area) = self.playlist_area {
                        // Check if click is within the playlist (inside borders)
                        if column > area.x
                            && column < area.x + area.width - 1
                            && row > area.y
                            && row < area.y + area.height - 1
                        {
                            // Calculate which item was clicked
                            let offset = self.playlist_state.offset();
                            let clicked_idx = offset + (row - area.y - 1) as usize;

                            let playlist = self.player.playlist();
                            let playlist_len = playlist.read().unwrap().len();

                            if clicked_idx < playlist_len {
                                let now = std::time::Instant::now();
                                let is_double_click = self
                                    .last_click_time
                                    .map(|t| now.duration_since(t) < Duration::from_millis(400))
                                    .unwrap_or(false)
                                    && self.last_click_row == Some(row);

                                if is_double_click {
                                    // Double-click: select and play
                                    self.playlist_state.select(Some(clicked_idx));
                                    self.play_selected();
                                    self.last_click_time = None;
                                    self.last_click_row = None;
                                } else {
                                    // Single click: select
                                    self.playlist_state.select(Some(clicked_idx));
                                    self.last_click_time = Some(now);
                                    self.last_click_row = Some(row);
                                }
                            }
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                // Scroll playlist up
                if self.view_mode == ViewMode::Playlist {
                    self.playlist_select_previous();
                }
            }
            MouseEventKind::ScrollDown => {
                // Scroll playlist down
                if self.view_mode == ViewMode::Playlist {
                    self.playlist_select_next();
                }
            }
            _ => {}
        }
    }

    /// View-switch keys shared across Playlist, Browser, Playlists, TrackInfo, Visualizer.
    /// Pressing the same key again returns to Playlist.
    fn handle_view_jump(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('b') => {
                self.view_mode = if self.view_mode == ViewMode::Browser {
                    ViewMode::Playlist
                } else {
                    ViewMode::Browser
                };
                true
            }
            KeyCode::Char('v') => {
                self.view_mode = if self.view_mode == ViewMode::Visualizer {
                    ViewMode::Playlist
                } else {
                    ViewMode::Visualizer
                };
                true
            }
            KeyCode::Char('p') => {
                if self.view_mode == ViewMode::Playlists {
                    self.view_mode = ViewMode::Playlist;
                } else {
                    self.refresh_playlist_lists();
                    self.view_mode = ViewMode::Playlists;
                }
                true
            }
            KeyCode::Char('i') => {
                self.view_mode = if self.view_mode == ViewMode::TrackInfo {
                    ViewMode::Playlist
                } else {
                    ViewMode::TrackInfo
                };
                true
            }
            _ => false,
        }
    }

    fn handle_normal_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        // Global keys (work in any view)
        match code {
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Command;
                self.input_buffer.clear();
                return;
            }
            KeyCode::Tab => {
                self.view_mode = self.view_mode.next_tab();
                if self.view_mode == ViewMode::Playlists {
                    self.refresh_playlist_lists();
                }
                return;
            }
            KeyCode::BackTab => {
                // Shift+Tab goes to previous view
                self.view_mode = self.view_mode.prev_tab();
                if self.view_mode == ViewMode::Playlists {
                    self.refresh_playlist_lists();
                }
                return;
            }
            KeyCode::Char('?') => {
                self.view_mode = ViewMode::Help;
                return;
            }
            KeyCode::Char('H') => {
                let shortcuts = match self.view_mode {
                    ViewMode::Playlist => {
                        r#"Playlist Shortcuts

  Playlist:
  Space  Play/Pause     h/←  Previous       l/→  Next
  Enter  Play Selected  k/↑  Navigate Up    j/↓  Navigate Down  
  +/-    Volumes        s    Save M3U       S    Shuffle         
  M      Mute           r    Repeat         R    Reload         
  Ctrl+h/← Rewind 10 seconds
  Ctrl+l/→ Fast forward 10 seconds

  Edit Mode:
  e      Edit Mode
  Shift+j/k Move        d    Delete         c    Clear

  Views:
  Tab    Next View      Shift+Tab Previous View
  v      Visualizer     p    Playlists
  b      Browser        i    Track Info

  Other:
  /      Cmd            H    Shortcuts      q    Quit

  "#
                    }
                    ViewMode::Browser => {
                        r#"Browser Shortcuts

  Browser:
  h/←       Return to parent directory
  l/→       Enter the selected directory
  k/↑       Navigate up
  j/↓       Navigate down
  a         Add to playlist
  Enter     Add to playlist and play
  s         Save as M3U playlist
  S         Save as Dir playlist
  R         Refresh
  ~         Home

  Views:
  Tab    Next View      Shift+Tab Previous View
  v      Visualizer     p    Playlists
  b/Esc  Playlist       i    Track Info

  Other:
  /      Cmd            H    Shortcuts      q    Quit

  "#
                    }
                    ViewMode::Playlists => {
                        r#"Playlists Shortcuts

  Playlists:
  Enter  Load           d    Delete         r    Rename         
  Space  Play/Pause     k/↑  Navigate Up    j/↓  Navigate Down
  +/-    Volumes        h/←  Previous       l/→  Next
  m      Mute

  Views:
  Tab    Next View      Shift+Tab Previous View
  v      Visualizer     p/Esc Playlist
  b      Browser        i    Track Info

  Other:
  /      Cmd            H    Shortcuts      q    Quit

  "#
                    }
                    ViewMode::TrackInfo => {
                        r#"Track Info Shortcuts

  TrackInfo:
  Space  Play/Pause     h/←  Previous       l/→  Next
  +/-    Volumes        M    Mute
  Ctrl+h/← Rewind 10 seconds
  Ctrl+l/→ Fast forward 10 seconds

  Views:
  Tab    Next View      Shift+Tab Previous View
  v      Visualizer      p    Playlists
  b      Browser        i/Esc Playlist

  Other:
  /      Cmd            H    Shortcuts      q    Quit

  "#
                    }
                    ViewMode::Visualizer => {
                        r#"Visualizer Shortcuts

  Visualizer:
  s      Style          f    FFT/Volume
  Space  Play/Pause     h/←  Previous       l/→  Next
  +/-    Volumes        M    Mute
  Ctrl+h/← Rewind 10 seconds
  Ctrl+l/→ Fast forward 10 seconds

  Views:
  Tab    Next View      Shift+Tab Previous View
  v/Esc  playlist       p    Playlists
  b      Browser        i    Track Info

  Other:
  /      Cmd            H    Shortcuts      q    Quit

  "#
                    }
                    ViewMode::Settings => {
                        r#"Settings Shortcuts

  Settings:
  Enter  Toggle         j/↓   Navigate       k/↑  Navigate
  Space  Play/Pause     h/←  Previous        l/→  Next
  +/-    Volumes        M    Mute

  Other:
  /      Cmd            Esc  Close           H    Shortcuts
  q      Quit
  "#
                    }
                    ViewMode::Help => views::help::SHORTCUTS,
                };
                let lines = shortcuts.lines().count() as u16 + 2; // +2 for borders
                self.popup_state = Some(popup::PopupState::new_confirm_tall(
                    format!("Shortcuts"),
                    shortcuts.to_string(),
                    popup::PendingAction::None,
                    lines,
                ));
            }
            KeyCode::Esc => {
                if self.view_mode == ViewMode::Help
                    || self.view_mode == ViewMode::TrackInfo
                    || self.view_mode == ViewMode::Visualizer
                    || self.view_mode == ViewMode::Playlists
                {
                    self.view_mode = ViewMode::Playlist;
                    return;
                }
                if self.edit_mode {
                    self.edit_mode = false;
                    self.set_status("Edit mode off");
                    return;
                }
            }
            _ => {}
        }

        // View-specific keys
        match self.view_mode {
            ViewMode::Playlist => views::playlist::handle(self, code, modifiers),
            ViewMode::Browser => views::browser::handle(self, code),
            ViewMode::Playlists => views::playlists_view::handle(self, code, modifiers),
            ViewMode::Help => views::help::handle(self, code),
            ViewMode::TrackInfo => views::track_info::handle(self, code, modifiers),
            ViewMode::Visualizer => views::visualizer::handle(self, code, modifiers),
            ViewMode::Settings => views::settings_view::handle(self, code, modifiers),
        }
    }

    /// Shared playback controls: volume, mute, seek, prev/next.
    /// Returns true if the key was consumed.
    fn handle_playback_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        match code {
            KeyCode::Char(' ') => {
                self.send_command(AppCommand::TogglePlayback);
                true
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.volume = (self.volume + 0.05).min(1.5);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
                true
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.volume = (self.volume - 0.05).max(0.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
                true
            }
            KeyCode::Char('m') => {
                if self.volume > 0.0 {
                    self.volume = 0.0;
                    self.set_status("Muted");
                } else {
                    self.volume = 1.0;
                    self.set_status("Volume: 100%");
                }
                self.send_command(AppCommand::SetVolume { level: self.volume });
                true
            }
            KeyCode::Char('l') if modifiers.contains(KeyModifiers::CONTROL) => {
                let pos = self.player.position();
                let new_pos = pos + Duration::from_secs(10);
                if let Some(duration) = self.player.duration() {
                    if new_pos < duration {
                        self.send_command(AppCommand::Seek {
                            position_secs: new_pos.as_secs_f64(),
                        });
                    }
                }
                true
            }
            KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
                let pos = self.player.position();
                let new_pos = pos.saturating_sub(Duration::from_secs(10));
                self.send_command(AppCommand::Seek {
                    position_secs: new_pos.as_secs_f64(),
                });
                true
            }
            KeyCode::Right if modifiers.contains(KeyModifiers::CONTROL) => {
                let pos = self.player.position();
                let new_pos = pos + Duration::from_secs(10);
                if let Some(duration) = self.player.duration() {
                    if new_pos < duration {
                        self.send_command(AppCommand::Seek {
                            position_secs: new_pos.as_secs_f64(),
                        });
                    }
                }
                true
            }
            KeyCode::Left if modifiers.contains(KeyModifiers::CONTROL) => {
                let pos = self.player.position();
                let new_pos = pos.saturating_sub(Duration::from_secs(10));
                self.send_command(AppCommand::Seek {
                    position_secs: new_pos.as_secs_f64(),
                });
                true
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.play_previous();
                true
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.play_next();
                true
            }
            _ => false,
        }
    }

    /// Visible rows available in the browser list widget (~ terminal rows minus header/nowplaying/status).
    pub fn browser_visible_rows() -> usize {
        let h = crossterm::terminal::size().unwrap_or((80, 24)).1;
        (h.saturating_sub(11) as usize).max(1)
    }

    fn handle_command_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Enter => {
                let input = self.input_buffer.content().to_string();
                self.execute_command(&input);
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.command_ghost = None;
                return;
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.command_ghost = None;
                return;
            }
            KeyCode::Backspace => {
                if self.input_buffer.is_empty() {
                    self.input_mode = InputMode::Normal;
                    self.command_ghost = None;
                    return;
                } else {
                    self.input_buffer.backspace();
                }
            }
            KeyCode::Delete => {
                self.input_buffer.delete();
            }
            KeyCode::Left => {
                self.input_buffer.move_left();
            }
            KeyCode::Right => {
                if self.input_buffer.cursor_at_end() {
                    if let Some(ref ghost) = self.command_ghost.clone() {
                        let chunk = get_next_word_chunk(ghost);
                        if !chunk.is_empty() {
                            self.input_buffer.insert_str(&chunk);
                        }
                    } else {
                        self.input_buffer.move_right();
                    }
                } else {
                    self.input_buffer.move_right();
                }
            }
            KeyCode::Home => {
                self.input_buffer.move_home();
            }
            KeyCode::End => {
                self.input_buffer.move_end();
            }
            KeyCode::Char(c) => {
                self.input_buffer.insert(c);
            }
            _ => {}
        }
        self.update_command_ghost();
    }

    /// Updates the command ghost text based on current input.
    fn update_command_ghost(&mut self) {
        self.command_ghost = get_suggestion(self.input_buffer.content());
    }

    fn handle_search_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Enter | KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                if code == KeyCode::Esc {
                    self.browser.clear_filter();
                }
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.backspace();
                self.browser
                    .set_filter(self.input_buffer.content().to_string());
            }
            KeyCode::Char(c) => {
                self.input_buffer.insert(c);
                self.browser
                    .set_filter(self.input_buffer.content().to_string());
            }
            _ => {}
        }
    }

    fn execute_command(&mut self, input: &str) {
        match Command::parse(input) {
            Ok(cmd) => {
                if let Err(e) = self.run_command(cmd) {
                    self.set_status(format!("Error: {}", e));
                }
            }
            Err(e) => {
                self.set_status(format!("{}", e));
            }
        }
    }

    fn run_command(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::Add { path } => {
                self.send_command(AppCommand::AddPath {
                    path: path.to_string_lossy().to_string(),
                });
                self.set_status("Adding to playlist...");
            }
            Command::Remove => {
                self.delete_selected_track();
            }
            Command::Clear => {
                self.send_command(AppCommand::ClearPlaylist);
                self.set_status("Playlist cleared");
            }
            Command::Dedup => {
                self.send_command(AppCommand::Dedup);
                self.set_status("Deduplicating...");
            }
            Command::Shuffle => {
                self.send_command(AppCommand::ToggleShuffle);
                self.set_status("Shuffle toggled");
            }
            Command::Repeat { mode } => {
                match mode {
                    Some(RepeatModeArg::Off) => self.send_command(AppCommand::SetRepeat {
                        mode: horikawa_protocol::RepeatModeValue::Off,
                    }),
                    Some(RepeatModeArg::One) => self.send_command(AppCommand::SetRepeat {
                        mode: horikawa_protocol::RepeatModeValue::One,
                    }),
                    Some(RepeatModeArg::All) => self.send_command(AppCommand::SetRepeat {
                        mode: horikawa_protocol::RepeatModeValue::All,
                    }),
                    None => self.send_command(AppCommand::CycleRepeat),
                };
                self.set_status("Repeat mode changed");
            }
            Command::Play => {
                self.play_selected();
            }
            Command::Pause => {
                self.send_command(AppCommand::Pause);
                self.set_status("Paused");
            }
            Command::Stop => {
                self.send_command(AppCommand::Stop);
                self.set_status("Stopped");
            }
            Command::Next => {
                self.play_next();
            }
            Command::Prev => {
                self.play_previous();
            }
            Command::Goto { path } => {
                self.browser.navigate_to(&path)?;
                self.view_mode = ViewMode::Browser;
            }
            Command::Home => {
                if let Some(home) = dirs::home_dir() {
                    self.browser.navigate_to(&home)?;
                    self.view_mode = ViewMode::Browser;
                }
            }
            Command::Search { term } => {
                self.browser.set_filter(term);
                self.view_mode = ViewMode::Browser;
            }
            Command::Help => {
                self.view_mode = ViewMode::Help;
            }
            Command::Quit => {
                self.should_quit = true;
            }
            Command::Save { name } => {
                self.send_command(AppCommand::SavePlaylist { name });
                self.set_status("Saving playlist...");
            }
            Command::Load { name } => {
                self.last_loaded = Some(PlaylistEntry::M3u(name.clone()));
                self.send_command(AppCommand::LoadPlaylist { name });
                self.set_status("Loading playlist...");
            }
            Command::ListPlaylists => {
                self.send_command(AppCommand::ListPlaylists);
            }
            Command::DeletePlaylist { name } => {
                self.send_command(AppCommand::DeletePlaylist { name });
            }
            Command::DirPl { sub } => match sub {
                DirPlCmd::Save { name, directory } => {
                    let dir = directory.unwrap_or_else(|| self.browser.current_dir().to_path_buf());
                    self.send_command(AppCommand::SaveDirPlaylist {
                        name,
                        directory: dir.to_string_lossy().to_string(),
                    });
                    self.set_status("Saving directory playlist...");
                }
                DirPlCmd::Load { name } => {
                    self.last_loaded = Some(PlaylistEntry::DirPl(name.clone()));
                    self.send_command(AppCommand::LoadDirPlaylist { name });
                    self.set_status("Loading directory playlist...");
                }
                DirPlCmd::List => {
                    self.send_command(AppCommand::ListDirPlaylists);
                }
                DirPlCmd::Delete { name } => {
                    self.send_command(AppCommand::DeleteDirPlaylist { name });
                }
            },
            Command::Seek { position } => {
                self.send_command(AppCommand::Seek {
                    position_secs: position.as_secs_f64(),
                });
                self.set_status(format!(
                    "Seeking to {}:{:02}",
                    position.as_secs() / 60,
                    position.as_secs() % 60
                ));
            }
            Command::Vis => {
                self.visualizer_style = self.visualizer_style.next();
                self.set_status(format!("Visualizer: {}", self.visualizer_style.name()));
            }
            Command::Volume { level } => {
                // Volume command handled below
                if let Some(level) = level {
                    self.volume = (level as f32 / 100.0).clamp(0.0, 1.5);
                    self.send_command(AppCommand::SetVolume { level: self.volume });
                    self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
                } else {
                    self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
                }
            }
            Command::Reload => {
                self.reload_last_playlist();
            }
        }
        Ok(())
    }

    fn playlist_select_next(&mut self) {
        let playlist = self.player.playlist();
        let playlist = playlist.read().unwrap();
        let len = playlist.len();

        if len == 0 {
            return;
        }

        let i = match self.playlist_state.selected() {
            Some(i) => {
                if i >= len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.playlist_state.select(Some(i));
    }

    fn playlist_select_previous(&mut self) {
        let playlist = self.player.playlist();
        let playlist = playlist.read().unwrap();
        let len = playlist.len();

        if len == 0 {
            return;
        }

        let i = match self.playlist_state.selected() {
            Some(i) => {
                if i == 0 {
                    len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.playlist_state.select(Some(i));
    }

    fn play_selected(&mut self) {
        if let Some(idx) = self.playlist_state.selected() {
            self.send_command(AppCommand::PlayTrack { index: idx });
        }
    }

    fn play_next(&mut self) {
        self.send_command(AppCommand::Next);
    }

    fn play_previous(&mut self) {
        self.send_command(AppCommand::Previous);
    }

    fn move_track_down(&mut self) {
        if let Some(idx) = self.playlist_state.selected() {
            let len = self.player.playlist().read().unwrap().len();
            if idx < len.saturating_sub(1) {
                self.send_command(AppCommand::MoveTrack {
                    from: idx,
                    to: idx + 1,
                });
                self.playlist_state.select(Some(idx + 1));
            }
        }
    }

    fn move_track_up(&mut self) {
        if let Some(idx) = self.playlist_state.selected() {
            if idx > 0 {
                self.send_command(AppCommand::MoveTrack {
                    from: idx,
                    to: idx - 1,
                });
                self.playlist_state.select(Some(idx - 1));
            }
        }
    }

    fn delete_selected_track(&mut self) {
        if let Some(idx) = self.playlist_state.selected() {
            let len = self.player.playlist().read().unwrap().len();
            self.send_command(AppCommand::RemoveTrack { index: idx });
            // Adjust selection for the removed item
            let new_len = len.saturating_sub(1);
            if new_len == 0 {
                self.playlist_state.select(None);
            } else if idx >= new_len {
                self.playlist_state.select(Some(new_len - 1));
            }
            self.set_status("Track removed");
        }
    }

    /// Refreshes the list of saved playlists from the playlist directory.
    fn refresh_playlist_lists(&mut self) {
        self.playlist_entries.clear();
        self.playlist_list_selected = 0;

        let dir = match horikawa_core::Playlist::playlist_dir() {
            Some(d) if d.exists() => d,
            _ => return,
        };

        let mut m3u_names = Vec::new();
        let mut dirpl_names = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

                // Skip session auto-save
                if stem == "_last" {
                    continue;
                }

                match extension {
                    "m3u" => m3u_names.push(stem.to_string()),
                    "horikawa" => dirpl_names.push(stem.to_string()),
                    _ => {}
                }
            }
        }

        m3u_names.sort();
        dirpl_names.sort();

        self.playlist_entries
            .extend(m3u_names.into_iter().map(PlaylistEntry::M3u));
        self.playlist_entries
            .extend(dirpl_names.into_iter().map(PlaylistEntry::DirPl));
    }

    /// Reloads the last loaded playlist, if any.
    fn reload_last_playlist(&mut self) {
        if let Some(ref entry) = self.last_loaded {
            match entry {
                PlaylistEntry::M3u(name) => {
                    self.send_command(AppCommand::LoadPlaylist { name: name.clone() });
                    self.set_status(format!("Reloaded playlist: {}", name));
                }
                PlaylistEntry::DirPl(name) => {
                    self.send_command(AppCommand::LoadDirPlaylist { name: name.clone() });
                    self.set_status(format!("Reloaded directory playlist: {}", name));
                }
            }
        } else {
            self.set_status("Nothing to reload");
        }
    }

    /// Executes the action from a confirmed popup.
    fn execute_popup_action(&mut self, action: popup::PendingAction, name: Option<String>) {
        match action {
            popup::PendingAction::None => {}

            popup::PendingAction::SaveM3uFromBrowser(dir) => {
                let playlist_name = name.unwrap_or_else(|| "untitled".to_string());

                // Scan directory for audio files
                let mut scanner = LibraryScanner::new();
                scanner.add_root(dir.clone());

                match scanner.scan() {
                    Ok(tracks) => {
                        let paths: Vec<PathBuf> = tracks.into_iter().map(|t| t.path).collect();
                        let count = paths.len();

                        if count == 0 {
                            self.set_status("No audio files found in directory");
                            return;
                        }

                        // Add to playlist
                        {
                            let playlist_arc = self.player.playlist();
                            let mut playlist = playlist_arc.write().unwrap();
                            playlist.clear();
                            playlist.add_many(paths);
                        }

                        // Save as M3U
                        if let Some(dir) = horikawa_core::Playlist::ensure_playlist_dir() {
                            let path = dir.join(format!("{}.m3u", playlist_name));
                            let playlist_arc = self.player.playlist();
                            let playlist = playlist_arc.read().unwrap();
                            if let Err(e) = playlist.save(&path) {
                                self.set_status(format!("Save error: {}", e));
                            } else {
                                self.set_status(format!(
                                    "Saved M3U playlist: {} ({} tracks)",
                                    playlist_name, count
                                ));
                            }
                        }

                        self.last_loaded = Some(PlaylistEntry::M3u(playlist_name));
                    }
                    Err(e) => {
                        self.set_status(format!("Scan error: {}", e));
                    }
                }
            }

            popup::PendingAction::SaveM3uFromPlaylist => {
                let playlist_name = name.unwrap_or_else(|| "untitled".to_string());

                let count = {
                    let playlist_arc = self.player.playlist();
                    let playlist = playlist_arc.read().unwrap();
                    let count = playlist.len();
                    if count == 0 {
                        self.set_status("Playlist is empty");
                        return;
                    }
                    // Save as M3U
                    if let Some(dir) = horikawa_core::Playlist::ensure_playlist_dir() {
                        let path = dir.join(format!("{}.m3u", playlist_name));
                        if let Err(e) = playlist.save(&path) {
                            self.set_status(format!("Save error: {}", e));
                            return;
                        }
                    }
                    count
                };

                self.last_loaded = Some(PlaylistEntry::M3u(playlist_name.clone()));
                self.set_status(format!(
                    "Saved M3U playlist: {} ({} tracks)",
                    playlist_name, count
                ));
            }

            popup::PendingAction::SaveDirPlFromBrowser(dir) => {
                let playlist_name = name.unwrap_or_else(|| "untitled".to_string());
                self.send_command(AppCommand::SaveDirPlaylist {
                    name: playlist_name.clone(),
                    directory: dir.to_string_lossy().to_string(),
                });
                self.last_loaded = Some(PlaylistEntry::DirPl(playlist_name.clone()));
                self.set_status(format!("Saved directory playlist: {}", playlist_name));
            }

            popup::PendingAction::DeletePlaylist(entry) => {
                match &entry {
                    PlaylistEntry::M3u(name) => {
                        self.send_command(AppCommand::DeletePlaylist { name: name.clone() });
                        self.set_status(format!("Deleted playlist: {}", name));
                    }
                    PlaylistEntry::DirPl(name) => {
                        self.send_command(AppCommand::DeleteDirPlaylist { name: name.clone() });
                        self.set_status(format!("Deleted directory playlist: {}", name));
                    }
                }
                self.needs_playlist_refresh = true;
            }

            popup::PendingAction::RenamePlaylist(entry) => {
                let new_name = name.unwrap_or_default();
                if new_name.is_empty() {
                    self.set_status("Rename cancelled: empty name");
                    return;
                }
                if let Some(dir) = horikawa_core::Playlist::playlist_dir() {
                    let (old_ext, old_name) = match &entry {
                        PlaylistEntry::M3u(n) => ("m3u", n.as_str()),
                        PlaylistEntry::DirPl(n) => ("horikawa", n.as_str()),
                    };
                    let old_path = dir.join(format!("{}.{}", old_name, old_ext));
                    let new_path = dir.join(format!("{}.{}", new_name, old_ext));
                    if old_path.exists() && !new_path.exists() {
                        match std::fs::rename(&old_path, &new_path) {
                            Ok(()) => {
                                self.set_status(format!("Renamed: {} → {}", old_name, new_name));
                                self.refresh_playlist_lists();
                            }
                            Err(e) => {
                                self.set_status(format!("Rename error: {}", e));
                            }
                        }
                    } else if new_path.exists() {
                        self.set_status(format!("'{}' already exists", new_name));
                    }
                }
            }
        }
    }
}
pub fn draw_ui(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Create layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Header
            Constraint::Min(0),    // Main content
            Constraint::Length(5), // Now playing
            Constraint::Length(2), // Status bar
        ])
        .split(area);

    // Header with view indicator
    let view_indicator = match app.view_mode {
        ViewMode::Playlist => {
            if app.edit_mode {
                "PLAYLIST [EDIT]"
            } else {
                "PLAYLIST"
            }
        }
        ViewMode::Browser => "BROWSER",
        ViewMode::Help => "HELP",
        ViewMode::TrackInfo => "TRACK INFO",
        ViewMode::Visualizer => "VISUALIZER",
        ViewMode::Settings => "SETTINGS",
        ViewMode::Playlists => "PLAYLISTS",
    };

    let header = Paragraph::new(format!("  HORIKAWA - {}", view_indicator))
        .style(Style::default().fg(Color::Cyan).bold())
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, chunks[0]);

    // Main content area based on view mode
    match app.view_mode {
        ViewMode::Playlist => views::playlist::draw(frame, app, chunks[1]),
        ViewMode::Browser => views::browser::draw(frame, app, chunks[1]),
        ViewMode::Help => views::help::draw(frame, app, chunks[1]),
        ViewMode::TrackInfo => views::track_info::draw(frame, app, chunks[1]),
        ViewMode::Visualizer => views::visualizer::draw(frame, app, chunks[1]),
        ViewMode::Settings => views::settings_view::draw(frame, app, chunks[1]),
        ViewMode::Playlists => views::playlists_view::draw(frame, app, chunks[1]),
    }

    // Now playing
    views::status::draw_now_playing(frame, app, chunks[2]);

    // Status bar
    views::status::draw_status_bar(frame, app, chunks[3]);

    // Popup overlay (rendered last, on top of everything)
    if let Some(ref popup) = app.popup_state {
        let height = if popup.preferred_height > 0 {
            popup.preferred_height
        } else {
            5
        };
        let popup_area = popup::centered_rect(50, height, frame.area());
        popup::draw_popup(frame, popup, popup_area);
    }
}
