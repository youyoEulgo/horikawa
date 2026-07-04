//! Horikawa CLI - Terminal UI music player
#![allow(unexpected_cfgs)]

mod browser;
mod cli;
mod discord;
mod input;
mod integrations;
mod media_controls;
mod popup;
mod settings;
mod view;

use std::io;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::Alignment,
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use browser::FileBrowser;
use cli::Args;
use input::{InputBuffer, InputMode};
use view::{ViewMode, VisualizerStyle};

use horikawa_core::{
    command::{self, get_next_word_chunk, get_suggestion, DirPlCmd, RepeatModeArg},
    library::LibraryScanner,
    player::PlaybackState,
    Command, Player, RepeatMode,
};
use horikawa_ctl::{CommandProcessor, CommandSender, ControlChannel, ProcessorSettings};
use horikawa_protocol::{AppCommand, StateUpdate};

/// Entry type for the Playlists view.
#[derive(Clone)]
enum PlaylistEntry {
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
struct App {
    player: Arc<Player>,
    command_sender: CommandSender,
    state_rx: tokio::sync::broadcast::Receiver<StateUpdate>,
    should_quit: bool,

    // View state
    view_mode: ViewMode,
    playlist_state: ListState,
    browser: FileBrowser,

    // Input state
    input_mode: InputMode,
    input_buffer: InputBuffer,
    command_ghost: Option<String>,

    // Edit mode
    edit_mode: bool,

    // Visualizer style
    visualizer_style: VisualizerStyle,

    // Visualizer data source: true = FFT spectrum, false = RMS volume
    spectrum_mode: bool,

    // Volume (0.0 to 1.0)
    volume: f32,

    // Flag to scroll to playing track without changing selection
    scroll_to_playing: bool,

    // Mouse click tracking for double-click detection
    last_click_time: Option<std::time::Instant>,
    last_click_row: Option<u16>,

    // Store playlist area for mouse hit detection
    playlist_area: Option<Rect>,

    // Help view scroll offset
    help_scroll: u16,

    // Track change detection (for auto-scroll on advance)
    last_track: Option<PathBuf>,

    // Status message (shown in status bar)
    status_message: Option<String>,
    status_clear_at: Option<std::time::Instant>,

    // Settings
    settings: settings::Settings,
    settings_selected: usize,

    // Playlists view state
    playlist_entries: Vec<PlaylistEntry>,
    playlist_list_selected: usize,

    // Last loaded playlist (for quick reload)
    last_loaded: Option<PlaylistEntry>,

    // Popup state (input or confirm dialog)
    popup_state: Option<popup::PopupState>,

