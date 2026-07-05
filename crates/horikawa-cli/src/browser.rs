//! File browser for directory navigation.
//!
//! Provides a file browser that can navigate directories,
//! filter entries, and select files for adding to the playlist.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// Supported audio extensions for highlighting.
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "wav", "m4a", "aac", "opus", "wma", "aiff", "alac",
];

/// A file or directory entry in the browser.
#[derive(Debug, Clone)]
pub struct BrowserEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_audio: bool,
}

/// File browser state.
#[derive(Debug)]
pub struct FileBrowser {
    current_dir: PathBuf,
    entries: Vec<BrowserEntry>,
    filtered_indices: Vec<usize>,
    selected: usize,
    filter: String,
    show_hidden: bool,
    scroll_offset: usize,
}

impl FileBrowser {
    /// Creates a new file browser at the given path.
    pub fn new(path: PathBuf) -> Result<Self> {
        let mut browser = Self {
            current_dir: path,
            entries: Vec::new(),
            filtered_indices: Vec::new(),
            selected: 0,
            filter: String::new(),
            show_hidden: false,
            scroll_offset: 0,
        };
        browser.refresh()?;
        Ok(browser)
    }

    /// Refreshes the directory listing.
    pub fn refresh(&mut self) -> Result<()> {
        self.entries.clear();
        self.filtered_indices.clear();
        self.selected = 0;
        self.scroll_offset = 0;

        // Add parent directory entry (unless at root)
        if let Some(parent) = self.current_dir.parent() {
            self.entries.push(BrowserEntry {
                path: parent.to_path_buf(),
                name: "..".to_string(),
                is_dir: true,
                is_audio: false,
            });
        }

        // Read directory contents
        let mut dirs = Vec::new();
        let mut files = Vec::new();

        let read_result = fs::read_dir(&self.current_dir);
        if let Ok(entries) = read_result {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();

                // Skip hidden files unless show_hidden is enabled
                if name != ".." && name.starts_with('.') && !self.show_hidden {
                    continue;
                }

                let is_dir = path.is_dir();
                let is_audio = !is_dir && Self::is_audio_file(&path);

                let browser_entry = BrowserEntry {
                    path,
                    name,
                    is_dir,
                    is_audio,
                };

                if is_dir {
                    dirs.push(browser_entry);
                } else if is_audio {
                    files.push(browser_entry);
                }
            }
        }

        // Sort directories and files separately (case-insensitive)
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        // Directories first, then files
        self.entries.extend(dirs);
        self.entries.extend(files);

        self.apply_filter();
        Ok(())
    }

    /// Navigates to a specific path.
    pub fn navigate_to(&mut self, path: &Path) -> Result<()> {
        let canonical = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.current_dir.join(path)
        };

        if canonical.is_dir() {
            self.current_dir = canonical;
            self.filter.clear();
            self.refresh()?;
        }
        Ok(())
    }

    /// Enters the selected directory or returns the selected file path.
    ///
    /// @returns Some(path) if a file was selected, None if entered a directory
    pub fn enter_selected(&mut self) -> Result<Option<PathBuf>> {
        if let Some(entry) = self.selected_entry() {
            let entry = entry.clone();
            if entry.is_dir {
                self.navigate_to(&entry.path)?;
                Ok(None)
            } else {
                Ok(Some(entry.path))
            }
        } else {
            Ok(None)
        }
    }

    /// Goes up to parent directory.
    pub fn go_up(&mut self) -> Result<()> {
        if let Some(parent) = self.current_dir.parent() {
            let parent = parent.to_path_buf();
            self.navigate_to(&parent)?;
        }
        Ok(())
    }

    /// Sets the filter text and updates visible entries.
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.apply_filter();
    }

    /// Clears the filter.
    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.apply_filter();
    }

    fn apply_filter(&mut self) {
        self.filtered_indices.clear();

        let filter_lower = self.filter.to_lowercase();

        for (idx, entry) in self.entries.iter().enumerate() {
            // Always show ".." parent directory
            if entry.name == ".." {
                self.filtered_indices.push(idx);
                continue;
            }

            if self.filter.is_empty() || entry.name.to_lowercase().contains(&filter_lower) {
                self.filtered_indices.push(idx);
            }
        }

        // Adjust selection if out of bounds
        if self.selected >= self.filtered_indices.len() {
            self.selected = self.filtered_indices.len().saturating_sub(1);
        }
    }

    /// Yazi-style scroll: moves scroll_offset at most ±1 per step, only
    /// when the selected item enters the MARGIN zone at the top or bottom edge.
    const SCROLL_MARGIN: usize = 4;

    fn clamp_scroll(&mut self, visible_height: usize, jumped: bool) {
        let total = self.filtered_indices.len();
        if total == 0 || visible_height <= 1 {
            self.scroll_offset = 0;
            return;
        }
        if total <= visible_height {
            self.scroll_offset = 0;
            return;
        }
        let max = total - visible_height;
        if jumped {
            // Large jump: keep selection visible with margin
            if self.selected < self.scroll_offset + Self::SCROLL_MARGIN {
                self.scroll_offset = self.selected.saturating_sub(Self::SCROLL_MARGIN);
            }
            if self.selected > self.scroll_offset + visible_height.saturating_sub(Self::SCROLL_MARGIN + 2) {
                self.scroll_offset = (self.selected + Self::SCROLL_MARGIN + 1)
                    .saturating_sub(visible_height)
                    .min(max);
            }
        } else {
            let rel = self.selected.saturating_sub(self.scroll_offset);
            if rel <= Self::SCROLL_MARGIN {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            if rel >= visible_height.saturating_sub(Self::SCROLL_MARGIN + 1) {
                self.scroll_offset = (self.scroll_offset + 1).min(max);
            }
        }
    }

    /// Moves selection down.
    pub fn select_next(&mut self, visible_height: usize) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let old = self.selected;
        self.selected = (self.selected + 1) % self.filtered_indices.len();
        self.clamp_scroll(visible_height, self.selected < old);
    }

    /// Moves selection up.
    pub fn select_previous(&mut self, visible_height: usize) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let jumped = self.selected == 0;
        self.selected = if jumped {
            self.filtered_indices.len() - 1
        } else {
            self.selected - 1
        };
        self.clamp_scroll(visible_height, jumped);
    }

    /// Jumps to first entry.
    pub fn select_first(&mut self, visible_height: usize) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.selected = 0;
        self.clamp_scroll(visible_height, true);
    }

    /// Jumps to last entry.
    pub fn select_last(&mut self, visible_height: usize) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.selected = self.filtered_indices.len() - 1;
        self.clamp_scroll(visible_height, true);
    }

    /// Gets the current scroll offset for the UI list.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Gets the currently selected entry.
    pub fn selected_entry(&self) -> Option<&BrowserEntry> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|&idx| self.entries.get(idx))
    }

    /// Gets visible entries (filtered).
    pub fn visible_entries(&self) -> Vec<&BrowserEntry> {
        self.filtered_indices
            .iter()
            .filter_map(|&idx| self.entries.get(idx))
            .collect()
    }

    /// Gets the selected index for UI state.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Gets the current directory path.
    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    /// Toggles showing hidden files/directories.
    pub fn toggle_hidden(&mut self) -> Result<()> {
        self.show_hidden = !self.show_hidden;
        self.refresh()
    }

    fn is_audio_file(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
            .unwrap_or(false)
    }
}

impl Default for FileBrowser {
    fn default() -> Self {
        let start_dir = dirs::home_dir()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        Self::new(start_dir).expect("Failed to create file browser")
    }
}
