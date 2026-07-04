# Horikawa

A lightweight Terminal User Interface (TUI) music player written in Rust.

![License](https://img.shields.io/badge/license-MIT-blue.svg)

## Features

- **Audio Playback** — Play, pause, seek, volume control, next/previous track
- **Multiple Formats** — MP3, FLAC, OGG, WAV, M4A/AAC, OPUS, WMA, AIFF, ALAC
- **Visualizers** — Bars, spectrum, waveform, and level meter (~30 FPS)
- **Playlist Management** — M3U and directory-based (.horikawa) playlists, shuffle, repeat, reorder, dedup
- **File Browser** — Navigate local directories, add files/folders, quick-save as playlist
- **Playlists View** — Dedicated TUI view to browse, load, delete, and rename saved playlists
- **Popup Dialogs** — Input popups for naming playlists, confirmation popups for destructive actions
- **Shortcut Help** — `H` key shows per-view keyboard shortcuts
- **Vim-style Keybindings** — `h/j/k/l` for navigation and playback
- **Session Persistence** — Remembers playlist, position, volume, settings, and last loaded playlist
- **Slash Commands** — `/playlist`, `/dirplaylist`, `/queue`, `/goto`, `/seek`, `/reload`, etc.
- **Platform Integration**
  - macOS: Now Playing in Control Center, media keys
  - Linux: MPRIS D-Bus (KDE/GNOME media controls)
  - Windows: System Media Transport Controls (lock screen, media keys)
  - Discord Rich Presence

## Installation

### From Source

**Prerequisites:**
- Rust 1.70+
- Linux: ALSA development headers (`libasound2-dev` on Debian/Ubuntu)

```bash
git clone https://github.com/youyoEulgo/horikawa.git
cd horikawa
cargo build --release
```

The binary will be at `target/release/horikawa`.

**A Nerd Font is required** for icons (folder, music, play/pause/stop). We recommend [FiraCode Nerd Font](https://github.com/ryanoasis/nerd-fonts/tree/master/patched-fonts/FiraCode) or any other Nerd Font.

## Usage

```bash
# Launch with file browser
horikawa --browse

# Open a directory
horikawa --path /path/to/music

# Play specific files
horikawa track1.mp3 track2.flac

# Play all audio in a directory
horikawa /path/to/music/
```

## Keyboard Shortcuts

### Playlist

| Key            | Action                                     |
| -------------- | ------------------------------------------ |
| `Space`        | Play / Pause                               |
| `h` / `←`      | Previous track                             |
| `l` / `→`      | Next track                                 |
| `Enter`        | Play the selected track                    |
| `k` / `↑`      | Move selection up                          |
| `j` / `↓`      | Move selection down                        |
| `g` / `Home`   | Jump to first track                        |
| `G` / `End`    | Jump to last track                         |
| `+` / `-`      | Volume up / down                           |
| `m`            | Mute / unmute                              |
| `s`            | Save current playlist as M3U (name prompt) |
| `S`            | Toggle shuffle                             |
| `r`            | Cycle repeat mode (Off → One → All)        |
| `R`            | Reload the last loaded playlist            |
| `Ctrl+h` / `←` | Rewind 10 seconds                          |
| `Ctrl+l` / `→` | Fast-forward 10 seconds                    |

Edit Mode (`e` to enter):

| Key                   | Action                        |
| --------------------- | ----------------------------- |
| `e`                   | Toggle edit mode              |
| `Shift+j` / `Shift+k` | Move selected track down / up |
| `d`                   | Delete selected track         |
| `c`                   | Clear entire playlist         |

Views:

| Key                 | Action               |
| ------------------- | -------------------- |
| `Tab` / `Shift+Tab` | Next / previous view |
| `v`                 | Open Visualizer      |
| `p`                 | Open Playlists       |
| `b`                 | Open Browser         |
| `i`                 | Open Track Info      |

Other:

| Key | Action                            |
| --- | --------------------------------- |
| `/` | Enter slash command mode          |
| `H` | Show per-view shortcut help popup |
| `q` | Quit                              |

### Browser

| Key                     | Action                                                     |
| ----------------------- | ---------------------------------------------------------- |
| `h` / `←` / `Backspace` | Go to parent directory                                     |
| `l` / `→` / `Enter`     | Enter directory, or play file (adds to playlist if new)    |
| `k` / `↑`               | Move selection up                                          |
| `j` / `↓`               | Move selection down                                        |
| `a`                     | Add selected file or folder to playlist (skips duplicates) |
| `Enter`                 | Enter directory, or play file (adds to playlist if new)    |
| `s`                     | Save selected directory as M3U (name prompt)               |
| `S`                     | Save selected directory as .horikawa (name prompt)         |
| `.`                     | Toggle hidden files (.dotfiles)                            |
| `R`                     | Refresh directory listing                                  |
| `~`                     | Go to home directory                                       |
| `g` / `Home`            | Jump to first entry                                        |
| `G` / `End`             | Jump to last entry                                         |

Views:

| Key                 | Action               |
| ------------------- | -------------------- |
| `Tab` / `Shift+Tab` | Next / previous view |
| `v`                 | Open Visualizer      |
| `p`                 | Open Playlists       |
| `b` / `Esc`         | Return to Playlist   |
| `i`                 | Open Track Info      |

Other:

| Key | Action                       |
| --- | ---------------------------- |
| `/` | Enter slash command mode     |
| `H` | Show Browser shortcuts popup |
| `q` | Quit                         |

### Playlists

| Key       | Action                                        |
| --------- | --------------------------------------------- |
| `Enter`   | Load the selected playlist                    |
| `d`       | Delete selected playlist (confirmation popup) |
| `r`       | Rename selected playlist (input popup)        |
| `Space`   | Play / Pause                                  |
| `k` / `↑` | Move selection up                             |
| `j` / `↓` | Move selection down                           |
| `+` / `-` | Volume up / down                              |
| `h` / `←` | Previous track                                |
| `l` / `→` | Next track                                    |
| `m`       | Mute / unmute                                 |

Views:

| Key                 | Action               |
| ------------------- | -------------------- |
| `Tab` / `Shift+Tab` | Next / previous view |
| `v`                 | Open Visualizer      |
| `p` / `Esc`         | Return to Playlist   |
| `b`                 | Open Browser         |
| `i`                 | Open Track Info      |

Other:

| Key | Action                         |
| --- | ------------------------------ |
| `/` | Enter slash command mode       |
| `H` | Show Playlists shortcuts popup |
| `q` | Quit                           |

### Track Info

| Key            | Action                  |
| -------------- | ----------------------- |
| `Space`        | Play / Pause            |
| `h` / `←`      | Previous track          |
| `l` / `→`      | Next track              |
| `Ctrl+h` / `←` | Rewind 10 seconds       |
| `Ctrl+l` / `→` | Fast-forward 10 seconds |
| `+` / `-`      | Volume up / down        |
| `m`            | Mute / unmute           |

Views:

| Key                 | Action               |
| ------------------- | -------------------- |
| `Tab` / `Shift+Tab` | Next / previous view |
| `v`                 | Open Visualizer      |
| `p`                 | Open Playlists       |
| `b`                 | Open Browser         |
| `i` / `Esc`         | Return to Playlist   |

Other:

| Key | Action                          |
| --- | ------------------------------- |
| `/` | Enter slash command mode        |
| `H` | Show Track Info shortcuts popup |
| `q` | Quit                            |

### Visualizer

| Key            | Action                                                 |
| -------------- | ------------------------------------------------------ |
| `s`            | Cycle style (Bars → Spectrum → Waveform → Level Meter) |
| `f`            | Toggle FFT spectrum / RMS volume meter                 |
| `Space`        | Play / Pause                                           |
| `h` / `←`      | Previous track                                         |
| `l` / `→`      | Next track                                             |
| `Ctrl+h` / `←` | Rewind 10 seconds                                      |
| `Ctrl+l` / `→` | Fast-forward 10 seconds                                |
| `+` / `-`      | Volume up / down                                       |
| `m`            | Mute / unmute                                          |

Views:

| Key                 | Action               |
| ------------------- | -------------------- |
| `Tab` / `Shift+Tab` | Next / previous view |
| `v` / `Esc`         | Return to Playlist   |
| `p`                 | Open Playlists       |
| `b`                 | Open Browser         |
| `i`                 | Open Track Info      |

Other:

| Key | Action                          |
| --- | ------------------------------- |
| `/` | Enter slash command mode        |
| `H` | Show Visualizer shortcuts popup |
| `q` | Quit                            |

### Settings

| Key       | Action                      |
| --------- | --------------------------- |
| `Enter`   | Toggle the selected setting |
| `k` / `↑` | Move selection up           |
| `j` / `↓` | Move selection down         |
| `Space`   | Play / Pause                |
| `h` / `←` | Previous track              |
| `l` / `→` | Next track                  |
| `+` / `-` | Volume up / down            |
| `m`       | Mute / unmute               |

Other:

| Key   | Action                        |
| ----- | ----------------------------- |
| `/`   | Enter slash command mode      |
| `Esc` | Return to Playlist            |
| `H`   | Show Settings shortcuts popup |
| `q`   | Quit                          |

### Help

| Key       | Action      |
| --------- | ----------- |
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up   |
| `PgDn`    | Page down   |
| `PgUp`    | Page up     |

Other:

| Key         | Action                    |
| ----------- | ------------------------- |
| `/`         | Enter slash command mode  |
| `Esc` / `?` | Close help                |
| `H`         | Show Help shortcuts popup |
| `q`         | Quit                      |

## Views

Press **Tab** / **Shift+Tab** to cycle through views:

```
Playlist → Browser → Playlists → Track Info → Visualizer → Settings
```

Quick-access keys from Playlist view: `b` Browser, `p` Playlists, `i` Track Info, `v` Visualizer.

## Slash Commands

Type `/` to enter command mode, then use any of these:

### Queue

| Command             | Alias   | Action                      |
| ------------------- | ------- | --------------------------- |
| `/queue add <path>` | `/q a`  | Add file or folder to queue |
| `/queue remove`     | `/q rm` | Remove selected track       |
| `/queue clear`      | `/q cl` | Clear queue                 |
| `/queue dedup`      | `/q`    | Remove duplicate tracks     |

### Playlist

| Command                   | Alias      | Action                      |
| ------------------------- | ---------- | --------------------------- |
| `/playlist save <name>`   | `/pl save` | Save current playlist (M3U) |
| `/playlist load <name>`   | `/pl load` | Load saved playlist         |
| `/playlist list`          | `/pl ls`   | List saved playlists        |
| `/playlist delete <name>` | `/pl del`  | Delete saved playlist       |

### Directory Playlist

| Command                          | Alias         | Action                                   |
| -------------------------------- | ------------- | ---------------------------------------- |
| `/dirplaylist save <name> [dir]` | `/dirpl save` | Save directory as playlist (`.horikawa`) |
| `/dirplaylist load <name>`       | `/dirpl load` | Load and scan directory playlist         |
| `/dirplaylist list`              | `/dirpl ls`   | List saved directory playlists           |
| `/dirplaylist delete <name>`     | `/dirpl del`  | Delete directory playlist                |

### Navigation and Playback

| Command                   | Action                          |
| ------------------------- | ------------------------------- |
| `/goto <path>`            | Navigate browser to path        |
| `/search <term>`          | Filter current view             |
| `/home`                   | Go to home directory            |
| `/seek <time>`            | Seek to position (e.g., `1:30`) |
| `/shuffle`                | Toggle shuffle                  |
| `/repeat [off\|one\|all]` | Set repeat mode                 |
| `/vol [0-100]`            | Set or show volume              |
| `/reload`                 | Reload last loaded playlist     |
| `/vis`                    | Toggle visualizer               |
| `/help`                   | Show help                       |
| `/quit`                   | Quit application                |

## Configuration

Settings are stored at:
- Linux/macOS: `~/.config/horikawa/settings.json`
- Windows: `%APPDATA%\horikawa\settings.json`

Playlists are stored at:
- Linux: `~/.local/share/horikawa/playlists/`
- macOS: `~/Library/Application Support/horikawa/playlists/`
- Windows: `Music/Horikawa/`

```json
{
  "discord_enabled": true,
  "smtc_enabled": true
}
```

## Building

### Native Build

```bash
cargo build --release
```

### Cross-Compile for Windows (from Linux)

```bash
cargo build --release --target x86_64-pc-windows-gnu
```

## License

MIT