    // macOS media controls (must run on main thread)
    #[cfg(target_os = "macos")]
    media_controls: Option<crate::media_controls::MediaControlsHandler>,
    #[cfg(target_os = "macos")]
    smtc_rx: Option<mpsc::Receiver<crate::media_controls::MediaControlCommand>>,
    #[cfg(target_os = "macos")]
    last_smtc_state: Option<PlaybackState>,
    #[cfg(target_os = "macos")]
    last_smtc_track: Option<PathBuf>,
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
    fn new(
        player: Arc<Player>,
        command_sender: CommandSender,
        state_rx: tokio::sync::broadcast::Receiver<StateUpdate>,
        args: &Args,
    ) -> Result<Self> {
        // Determine starting directory for browser
        let start_path = args
            .path
            .clone()
            .or_else(|| dirs::home_dir())
            .unwrap_or_else(|| PathBuf::from("."));

        let browser = FileBrowser::new(start_path)?;

        // Determine starting view
        let view_mode = if args.browse {
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
    fn tick(&mut self) {
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
    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
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
    fn handle_mouse(&mut self, column: u16, row: u16, kind: MouseEventKind) {
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
                    ViewMode::Help => {
                        r#"Help Shortcuts

  Help:
  j/↓    Scroll Down    k/↑  Scroll Up
  PgDn   Page Down      PgUp Page Up

  Other:
  /      Cmd            Esc/? Close          H    Shortcuts
  q      Quit
  "#
                    }
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
            ViewMode::Playlist => self.handle_playlist_key(code, modifiers),
            ViewMode::Browser => self.handle_browser_key(code),
            ViewMode::Playlists => self.handle_playlists_key(code),
            ViewMode::Help => self.handle_help_key(code),
            ViewMode::TrackInfo => self.handle_track_info_key(code, modifiers),
            ViewMode::Visualizer => self.handle_visualizer_key(code, modifiers),
            ViewMode::Settings => self.handle_settings_key(code),
        }
    }

    fn handle_playlist_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        if self.handle_view_jump(code) {
            return;
        }
        match code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Char(' ') => {
                // Toggle play/pause
                match self.player.state() {
                    PlaybackState::Playing | PlaybackState::Paused => {
                        self.send_command(AppCommand::TogglePlayback);
                    }
                    PlaybackState::Stopped => {
                        // Start playing selected track
                        self.play_selected();
                    }
                }
            }
            KeyCode::Char('s') if !self.edit_mode => {
                // Save current playlist as M3U (with name prompt)
                self.popup_state = Some(popup::PopupState::new_input(
                    "Save Playlist".to_string(),
                    String::new(),
                    popup::PendingAction::SaveM3uFromPlaylist,
                ));
            }
            KeyCode::Char('e') => {
                self.edit_mode = !self.edit_mode;
                if self.edit_mode {
                    // Persistent — stays until edit mode is turned off
                    self.status_message =
                        Some("Edit mode: Shift+J/K to move, d to delete, c to clear".into());
                    self.status_clear_at = None;
                } else {
                    self.status_message = None;
                    self.set_status("Edit mode off");
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.playlist_select_previous();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.playlist_select_next();
            }
            // Edit mode: Shift+J/K to move tracks
            KeyCode::Char('J') if self.edit_mode && modifiers.contains(KeyModifiers::SHIFT) => {
                self.move_track_down();
            }
            KeyCode::Char('K') if self.edit_mode && modifiers.contains(KeyModifiers::SHIFT) => {
                self.move_track_up();
            }
            KeyCode::Char('d') if self.edit_mode => {
                self.delete_selected_track();
            }
            KeyCode::Enter => {
                self.play_selected();
            }
            KeyCode::Char('l') if modifiers.contains(KeyModifiers::CONTROL) => {
                // Seek forward 10 seconds
                let pos = self.player.position();
                let new_pos = pos + Duration::from_secs(10);
                if let Some(duration) = self.player.duration() {
                    if new_pos < duration {
                        self.send_command(AppCommand::Seek {
                            position_secs: new_pos.as_secs_f64(),
                        });
                    }
                }
            }
            KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
                // Seek backward 10 seconds
                let pos = self.player.position();
                let new_pos = pos.saturating_sub(Duration::from_secs(10));
                self.send_command(AppCommand::Seek {
                    position_secs: new_pos.as_secs_f64(),
                });
            }
            KeyCode::Char('h') => {
                self.play_previous();
            }
            KeyCode::Char('l') => {
                self.play_next();
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
            }
            KeyCode::Left if modifiers.contains(KeyModifiers::CONTROL) => {
                let pos = self.player.position();
                let new_pos = pos.saturating_sub(Duration::from_secs(10));
                self.send_command(AppCommand::Seek {
                    position_secs: new_pos.as_secs_f64(),
                });
            }
            KeyCode::Right => {
                self.play_next();
            }
            KeyCode::Left => {
                self.play_previous();
            }
            KeyCode::Char('c') if self.edit_mode => {
                self.send_command(AppCommand::ClearPlaylist);
                self.set_status("Playlist cleared");
            }
            KeyCode::Char('R') => {
                self.reload_last_playlist();
            }
            KeyCode::Char('r') => {
                // Cycle repeat mode
                let playlist_arc = self.player.playlist();
                let playlist = playlist_arc.read().unwrap();
                let new_mode = match playlist.repeat() {
                    RepeatMode::Off => RepeatMode::One,
                    RepeatMode::One => RepeatMode::All,
                    RepeatMode::All => RepeatMode::Off,
                };
                drop(playlist);
                self.send_command(AppCommand::CycleRepeat);
                self.set_status(format!("Repeat: {:?}", new_mode));
            }
            KeyCode::Char('S') => {
                // Toggle shuffle
                let playlist_arc = self.player.playlist();
                let playlist = playlist_arc.read().unwrap();
                let new_shuffle = !playlist.shuffle();
                drop(playlist);
                self.send_command(AppCommand::ToggleShuffle);
                self.set_status(format!(
                    "Shuffle: {}",
                    if new_shuffle { "on" } else { "off" }
                ));
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                // Volume up
                self.volume = (self.volume + 0.05).min(1.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                // Volume down
                self.volume = (self.volume - 0.05).max(0.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
            }
            KeyCode::Char('m') => {
                // Mute/unmute toggle
                if self.volume > 0.0 {
                    self.volume = 0.0;
                    self.set_status("Muted");
                } else {
                    self.volume = 1.0;
                    self.set_status("Volume: 100%");
                }
                self.send_command(AppCommand::SetVolume { level: self.volume });
            }
            KeyCode::Char('i') => {
                // Show track info
                self.view_mode = ViewMode::TrackInfo;
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.playlist_state.select(Some(0));
            }
            KeyCode::End | KeyCode::Char('G') => {
                let playlist = self.player.playlist();
                let playlist = playlist.read().unwrap();
                if !playlist.is_empty() {
                    self.playlist_state.select(Some(playlist.len() - 1));
                }
            }
            _ => {}
        }
    }

    /// Visible rows available in the browser list widget (~ terminal rows minus header/nowplaying/status).
    fn browser_visible_rows() -> usize {
        let h = crossterm::terminal::size().unwrap_or((80, 24)).1;
        (h.saturating_sub(11) as usize).max(1)
    }

    fn handle_browser_key(&mut self, code: KeyCode) {
        let visible = Self::browser_visible_rows();
        if self.handle_view_jump(code) {
            return;
        }
        match code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc => {
                self.view_mode = ViewMode::Playlist;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.browser.select_previous(visible);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.browser.select_next(visible);
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                if let Ok(Some(file_path)) = self.browser.enter_selected() {
                    self.send_command(AppCommand::PlayPath {
                        path: file_path.to_string_lossy().to_string(),
                    });
                }
            }
            KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => {
                let _ = self.browser.go_up();
            }
            KeyCode::Char('a') => {
                if let Some(entry) = self.browser.selected_entry() {
                    let path = entry.path.clone();
                    let is_dir = entry.is_dir;
                    let is_audio = entry.is_audio;
                    if (is_dir && entry.name != "..") || is_audio {
                        self.send_command(AppCommand::AddPath {
                            path: path.to_string_lossy().to_string(),
                        });
                    }
                }
            }
            KeyCode::Char('s') => {
                // Save selected dir as M3U (popup to name)
                if let Some(entry) = self.browser.selected_entry() {
                    let (dir, default_name) = if entry.is_dir && entry.name != ".." {
                        (entry.path.clone(), entry.name.clone())
                    } else if entry.name == ".." {
                        self.set_status("Cannot save parent directory");
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
                    self.popup_state = Some(popup::PopupState::new_input(
                        "Save M3U Playlist".to_string(),
                        default_name,
                        popup::PendingAction::SaveM3uFromBrowser(dir),
                    ));
                }
            }
            KeyCode::Char('S') => {
                // Save selected dir as .horikawa (popup to name)
                if let Some(entry) = self.browser.selected_entry() {
                    let (dir, default_name) = if entry.is_dir && entry.name != ".." {
                        (entry.path.clone(), entry.name.clone())
                    } else if entry.name == ".." {
                        self.set_status("Cannot save parent directory");
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
                    self.popup_state = Some(popup::PopupState::new_input(
                        "Save Dir Playlist".to_string(),
                        default_name,
                        popup::PendingAction::SaveDirPlFromBrowser(dir),
                    ));
                }
            }
            KeyCode::Char('R') => {
                let _ = self.browser.refresh();
                self.set_status("Refreshed");
            }
            KeyCode::Char('.') => {
                let _ = self.browser.toggle_hidden();
                self.set_status("Toggled hidden files");
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.browser.select_first(visible);
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.browser.select_last(visible);
            }
            KeyCode::Char('~') => {
                if let Some(home) = dirs::home_dir() {
                    let _ = self.browser.navigate_to(&home);
                }
            }
            _ => {}
        }
    }

    fn handle_help_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('?') => {
                self.view_mode = ViewMode::Playlist;
                self.help_scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.help_scroll = self.help_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.help_scroll = self.help_scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                self.help_scroll = self.help_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.help_scroll = self.help_scroll.saturating_add(10);
            }
            KeyCode::Home => {
                self.help_scroll = 0;
            }
            _ => {}
        }
    }

    fn handle_track_info_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        if self.handle_view_jump(code) {
            return;
        }
        match code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc => {
                self.view_mode = ViewMode::Playlist;
            }
            // Playback controls
            KeyCode::Char(' ') => {
                self.send_command(AppCommand::TogglePlayback);
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
            }
            KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
                let pos = self.player.position();
                let new_pos = pos.saturating_sub(Duration::from_secs(10));
                self.send_command(AppCommand::Seek {
                    position_secs: new_pos.as_secs_f64(),
                });
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
            }
            KeyCode::Left if modifiers.contains(KeyModifiers::CONTROL) => {
                let pos = self.player.position();
                let new_pos = pos.saturating_sub(Duration::from_secs(10));
                self.send_command(AppCommand::Seek {
                    position_secs: new_pos.as_secs_f64(),
                });
            }
            KeyCode::Right | KeyCode::Char('l') => self.play_next(),
            KeyCode::Left | KeyCode::Char('h') => self.play_previous(),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.volume = (self.volume + 0.05).min(1.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.volume = (self.volume - 0.05).max(0.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
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
            }
            _ => {}
        }
    }

