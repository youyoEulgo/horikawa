//! Horikawa CLI — entry point, argument parsing, and process wiring.

mod cli;

use std::io;
use std::panic;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use cli::Args;
use crossterm::{
    event::{self, Event, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{Terminal, backend::CrosstermBackend};

use horikawa_core::{library::LibraryScanner, Player};
use horikawa_ctl::{CommandProcessor, ControlChannel, ProcessorSettings};
use horikawa_protocol::AppCommand;

use horikawa_tui::{App, PlaylistEntry};

struct TuiGuard {
    active: bool,
}

impl TuiGuard {
    fn activate() -> io::Result<Self> {
        enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        io::stdout().execute(crossterm::event::EnableMouseCapture)?;
        Ok(Self { active: true })
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        restore_terminal();
        self.active = false;
    }
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

fn restore_terminal() {
    let _ = io::stdout().execute(crossterm::event::DisableMouseCapture);
    let _ = disable_raw_mode();
    let _ = io::stdout().execute(LeaveAlternateScreen);
}

fn install_panic_hook() {
    panic::set_hook(Box::new(|info| {
        restore_terminal();
        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_string());
        tracing::error!(
            panic.location = %location,
            panic.message = %payload,
            backtrace = %std::backtrace::Backtrace::force_capture(),
            "horikawa panicked"
        );
    }));
}

fn main() {
    if let Err(e) = run() {
        restore_terminal();
        tracing::error!(error = ?e, "horikawa exited with error");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // Log to file so nothing spills into the TUI alternate screen.
    let log_path = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("horikawa")
        .join("horikawa.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .unwrap_or_else(|_| {
            // fallback: /dev/null equivalent
            std::fs::File::create("/tmp/horikawa.log").unwrap()
        });
    tracing_subscriber::fmt()
        .with_env_filter("horikawa=info")
        .with_ansi(false)
        .with_thread_names(true)
        .with_target(true)
        .with_writer(std::sync::Mutex::new(log_file))
        .init();
    install_panic_hook();
    tracing::info!("Horikawa starting; log_path={:?}", log_path);

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
            session_last_loaded = session.playlist_name
                .strip_prefix("m3u:")
                .map(|n| PlaylistEntry::M3u(n.to_string()))
                .or_else(|| {
                    session.playlist_name
                        .strip_prefix("dirpl:")
                        .map(|n| PlaylistEntry::DirPl(n.to_string()))
                });

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

    // Determine starting directory
    let start_path = args
        .path
        .clone()
        .or_else(dirs::home_dir)
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

    let mut processor = CommandProcessor::new(
        processor_player,
        proc_settings,
        start_path,
        args.browse,
        command_rx,
        broadcast_tx,
    );

    // Spawn command processor
    let processor_handle = std::thread::Builder::new()
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
    {
        let integrations_player = Arc::clone(&player);
        let integrations_sender = channel.sender();
        let integrations_rx = channel.subscribe();
        let integrations_settings = ProcessorSettings {
            discord_enabled: integrations_discord,
            #[cfg(target_os = "macos")]
            smtc_enabled: false,
            #[cfg(not(target_os = "macos"))]
            smtc_enabled: integrations_smtc
        };
        std::thread::Builder::new()
            .name("horikawa-integrations".to_string())
            .spawn(move || {
                horikawa_tui::integrations::run_integrations(
                    integrations_player,
                    integrations_sender,
                    integrations_rx,
                    &integrations_settings,
                );
            })
            .expect("Failed to spawn integrations thread");
    }

    // Daemon mode
    if args.daemon {
        let daemon_quit = Arc::new(AtomicBool::new(false));
        {
            let q = Arc::clone(&daemon_quit);
            ctrlc::set_handler(move || {
                q.store(true, Ordering::SeqCst);
            })?;
        }
        tracing::info!("Running in daemon mode (headless). Press Ctrl+C to stop.");
        while !daemon_quit.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(1));
        }
        tracing::info!("Daemon shutting down.");
        let _ = command_sender.try_send(AppCommand::Quit);
        if processor_handle.join().is_err() {
            tracing::error!("Command processor thread panicked during daemon shutdown");
        }
        return Ok(());
    }

    // --- TUI mode ---
    // Ctrl+C / SIGTERM handler: set flag, main loop exits gracefully
    let quit_signal = Arc::new(AtomicBool::new(false));
    {
        let q = Arc::clone(&quit_signal);
        ctrlc::set_handler(move || {
            q.store(true, Ordering::SeqCst);
        })?;
    }

    let mut tui_guard = TuiGuard::activate()?;

    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let state_rx = channel.subscribe();
    let mut app = App::new(player, command_sender, state_rx, args.path, args.browse)?;
    app.last_loaded = session_last_loaded;

    // macOS media controls on main thread
    #[cfg(target_os = "macos")]
    {
        let (smtc_tx, smtc_rx) = mpsc::channel();
        app.media_controls = horikawa_tui::media_controls::MediaControlsHandler::new(smtc_tx);
        app.smtc_rx = Some(smtc_rx);
    }

    // Main loop
    loop {
        app.tick();
        terminal.draw(|frame| horikawa_tui::draw_ui(frame, &mut app))?;

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

        horikawa_tui::media_controls::pump_run_loop();

        if app.should_quit || quit_signal.load(Ordering::SeqCst) {
            let _ = app.command_sender.try_send(AppCommand::Quit);
            break;
        }
    }

    // Cleanup before waiting so slow shutdown never leaves the terminal in TUI mode.
    tui_guard.restore();

    if processor_handle.join().is_err() {
        tracing::error!("Command processor thread panicked during shutdown");
    }

    Ok(())
}
