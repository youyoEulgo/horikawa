//! Directory-based playlist management.
//!
//! Directory playlists store references to directories rather than
//! individual track paths. When loaded, each directory is scanned
//! for audio files in real-time using the LibraryScanner.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::library::LibraryScanner;
use crate::playlist::Playlist;


/// Errors for directory playlist operations.
#[derive(Debug, Error)]
pub enum DirPlaylistError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Directory not found: {0}")]
    DirNotFound(PathBuf),

    #[error("Scan error: {0}")]
    ScanError(String),
}


/// Serializable structure stored as a `.horikawa` JSON file.
///
/// Contains one or more directory references. When loaded,
/// each directory is recursively scanned for audio files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryPlaylist {
    /// Display name for the playlist.
    pub name: String,

    /// One or more directory paths to scan when loaded.
    pub directories: Vec<String>,
}


impl DirectoryPlaylist {
    /// Creates a new directory playlist with a single directory.
    pub fn new(name: String, directory: String) -> Self {
        Self {
            name,
            directories: vec![directory],
        }
    }


    /// Scans all directories and returns discovered audio file paths.
    ///
    /// Uses `LibraryScanner` for recursive scanning with the same
    /// supported audio extensions (mp3, flac, ogg, wav, m4a, aac,
    /// opus, wma, aiff, alac).
    pub fn scan(&self) -> Result<Vec<PathBuf>, DirPlaylistError> {
        let mut scanner = LibraryScanner::new();

        for dir_str in &self.directories {
            let dir = PathBuf::from(dir_str);
            if !dir.is_dir() {
                return Err(DirPlaylistError::DirNotFound(dir));
            }
            scanner.add_root(dir);
        }

        let tracks = scanner
            .scan()
            .map_err(|e| DirPlaylistError::ScanError(e.to_string()))?;

        Ok(tracks.into_iter().map(|t| t.path).collect())
    }


    /// Saves this directory playlist to a `.horikawa` JSON file.
    ///
    /// The file is written to the standard playlist directory
    /// (`~/.local/share/horikawa/playlists/<name>.horikawa`).
    pub fn save(&self) -> Result<PathBuf, DirPlaylistError> {
        let dir = Self::playlist_dir()
            .ok_or_else(|| {
                DirPlaylistError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not determine playlist directory",
                ))
            })?;

        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.horikawa", self.name));

        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json)?;

        Ok(path)
    }


    /// Loads a directory playlist from a `.horikawa` JSON file.
    pub fn load(name: &str) -> Result<Self, DirPlaylistError> {
        let dir = Self::playlist_dir()
            .ok_or_else(|| {
                DirPlaylistError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not determine playlist directory",
                ))
            })?;

        let path = dir.join(format!("{}.horikawa", name));
        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let dpl: Self = serde_json::from_reader(reader)?;
        Ok(dpl)
    }


    /// Lists all saved directory playlist names.
    ///
    /// Returns names sorted alphabetically (excluding `_last`).
    pub fn list() -> Result<Vec<String>, DirPlaylistError> {
        let dir = match Self::playlist_dir() {
            Some(d) => d,
            None => return Ok(Vec::new()),
        };

        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("horikawa") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    if name != "_last" {
                        names.push(name.to_string());
                    }
                }
            }
        }
        names.sort();
        Ok(names)
    }


    /// Deletes a directory playlist file.
    pub fn delete(name: &str) -> Result<(), DirPlaylistError> {
        let dir = Self::playlist_dir()
            .ok_or_else(|| {
                DirPlaylistError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not determine playlist directory",
                ))
            })?;

        let path = dir.join(format!("{}.horikawa", name));
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }


    /// Gets the playlist directory (same as M3U playlists).
    ///
    /// Uses `~/.local/share/horikawa/playlists` on Linux/macOS,
    /// or `Music/Horikawa` on Windows.
    pub fn playlist_dir() -> Option<PathBuf> {
        Playlist::playlist_dir()
    }
}
