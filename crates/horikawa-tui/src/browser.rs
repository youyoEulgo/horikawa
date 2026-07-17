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
        tracing::info!("Refreshing browser directory: {:?}", self.current_dir);
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

        match fs::read_dir(&self.current_dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(e) => {
                            tracing::warn!(
                                "Failed to read browser entry in {:?}: {}",
                                self.current_dir,
                                e
                            );
                            continue;
                        }
                    };
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
            Err(e) => {
                tracing::warn!("Failed to read browser directory {:?}: {}", self.current_dir, e);
                return Err(e.into());
            }
        }

        // Sort directories and files separately (case-insensitive)
        dirs.sort_by_key(|a| a.name.to_lowercase());
        files.sort_by_key(|a| a.name.to_lowercase());

        // Directories first, then files
        self.entries.extend(dirs);
        self.entries.extend(files);

        self.apply_filter();
        tracing::info!(
            "Browser directory refreshed: {:?}; entries={}, dirs={}, audio_files={}",
            self.current_dir,
            self.entries.len(),
            self.entries.iter().filter(|e| e.is_dir).count(),
            self.entries.iter().filter(|e| e.is_audio).count()
        );
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
            tracing::info!("Browser navigating to directory: {:?}", canonical);
            self.current_dir = canonical;
            self.filter.clear();
            self.refresh()?;
        } else {
            tracing::warn!("Browser refused to navigate to non-directory: {:?}", canonical);
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

    /// Yazi-style scroll: ported from yazi-widgets/src/scrollable.rs.
    ///
    /// `scrolloff` = `visible_height / 2` keeps selection near the center.
    /// Offset moves only when selection enters the scrolloff zone at edges.
    fn scrolloff(visible_height: usize) -> usize {
        (visible_height / 2).min(5)
    }

    /// Moves selection down (j).
    pub fn select_next(&mut self, visible_height: usize) {
        let total = self.filtered_indices.len();
        if total == 0 { return; }
        let old = self.selected;
        self.selected = (self.selected + 1) % total;
        self.scroll_to(visible_height, old);
    }

    /// Moves selection up (k).
    pub fn select_previous(&mut self, visible_height: usize) {
        let total = self.filtered_indices.len();
        if total == 0 { return; }
        let old = self.selected;
        self.selected = if old == 0 { total - 1 } else { old - 1 };
        self.scroll_to(visible_height, old);
    }

    /// Jumps to first entry (g).
    pub fn select_first(&mut self, _visible_height: usize) {
        if self.filtered_indices.is_empty() { return; }
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Jumps to last entry (G).
    pub fn select_last(&mut self, visible_height: usize) {
        let total = self.filtered_indices.len();
        if total == 0 { return; }
        self.selected = total - 1;
        self.scroll_offset = total.saturating_sub(visible_height);
    }

    fn scroll_to(&mut self, visible_height: usize, old_cursor: usize) {
        let total = self.filtered_indices.len();
        if total <= visible_height || visible_height == 0 {
            self.scroll_offset = 0;
            return;
        }
        let scrolloff = Self::scrolloff(visible_height);
        let old_offset = self.scroll_offset;

        if self.selected > old_cursor {
            // next (down) — ported from yazi Scrollable::next()
            self.scroll_offset =
                if self.selected < total.min(old_offset + visible_height).saturating_sub(scrolloff) {
                    old_offset.min(total.saturating_sub(1))
                } else {
                    total.saturating_sub(visible_height)
                        .min(old_offset + self.selected - old_cursor)
                };
        } else {
            // prev (up) — ported from yazi Scrollable::prev()
            self.scroll_offset = if self.selected < old_offset + scrolloff {
                old_offset.saturating_sub(old_cursor - self.selected)
            } else {
                total.saturating_sub(1).min(old_offset)
            };
        }
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
