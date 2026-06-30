# Oxidio

A lightweight Terminal User Interface (TUI) music player written in Rust.

![License](https://img.shields.io/badge/license-MIT-blue.svg)

## Features

- **Audio Playback** - Play, pause, stop, seek, volume control, next/previous track
- **Multiple Formats** - MP3, FLAC, OGG, WAV, M4A/AAC, OPUS, WMA, AIFF, ALAC
- **Visualizers** - Bars, spectrum analyzer, waveform, and level meter (~30 FPS)
- **Playlist Management** - Shuffle, repeat modes (off/one/all), reordering, dedup, save/load (M3U)
- **Directory Playlists** - Save directory references; auto-scan for audio files on load (`.oxidio`)
- **File Browser** - Navigate local directories, add files/folders to playlist, quick-save as playlist
- **Session Persistence** - Remembers playlist, position, volume, and settings
- **Playlist Browser View** - Browse saved M3U and directory playlists in a dedicated TUI view
- **Popup Dialogs** - Input and confirmation popups for naming playlists, confirming deletions, etc.
- **Slash Commands** - `/playlist`, `/dirplaylist`, `/queue`, `/goto`, `/search`, `/seek`, etc.
- **Platform Integration**
  - Windows: System Media Transport Controls (lock screen, media keys)
  - Discord Rich Presence

## Installation

### From Source

**Prerequisites:**
- Rust 1.70+
- Linux: ALSA development headers (`libasound2-dev` on Debian/Ubuntu)

```bash
git clone https://github.com/pcnate/oxidio.git
cd oxidio
cargo build --release
```

The binary will be at `target/release/oxidio`.

### Windows Installer

Download the latest installer from [Releases](https://github.com/pcnate/oxidio/releases).

## Usage

```bash
# Launch with file browser
oxidio --browse

# Open a directory
oxidio --path /path/to/music

# Play specific files
oxidio track1.mp3 track2.flac

# Play all audio in a directory
oxidio /path/to/music/
```

## Keyboard Shortcuts

### Global (all views)

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Next / previous view |
| `/` | Enter slash command mode |
| `q` | Quit |
| `Esc` | Return to Playlist view |

### Playback (all views)

| Key | Action |
|-----|--------|
| `Space` | Play / Pause |
| `n` / `→` | Next track |
| `p` / `←` | Previous track |
| `Ctrl+→` | Seek forward 10s |
| `Ctrl+←` | Seek backward 10s |
| `+` / `=` | Volume up |
| `-` / `_` | Volume down |
| `m` | Mute / unmute |

### Playlist view

| Key | Action |
|-----|--------|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `g` / `Home` | Go to first track |
| `G` / `End` | Go to last track |
| `Enter` | Play selected track |
| `s` | Stop |
| `r` | Cycle repeat mode (Off → One → All) |
| `R` | Reload last loaded playlist |
| `S` | Toggle shuffle |
| `c` | Clear playlist |
| `e` | Toggle edit mode |
| `d` | Delete track (edit mode) |
| `Shift+J` | Move track down (edit mode) |
| `Shift+K` | Move track up (edit mode) |
| `v` | Cycle visualizer style |
| `i` | Show track info |

### Browser view

| Key | Action |
|-----|--------|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `Enter` | Enter directory / add file to playlist |
| `h` / `Backspace` | Go to parent directory |
| `a` | Add selected file or folder to playlist |
| `S` | Save selected directory as directory playlist |
| `M` | Save selected directory as M3U playlist (name prompt) |
| `R` | Refresh directory listing |
| `~` | Go to home directory |
| `g` / `Home` | Go to first entry |
| `G` / `End` | Go to last entry |

### Playlists view

| Key | Action |
|-----|--------|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `Enter` | Load selected playlist |
| `d` | Delete selected playlist |

### Help view

| Key | Action |
|-----|--------|
| `↑` / `k` | Scroll up |
| `↓` / `j` | Scroll down |
| `PgUp` / `PgDn` | Page up / down |
| `?` / `Esc` | Close help |

### Track Info view

| Key | Action |
|-----|--------|
| `i` / `Esc` | Close track info |

### Visualizer view

| Key | Action |
|-----|--------|
| `v` | Cycle visualizer style (Bars → Spectrum → Waveform → Level Meter) |
| `Esc` | Close visualizer |

### Settings view

| Key | Action |
|-----|--------|
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `Enter` | Toggle setting |
| `Esc` | Close settings |

## Slash Commands

Type `/` to enter command mode, then use any of these:

### Queue

| Command | Alias | Action |
|---------|-------|--------|
| `/queue add <path>` | `/q a` | Add file or folder to queue |
| `/queue remove` | `/q rm` | Remove selected track |
| `/queue clear` | `/q cl` | Clear queue |
| `/queue dedup` | `/q` | Remove duplicate tracks |

### Playlist

| Command | Alias | Action |
|---------|-------|--------|
| `/playlist save <name>` | `/pl save` | Save current playlist (M3U) |
| `/playlist load <name>` | `/pl load` | Load saved playlist |
| `/playlist list` | `/pl ls` | List saved playlists |
| `/playlist delete <name>` | `/pl del` | Delete saved playlist |

### Directory Playlist

| Command | Alias | Action |
|---------|-------|--------|
| `/dirplaylist save <name> [dir]` | `/dirpl save` | Save directory as playlist (`.oxidio`) |
| `/dirplaylist load <name>` | `/dirpl load` | Load & scan directory playlist |
| `/dirplaylist list` | `/dirpl ls` | List saved directory playlists |
| `/dirplaylist delete <name>` | `/dirpl del` | Delete directory playlist |

### Navigation & Playback

| Command | Action |
|---------|--------|
| `/goto <path>` | Navigate browser to path |
| `/search <term>` | Filter current view |
| `/home` | Go to home directory |
| `/seek <time>` | Seek to position (e.g., `1:30`) |
| `/shuffle` | Toggle shuffle |
| `/repeat [off\|one\|all]` | Set repeat mode |
| `/vol [0-100]` | Set or show volume |
| `/reload` | Reload last loaded playlist |
| `/vis` | Toggle visualizer |
| `/help` | Show help |
| `/quit` | Quit application |

## Views

Press **Tab** / **Shift+Tab** to cycle through views:

```
Playlist → Browser → Playlists → Track Info → Visualizer → Settings
```

## Configuration

Settings are stored at:
- Linux/macOS: `~/.config/oxidio/settings.json`
- Windows: `%APPDATA%\oxidio\settings.json`

Playlists are stored at:
- Linux/macOS: `~/.local/share/oxidio/playlists/`
- macOS: `~/Library/Application Support/oxidio/playlists/`
- Windows: `Music/Oxidio/`

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
