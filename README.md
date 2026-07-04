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

### Windows Installer

Download the latest installer from [Releases](https://github.com/youyoEulgo/horikawa/releases).

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

### Global (all views)

| Key                 | Action                            |
| ------------------- | --------------------------------- |
| `Tab` / `Shift+Tab` | Next / previous view              |
| `/`                 | Enter slash command mode          |
| `H`                 | Show per-view shortcut help popup |
| `q`                 | Quit                              |

### Playback (available in most views)

| Key                 | Action                                  |
| ------------------- | --------------------------------------- |
| `Space`             | Play / Pause                            |
| `h` / `←`           | Previous track                          |
| `l` / `→`           | Next track                              |
| `Ctrl+h` / `Ctrl+l` | Seek backward / forward 10s             |
| `Ctrl+←` / `Ctrl+→` | Seek backward / forward 10s (non-macOS) |
| `+` / `=`           | Volume up                               |
| `-` / `_`           | Volume down                             |
| `m`                 | Mute / unmute                           |

### Playlist View

| Key                   | Action                                     |
| --------------------- | ------------------------------------------ |
| `↑` / `k`             | Move selection up                          |
| `↓` / `j`             | Move selection down                        |
| `g` / `Home`          | Go to first track                          |
| `G` / `End`           | Go to last track                           |
| `Enter`               | Play selected track                        |
| `s`                   | Save current playlist as M3U (name prompt) |
| `S`                   | Toggle shuffle                             |
| `r`                   | Cycle repeat mode (Off → One → All)        |
| `R`                   | Reload last loaded playlist                |
| `e`                   | Toggle edit mode                           |
| `d`                   | Delete track (edit mode only)              |
| `c`                   | Clear playlist (edit mode only)            |
| `Shift+J` / `Shift+K` | Move track down / up (edit mode only)      |
| `v`                   | Open Visualizer                            |
| `p`                   | Open Playlists view                        |
| `b`                   | Open Browser view                          |
| `i`                   | Open Track Info view                       |
| `H`                   | Show Playlist shortcuts popup              |

### Browser View

| Key                     | Action                                             |
| ----------------------- | -------------------------------------------------- |
| `↑` / `k`               | Move selection up                                  |
| `↓` / `j`               | Move selection down                                |
| `l` / `Enter` / `→`     | Enter directory / add file to playlist             |
| `h` / `Backspace` / `←` | Go to parent directory                             |
| `a`                     | Add selected file or folder to playlist            |
| `s`                     | Save selected directory as M3U (name prompt)       |
| `S`                     | Save selected directory as .horikawa (name prompt) |
| `R`                     | Refresh directory listing                          |
| `~`                     | Go to home directory                               |
| `g` / `Home`            | Go to first entry                                  |
| `G` / `End`             | Go to last entry                                   |
| `b` / `Esc`             | Return to Playlist view                            |
| `H`                     | Show Browser shortcuts popup                       |

### Playlists View

| Key         | Action                                        |
| ----------- | --------------------------------------------- |
| `↑` / `k`   | Move selection up                             |
| `↓` / `j`   | Move selection down                           |
| `Enter`     | Load selected playlist                        |
| `d`         | Delete selected playlist (confirmation popup) |
| `r`         | Rename selected playlist (input popup)        |
| `p` / `Esc` | Return to Playlist view                       |
| `H`         | Show Playlists shortcuts popup                |

### Track Info View

| Key         | Action                  |
| ----------- | ----------------------- |
| `i` / `Esc` | Return to Playlist view |

### Visualizer View

| Key         | Action                                                            |
| ----------- | ----------------------------------------------------------------- |
| `s`         | Cycle visualizer style (Bars → Spectrum → Waveform → Level Meter) |
| `v` / `Esc` | Return to Playlist view                                           |

### Settings View

| Key       | Action                  |
| --------- | ----------------------- |
| `↑` / `k` | Move selection up       |
| `↓` / `j` | Move selection down     |
| `Enter`   | Toggle setting          |
| `Esc`     | Return to Playlist view |

### Help View

| Key             | Action         |
| --------------- | -------------- |
| `↑` / `k`       | Scroll up      |
| `↓` / `j`       | Scroll down    |
| `PgUp` / `PgDn` | Page up / down |
| `?` / `Esc`     | Close help     |

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

### Docker Build

```bash
docker-compose up build-all
# Outputs to ./dist/
```

## License

MIT