    fn handle_visualizer_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        if self.handle_view_jump(code) {
            return;
        }
        match code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc => {
                self.view_mode = ViewMode::Playlist;
            }
            KeyCode::Char('s') => {
                self.visualizer_style = self.visualizer_style.next();
                self.set_status(format!("Visualizer: {}", self.visualizer_style.name()));
            }
            KeyCode::Char('f') => {
                self.spectrum_mode = !self.spectrum_mode;
                self.set_status(if self.spectrum_mode {
                    "Spectrum mode: FFT"
                } else {
                    "Spectrum mode: Volume"
                });
            }
            // Playback controls
            KeyCode::Char(' ') => {
                self.send_command(AppCommand::TogglePlayback);
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
            }
            KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
                let pos = self.player.position();
                let new_pos = pos.saturating_sub(Duration::from_secs(10));
                self.send_command(AppCommand::Seek {
                    position_secs: new_pos.as_secs_f64(),
                });
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
            }
            KeyCode::Left if modifiers.contains(KeyModifiers::CONTROL) => {
                let pos = self.player.position();
                let new_pos = pos.saturating_sub(Duration::from_secs(10));
                self.send_command(AppCommand::Seek {
                    position_secs: new_pos.as_secs_f64(),
                });
            }
            KeyCode::Right | KeyCode::Char('l') => self.play_next(),
            KeyCode::Left | KeyCode::Char('h') => self.play_previous(),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.volume = (self.volume + 0.05).min(1.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.volume = (self.volume - 0.05).max(0.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
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
            }
            _ => {}
        }
    }

    fn handle_settings_key(&mut self, code: KeyCode) {
        // Number of settings items
        const SETTINGS_COUNT: usize = 2;

        match code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc => {
                self.view_mode = ViewMode::Playlist;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.settings_selected > 0 {
                    self.settings_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.settings_selected < SETTINGS_COUNT - 1 {
                    self.settings_selected += 1;
                }
            }
            KeyCode::Enter => {
                // Toggle the selected setting (sends command to processor;
                // integrations worker handles the actual enable/disable)
                match self.settings_selected {
                    0 => {
                        self.send_command(AppCommand::ToggleSetting {
                            key: "discord_enabled".to_string(),
                        });
                        self.settings.discord_enabled = !self.settings.discord_enabled;
                    }
                    1 => {
                        self.send_command(AppCommand::ToggleSetting {
                            key: "smtc_enabled".to_string(),
                        });
                        self.settings.smtc_enabled = !self.settings.smtc_enabled;
                    }
                    _ => {}
                }
            }
            // Playback controls
            KeyCode::Char(' ') => {
                self.send_command(AppCommand::TogglePlayback);
            }
            KeyCode::Left | KeyCode::Char('h') => self.play_previous(),
            KeyCode::Right | KeyCode::Char('l') => self.play_next(),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.volume = (self.volume + 0.05).min(1.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.volume = (self.volume - 0.05).max(0.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
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
            }
            _ => {}
        }
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
                    self.volume = (level as f32 / 100.0).clamp(0.0, 1.0);
                    self.send_command(AppCommand::SetVolume { level: self.volume });
                    self.set_status(format!("Volume: {}%", level.min(100)));
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

    /// Saves the current session state for restoration on next startup.
    fn save_session(&self) {
        let playlist_arc = self.player.playlist();
        let playlist = playlist_arc.read().unwrap();

        // Only save if there's something in the playlist
        if playlist.is_empty() {
            return;
        }

        // Save the playlist as "_last"
        if let Some(dir) = horikawa_core::Playlist::ensure_playlist_dir() {
            let path = dir.join("_last.m3u");
            if let Err(e) = playlist.save(&path) {
                tracing::warn!("Failed to save session playlist: {}", e);
            }
        }

        // Encode last_loaded into playlist_name so R key works after restart.
        // Format: "m3u:<name>", "dirpl:<name>", or "_last" if nothing loaded.
        let playlist_name = match self.last_loaded {
            Some(PlaylistEntry::M3u(ref name)) => format!("m3u:{}", name),
            Some(PlaylistEntry::DirPl(ref name)) => format!("dirpl:{}", name),
            None => "_last".to_string(),
        };

        let state = horikawa_core::playlist::SessionState {
            playlist_name,
            track_index: playlist.current_index().or(self.playlist_state.selected()),
            shuffle: playlist.shuffle(),
            repeat: playlist.repeat(),
            volume: self.volume,
        };

        if let Err(e) = horikawa_core::Playlist::save_session(&state) {
            tracing::warn!("Failed to save session state: {}", e);
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
                self.refresh_playlist_lists();
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

    /// Handles keyboard input for the Playlists view.
    fn handle_playlists_key(&mut self, code: KeyCode) {
        if self.handle_view_jump(code) {
            return;
        }
        match code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc => {
                self.view_mode = ViewMode::Playlist;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.playlist_entries.is_empty() {
                    self.playlist_list_selected = if self.playlist_list_selected == 0 {
                        self.playlist_entries.len() - 1
                    } else {
                        self.playlist_list_selected - 1
                    };
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.playlist_entries.is_empty() {
                    self.playlist_list_selected =
                        (self.playlist_list_selected + 1) % self.playlist_entries.len();
                }
            }
            KeyCode::Enter => {
                if let Some(entry) = self.playlist_entries.get(self.playlist_list_selected) {
                    self.last_loaded = Some(entry.clone());
                    match entry {
                        PlaylistEntry::M3u(name) => {
                            self.send_command(AppCommand::LoadPlaylist { name: name.clone() });
                            self.set_status(format!("Loading playlist: {}", name));
                        }
                        PlaylistEntry::DirPl(name) => {
                            self.send_command(AppCommand::LoadDirPlaylist { name: name.clone() });
                            self.set_status(format!("Loading directory playlist: {}", name));
                        }
                    }
                    self.view_mode = ViewMode::Playlist;
                }
            }
            KeyCode::Char('d') => {
                if let Some(entry) = self.playlist_entries.get(self.playlist_list_selected) {
                    self.popup_state = Some(popup::PopupState::new_confirm(
                        "Delete Playlist".to_string(),
                        format!("Delete '{}'?", entry.name()),
                        popup::PendingAction::DeletePlaylist(entry.clone()),
                    ));
                }
            }
            KeyCode::Char('r') => {
                if let Some(entry) = self.playlist_entries.get(self.playlist_list_selected) {
                    self.popup_state = Some(popup::PopupState::new_input(
                        "Rename Playlist".to_string(),
                        entry.name().to_string(),
                        popup::PendingAction::RenamePlaylist(entry.clone()),
                    ));
                }
            }
            // Playback controls
            KeyCode::Char(' ') => {
                self.send_command(AppCommand::TogglePlayback);
            }
            KeyCode::Left | KeyCode::Char('h') => self.play_previous(),
            KeyCode::Right | KeyCode::Char('l') => self.play_next(),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.volume = (self.volume + 0.05).min(1.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.volume = (self.volume - 0.05).max(0.0);
                self.send_command(AppCommand::SetVolume { level: self.volume });
                self.set_status(format!("Volume: {}%", (self.volume * 100.0) as i32));
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
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Create the shared player
    let player = Arc::new(Player::new()?);

    // Remember last_loaded for session restore so R key works after restart
    let mut session_last_loaded: Option<PlaylistEntry> = None;

    // Load initial playlist from CLI files or last session
    if !args.files.is_empty() {
        let playlist_arc = player.playlist();
        let mut playlist = playlist_arc.write().unwrap();
        for file in &args.files {
            if file.is_dir() {
                let mut scanner = LibraryScanner::new();
                scanner.add_root(file.clone());
                if let Ok(tracks) = scanner.scan() {
                    playlist.add_many(tracks.into_iter().map(|t| t.path));
                }
            } else {
                playlist.add(file.clone());
            }
        }
    } else {
        // Try to load last session
        if let Some(session) = horikawa_core::Playlist::load_session() {
            // Parse last_loaded from session so R key can reload after restart.
            // Format: "m3u:<name>", "dirpl:<name>", or just "<name>" (legacy).
            session_last_loaded = if let Some(name) = session.playlist_name.strip_prefix("m3u:") {
                Some(PlaylistEntry::M3u(name.to_string()))
            } else if let Some(name) = session.playlist_name.strip_prefix("dirpl:") {
                Some(PlaylistEntry::DirPl(name.to_string()))
            } else {
                None
            };

            if let Some(dir) = horikawa_core::Playlist::playlist_dir() {
                let path = dir.join("_last.m3u");
                if let Ok(loaded) = horikawa_core::Playlist::load(&path) {
                    let playlist_arc = player.playlist();
                    let mut playlist = playlist_arc.write().unwrap();
                    *playlist = loaded;
                    playlist.set_shuffle(session.shuffle);
                    playlist.set_repeat(session.repeat);
                    if let Some(idx) = session.track_index {
                        playlist.jump_to(idx);
                    }
                    player.set_volume(session.volume);
                    tracing::info!(
                        "Restored session: {}, track {}, shuffle={}, repeat={:?}, volume={}",
                        session.playlist_name,
                        session.track_index.unwrap_or(0),
                        session.shuffle,
                        session.repeat,
                        session.volume
                    );
                }
            }
        }
    }

    // Determine starting directory for the processor's browser
    let start_path = args
        .path
        .clone()
        .or_else(|| dirs::home_dir())
        .unwrap_or_else(|| PathBuf::from("."));

    // Create control channel
    let mut channel = ControlChannel::new();
    let command_sender = channel.sender();
    let command_rx = channel
        .take_command_rx()
        .expect("Command receiver already taken");
    let broadcast_tx = channel.broadcast_tx();

    // Load processor settings and create command processor
    let proc_settings = ProcessorSettings::load();
    let integrations_discord = proc_settings.discord_enabled;
    #[cfg(not(target_os = "macos"))]
    let integrations_smtc = proc_settings.smtc_enabled;
    let processor_player = Arc::clone(&player);
    let browse = args.browse;
    let mut processor = CommandProcessor::new(
        processor_player,
        proc_settings,
        start_path,
        browse,
        command_rx,
        broadcast_tx,
    );

    // Spawn command processor on a background thread with its own tokio runtime
    std::thread::Builder::new()
        .name("horikawa-processor".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for command processor");
            rt.block_on(processor.run());
        })
        .expect("Failed to spawn command processor thread");

    // Spawn integrations worker (Discord Rich Presence + SMTC)
    // Runs on its own thread so these work in both TUI and daemon modes
    {
        let integrations_player = Arc::clone(&player);
        let integrations_sender = channel.sender();
        let integrations_rx = channel.subscribe();
        let integrations_settings = ProcessorSettings {
            discord_enabled: integrations_discord,
            // On macOS, SMTC is handled on the main thread (not in this worker)
            #[cfg(target_os = "macos")]
            smtc_enabled: false,
            #[cfg(not(target_os = "macos"))]
            smtc_enabled: integrations_smtc,
            ..ProcessorSettings::default()
        };
        std::thread::Builder::new()
            .name("horikawa-integrations".to_string())
            .spawn(move || {
                integrations::run_integrations(
                    integrations_player,
                    integrations_sender,
                    integrations_rx,
                    &integrations_settings,
                );
            })
            .expect("Failed to spawn integrations thread");
    }

    // Branch: daemon mode (headless) vs TUI mode
    if args.daemon {
        tracing::info!("Running in daemon mode (headless). Press Ctrl+C to stop.");

        // Block until the process is killed
        // Future: this is where IPC socket listening for CLI oneshots will go
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    // --- TUI mode ---

    // Setup terminal
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    io::stdout().execute(crossterm::event::EnableMouseCapture)?;

    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    // Create TUI app (reads initial state from the shared player)
    let state_rx = channel.subscribe();
    let mut app = App::new(player, command_sender, state_rx, &args)?;
    app.last_loaded = session_last_loaded;

    // macOS: initialize Now Playing media controls on the main thread.
    // This must happen here (not in a background thread) because
    // MPNowPlayingInfoCenter and MPRemoteCommandCenter are main-thread-only.
    #[cfg(target_os = "macos")]
    {
        let (smtc_tx, smtc_rx) = mpsc::channel();
        app.media_controls = crate::media_controls::MediaControlsHandler::new(smtc_tx);
        app.smtc_rx = Some(smtc_rx);
    }

    // Main loop
    loop {
        // Update state
        app.tick();

        // Draw UI
        terminal.draw(|frame| draw_ui(frame, &mut app))?;

        // Handle events with timeout
        if event::poll(Duration::from_millis(33))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.handle_key(key.code, key.modifiers);
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse.column, mouse.row, mouse.kind);
                }
                _ => {}
            }
        }

        // macOS: pump CFRunLoop so MPNowPlayingInfoCenter XPC
        // messages get delivered (1ms timeout, non-blocking).
        crate::media_controls::pump_run_loop();

        if app.should_quit {
            // Save session before quitting
            app.save_session();
            break;
        }
    }

    // Cleanup
    io::stdout().execute(crossterm::event::DisableMouseCapture)?;
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}

/// Draws the main UI.
fn draw_ui(frame: &mut Frame, app: &mut App) {
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
        ViewMode::Playlist => draw_playlist(frame, app, chunks[1]),
        ViewMode::Browser => draw_browser(frame, app, chunks[1]),
        ViewMode::Help => draw_help(frame, app, chunks[1]),
        ViewMode::TrackInfo => draw_track_info(frame, app, chunks[1]),
        ViewMode::Visualizer => draw_visualizer(frame, app, chunks[1]),
        ViewMode::Settings => draw_settings(frame, app, chunks[1]),
        ViewMode::Playlists => draw_playlists(frame, app, chunks[1]),
    }

    // Now playing
    draw_now_playing(frame, app, chunks[2]);

    // Status bar
    draw_status_bar(frame, app, chunks[3]);

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

fn draw_playlist(frame: &mut Frame, app: &mut App, area: Rect) {
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

fn draw_browser(frame: &mut Frame, app: &mut App, area: Rect) {
    let path_str = app.browser.current_dir().display().to_string();
    let title = if path_str.len() > 50 {
        format!(" ...{} ", &path_str[path_str.len() - 47..])
    } else {
        format!(" {} ", path_str)
    };

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

    let browser_widget = List::new(slice.iter().cloned().collect::<Vec<_>>())
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol(">> ");

    frame.render_stateful_widget(browser_widget, area, &mut state);
}

fn draw_help(frame: &mut Frame, app: &mut App, area: Rect) {
    let help_text = command::help_text();
    let line_count = help_text.lines().count() as u16;
    let visible_height = area.height.saturating_sub(2); // Account for borders

    // Clamp scroll to valid range
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

fn draw_track_info(frame: &mut Frame, app: &App, area: Rect) {
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

fn draw_now_playing(frame: &mut Frame, app: &App, area: Rect) {
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

fn draw_visualizer(frame: &mut Frame, app: &App, area: Rect) {
    let vis_data = if app.spectrum_mode {
        app.player.vis_data()
    } else {
        app.player.vis_rms()
    };

    // Use the full height of the content area for visualization
    let inner_height = area.height.saturating_sub(2) as usize; // Account for borders
    let inner_width = area.width.saturating_sub(2) as usize;

    let mut lines = Vec::with_capacity(inner_height);

    if let Some(data) = vis_data {
        match app.visualizer_style {
            VisualizerStyle::Bars => {
                draw_vis_bars(&mut lines, &data, inner_height, inner_width);
            }
            VisualizerStyle::Spectrum => {
                draw_vis_spectrum(&mut lines, &data, inner_height, inner_width);
            }
            VisualizerStyle::Waveform => {
                draw_vis_waveform(&mut lines, &data, inner_height, inner_width);
            }
            VisualizerStyle::LevelMeter => {
                draw_vis_level_meter(&mut lines, &data, inner_height, inner_width);
            }
        }
    } else {
        // No audio data - show a message
        let msg = "No audio playing";
        let padding = (inner_height / 2).saturating_sub(1);
        for _ in 0..padding {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::DarkGray).italic(),
        )));
    }

    let title = format!(
        " Visualizer: {} (s:style, f:FFT/Vol, v/Esc:close) ",
        app.visualizer_style.name()
    );
    let visualizer = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .alignment(Alignment::Center);

    frame.render_widget(visualizer, area);
}

fn draw_vis_bars(lines: &mut Vec<Line<'static>>, data: &[f32], height: usize, width: usize) {
    let vis_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    // Pick a fixed bar width based on terminal width.
    let bw = if width >= 200 {
        3u16
    } else if width >= 100 {
        2
    } else {
        1
    };
    let gap = 1u16;
    let total_per_bar = (bw + gap) as u16;
    let num_bars = (width as u16 / total_per_bar)
        .min(data.len().max(1) as u16)
        .max(1) as usize;
    let total_width = (num_bars as u16 * total_per_bar).saturating_sub(gap) as usize;
    let pad_left = (width.saturating_sub(total_width)) / 2;

    for row in (0..height).rev() {
        let threshold = (row as f32 + 0.5) / height as f32;
        let mut line_content = String::with_capacity(width);
        for _ in 0..pad_left {
            line_content.push(' ');
        }

        for bar_idx in 0..num_bars {
            // Map bar to spectrum range with RMS
            let start = (bar_idx * data.len()) / num_bars;
            let end = ((bar_idx + 1) * data.len()) / num_bars;
            let amp: f32 =
                data[start..end].iter().map(|s| s * s).sum::<f32>() / (end - start) as f32;
            let scaled_amp = amp.sqrt().powf(0.35).min(1.0);

            if scaled_amp >= threshold {
                let level = (((scaled_amp - threshold) * height as f32 * 8.0) as usize).min(7);
                for _ in 0..bw {
                    line_content.push(vis_chars[level]);
                }
            } else {
                for _ in 0..bw {
                    line_content.push(' ');
                }
            }
            if bar_idx + 1 < num_bars {
                for _ in 0..gap {
                    line_content.push(' ');
                }
            }
        }

        lines.push(Line::from(Span::styled(
            line_content,
            Style::default().fg(Color::Cyan),
        )));
    }
}

fn draw_vis_spectrum(lines: &mut Vec<Line<'static>>, data: &[f32], height: usize, width: usize) {
    let vis_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    let bw = if width >= 200 {
        3u16
    } else if width >= 100 {
        2
    } else {
        1
    };
    let gap = 1u16;
    let total_per_bar = (bw + gap) as u16;
    let num_bars = (width as u16 / total_per_bar)
        .min(data.len().max(1) as u16)
        .max(1) as usize;
    let total_width = (num_bars as u16 * total_per_bar).saturating_sub(gap) as usize;
    let pad_left = (width.saturating_sub(total_width)) / 2;
    let half_height = height / 2;

    // Draw mirrored spectrum.
    //
    // Top half: lower block chars fill from cell bottom (which sits on the
    // center line), so bars naturally grow UP from center.
    //
    // Bottom half: line-level mirror of the top half. Lower block chars fill
    // from the cell bottom, which in the bottom half is away from center.
    // The silhouette (envelope of bar heights) mirrors correctly; partial-fill
    // characters within individual cells may appear to grow from the opposite
    // direction, but the overall shape is symmetric.
    let mut top_lines: Vec<String> = Vec::with_capacity(half_height);

    for row in (0..half_height).rev() {
        let threshold = (row as f32 + 0.5) / half_height as f32;
        let mut line_content = String::with_capacity(width);
        for _ in 0..pad_left {
            line_content.push(' ');
        }

        for bar_idx in 0..num_bars {
            let start = (bar_idx * data.len()) / num_bars;
            let end = ((bar_idx + 1) * data.len()) / num_bars;
            let amp: f32 =
                data[start..end].iter().map(|s| s * s).sum::<f32>() / (end - start) as f32;
            let scaled_amp = amp.sqrt().powf(0.35).min(1.0);

            if scaled_amp >= threshold {
                let level = (((scaled_amp - threshold) * half_height as f32 * 8.0) as usize).min(7);
                for _ in 0..bw {
                    line_content.push(vis_chars[level]);
                }
            } else {
                for _ in 0..bw {
                    line_content.push(' ');
                }
            }
            if bar_idx + 1 < num_bars {
                for _ in 0..gap {
                    line_content.push(' ');
                }
            }
        }
        top_lines.push(line_content);
    }

    for line in top_lines.iter() {
        lines.push(Line::from(Span::styled(
            line.clone(),
            Style::default().fg(Color::Magenta),
        )));
    }
    for line in top_lines.iter().rev() {
        lines.push(Line::from(Span::styled(
            line.clone(),
            Style::default().fg(Color::Cyan),
        )));
    }
}

fn draw_vis_waveform(lines: &mut Vec<Line<'static>>, data: &[f32], height: usize, width: usize) {
    let center_row = height / 2;

    // Build the waveform grid
    let mut grid: Vec<Vec<char>> = vec![vec![' '; width]; height];

    for x in 0..width {
        let data_idx = (x * data.len()) / width;
        let amp = data[data_idx.min(data.len() - 1)];

        // Convert amplitude to y offset from center
        let y_offset = (amp.powf(0.35) * center_row as f32) as isize;
        let y = (center_row as isize - y_offset).clamp(0, (height - 1) as isize) as usize;

        grid[y][x] = '●';

        // Draw vertical line from center to point
        let start_y = center_row.min(y);
        let end_y = center_row.max(y);
        for row in start_y..=end_y {
            if grid[row][x] == ' ' {
                grid[row][x] = '│';
            }
        }
    }

    // Draw center line
    for x in 0..width {
        if grid[center_row][x] == ' ' {
            grid[center_row][x] = '─';
        }
    }

    // Convert grid to lines
    for row in &grid {
        let line_str: String = row.iter().collect();
        lines.push(Line::from(Span::styled(
            line_str,
            Style::default().fg(Color::Green),
        )));
    }
}

fn draw_vis_level_meter(lines: &mut Vec<Line<'static>>, data: &[f32], height: usize, width: usize) {
    // Average amplitude for left and right channels (simple stereo simulation)
    let mid = data.len() / 2;
    let left_amp: f32 = data[..mid].iter().sum::<f32>() / mid.max(1) as f32;
    let right_amp: f32 = data[mid..].iter().sum::<f32>() / (data.len() - mid).max(1) as f32;
    let total_amp: f32 = data.iter().sum::<f32>() / data.len().max(1) as f32;

    let meter_width = width.saturating_sub(10);
    let left_filled = (left_amp.powf(0.35) * meter_width as f32) as usize;
    let right_filled = (right_amp.powf(0.35) * meter_width as f32) as usize;
    let total_filled = (total_amp.powf(0.35) * meter_width as f32) as usize;

    // Create meter characters
    let create_meter = |filled: usize, total: usize| -> String {
        let mut result = String::new();
        for i in 0..total {
            if i < filled {
                result.push('█');
            } else {
                result.push('░');
            }
        }
        result
    };

    // Pad vertically to center
    let content_height = 7;
    let padding = (height.saturating_sub(content_height)) / 2;

    for _ in 0..padding {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  L  [", Style::default().fg(Color::Gray)),
        Span::styled(
            create_meter(left_filled, meter_width),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("]", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  R  [", Style::default().fg(Color::Gray)),
        Span::styled(
            create_meter(right_filled, meter_width),
            Style::default().fg(Color::Magenta),
        ),
        Span::styled("]", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" Mix [", Style::default().fg(Color::Gray)),
        Span::styled(
            create_meter(total_filled, meter_width),
            Style::default().fg(Color::Green),
        ),
        Span::styled("]", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
}

fn draw_playlists(frame: &mut Frame, app: &mut App, area: Rect) {
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

fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
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

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
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
