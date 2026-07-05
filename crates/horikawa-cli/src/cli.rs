//! Command-line argument parsing for Horikawa.

use std::path::PathBuf;

use clap::Parser;

/// Horikawa — a terminal music player.
#[derive(Parser, Debug)]
#[command(name = "horikawa")]
#[command( version, about, long_about = None )]
pub struct Args {
    /// Directory or file to open on startup.
    #[arg(short, long)]
    pub path: Option<PathBuf>,

    /// Start in file browser mode.
    #[arg(short, long)]
    pub browse: bool,

    /// Run as a headless daemon (no TUI).
    #[arg(short = 'D', long)]
    pub daemon: bool,

    /// Add files/directories to playlist and start playing.
    #[arg(trailing_var_arg = true)]
    pub files: Vec<PathBuf>,
}
