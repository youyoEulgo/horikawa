# Horikawa

基于 Rust 的轻量级终端音乐播放器。

![License](https://img.shields.io/badge/license-MIT-blue.svg)

> 取名自东方Project的堀川雷鼓，能力是「什么都能变成节奏」。

## 功能

- **音频播放** — 播放、暂停、跳转、音量控制、上一首/下一首
- **多格式支持** — MP3、FLAC、OGG、WAV、M4A/AAC、OPUS、WMA、AIFF、ALAC
- **频谱可视化** — 柱状图、频谱图、波形图、电平表（~30 FPS）
- **播放列表管理** — M3U 和目录型（.horikawa）播放列表，随机、循环、排序、去重
- **文件浏览器** — 浏览本地目录，添加文件/文件夹，一键保存为歌单
- **歌单视图** — 浏览、加载、删除、重命名已保存的歌单
- **弹窗对话框** — 输入弹窗命名歌单，确认弹窗防止误删
- **快捷键帮助** — `H` 键显示当前视图的快捷键
- **Vim 风格键位** — `h/j/k/l` 导航和播放控制
- **会话持久化** — 记住歌单、播放位置、音量、设置
- **斜杠命令** — `/playlist`、`/dirplaylist`、`/queue`、`/goto`、`/seek`、`/reload` 等
- **平台集成**
  - macOS：控制中心「正在播放」、媒体键
  - Linux：MPRIS D-Bus（KDE/GNOME 媒体控制）
  - Windows：系统媒体传输控制（锁屏、媒体键）
  - Discord Rich Presence

## 安装

### 从源码编译

**前置条件：**
- Rust 1.70+
- Linux：ALSA 开发头文件（Debian/Ubuntu 下的 `libasound2-dev`）

```bash
git clone https://github.com/youyoEulgo/horikawa.git
cd horikawa
cargo build --release
```

编译好的二进制在 `target/release/horikawa`。

### Windows 安装包

从 [Releases](https://github.com/youyoEulgo/horikawa/releases) 下载最新安装包。

## 使用

```bash
# 启动并打开文件浏览器
horikawa --browse

# 打开指定目录
horikawa --path /path/to/music

# 播放指定文件
horikawa track1.mp3 track2.flac

# 播放目录下所有音频
horikawa /path/to/music/
```

## 快捷键

### 全局

| 键 | 功能 |
|-----|--------|
| `Tab` / `Shift+Tab` | 下一个/上一个视图 |
| `/` | 进入斜杠命令模式 |
| `H` | 显示当前视图快捷键帮助 |
| `q` | 退出 |

### 播放控制（大多数视图可用）

| 键 | 功能 |
|-----|--------|
| `Space` | 播放/暂停 |
| `h` / `←` | 上一首 |
| `l` / `→` | 下一首 |
| `Ctrl+h` / `Ctrl+l` | 后退/前进 10 秒 |
| `Ctrl+←` / `Ctrl+→` | 后退/前进 10 秒（非 macOS） |
| `+` / `=` | 音量增大 |
| `-` / `_` | 音量减小 |
| `m` | 静音/取消静音 |

### 播放列表视图

| 键 | 功能 |
|-----|--------|
| `↑` / `k` | 上移选择 |
| `↓` / `j` | 下移选择 |
| `g` / `Home` | 跳到第一首 |
| `G` / `End` | 跳到最后一首 |
| `Enter` | 播放选中曲目 |
| `s` | 保存当前列表为 M3U（弹窗命名） |
| `S` | 切换随机播放 |
| `r` | 循环模式（关 → 单曲 → 全部） |
| `R` | 重新加载上次歌单 |
| `e` | 切换编辑模式 |
| `d` | 删除曲目（仅编辑模式） |
| `c` | 清空列表（仅编辑模式） |
| `Shift+J` / `Shift+K` | 下移/上移曲目（仅编辑模式） |
| `v` | 打开可视化器 |
| `p` | 打开歌单视图 |
| `b` | 打开浏览器视图 |
| `i` | 打开曲目信息视图 |
| `H` | 显示播放列表快捷键 |

### 浏览器视图

| 键 | 功能 |
|-----|--------|
| `↑` / `k` | 上移选择 |
| `↓` / `j` | 下移选择 |
| `l` / `Enter` / `→` | 进入目录/播放文件 |
| `h` / `Backspace` / `←` | 返回上级目录 |
| `a` | 添加选中文件或文件夹到列表 |
| `s` | 将选中目录保存为 M3U（弹窗命名） |
| `S` | 将选中目录保存为 .horikawa（弹窗命名） |
| `R` | 刷新目录列表 |
| `~` | 回到主目录 |
| `g` / `Home` | 跳到第一条 |
| `G` / `End` | 跳到最后一条 |
| `b` / `Esc` | 返回播放列表视图 |
| `H` | 显示浏览器快捷键 |

### 歌单视图

| 键 | 功能 |
|-----|--------|
| `↑` / `k` | 上移选择 |
| `↓` / `j` | 下移选择 |
| `Enter` | 加载选中歌单 |
| `d` | 删除选中歌单（确认弹窗） |
| `r` | 重命名选中歌单（输入弹窗） |
| `p` / `Esc` | 返回播放列表视图 |
| `H` | 显示歌单快捷键 |

### 曲目信息视图

| 键 | 功能 |
|-----|--------|
| `i` / `Esc` | 返回播放列表视图 |

### 可视化器视图

| 键 | 功能 |
|-----|--------|
| `s` | 切换可视化样式（柱状图 → 频谱 → 波形 → 电平表） |
| `v` / `Esc` | 返回播放列表视图 |

### 设置视图

| 键 | 功能 |
|-----|--------|
| `↑` / `k` | 上移选择 |
| `↓` / `j` | 下移选择 |
| `Enter` | 切换设置项 |
| `Esc` | 返回播放列表视图 |

### 帮助视图

| 键 | 功能 |
|-----|--------|
| `↑` / `k` | 上滚 |
| `↓` / `j` | 下滚 |
| `PgUp` / `PgDn` | 上翻页/下翻页 |
| `?` / `Esc` | 关闭帮助 |

## 视图

按 **Tab** / **Shift+Tab** 循环切换视图：

```
播放列表 → 浏览器 → 歌单 → 曲目信息 → 可视化器 → 设置
```

从播放列表视图快速跳转：`b` 浏览器、`p` 歌单、`i` 曲目信息、`v` 可视化器。

## 斜杠命令

按 `/` 进入命令模式，可使用以下命令：

### 队列

| 命令 | 别名 | 功能 |
|---------|-------|--------|
| `/queue add <路径>` | `/q a` | 将文件或目录加入队列 |
| `/queue remove` | `/q rm` | 移除选中曲目 |
| `/queue clear` | `/q cl` | 清空队列 |
| `/queue dedup` | `/q` | 移除重复曲目 |

### 播放列表

| 命令 | 别名 | 功能 |
|---------|-------|--------|
| `/playlist save <名称>` | `/pl save` | 保存当前列表为 M3U |
| `/playlist load <名称>` | `/pl load` | 加载已保存的歌单 |
| `/playlist list` | `/pl ls` | 列出已保存的歌单 |
| `/playlist delete <名称>` | `/pl del` | 删除已保存的歌单 |

### 目录歌单

| 命令 | 别名 | 功能 |
|---------|-------|--------|
| `/dirplaylist save <名称> [目录]` | `/dirpl save` | 将目录保存为歌单（`.horikawa`） |
| `/dirplaylist load <名称>` | `/dirpl load` | 加载并扫描目录歌单 |
| `/dirplaylist list` | `/dirpl ls` | 列出已保存的目录歌单 |
| `/dirplaylist delete <名称>` | `/dirpl del` | 删除目录歌单 |

### 导航与播放

| 命令 | 功能 |
|---------|--------|
| `/goto <路径>` | 浏览器跳转到路径 |
| `/search <关键词>` | 过滤当前视图 |
| `/home` | 回到主目录 |
| `/seek <时间>` | 跳转到指定位置（如 `1:30`） |
| `/shuffle` | 切换随机播放 |
| `/repeat [off\|one\|all]` | 设置循环模式 |
| `/vol [0-100]` | 设置或显示音量 |
| `/reload` | 重新加载上次歌单 |
| `/vis` | 切换可视化器 |
| `/help` | 显示帮助 |
| `/quit` | 退出应用 |

## 配置

设置存储在：
- Linux/macOS：`~/.config/horikawa/settings.json`
- Windows：`%APPDATA%\horikawa\settings.json`

歌单存储在：
- Linux：`~/.local/share/horikawa/playlists/`
- macOS：`~/Library/Application Support/horikawa/playlists/`
- Windows：`Music/Horikawa/`

```json
{
  "discord_enabled": true,
  "smtc_enabled": true
}
```

## 编译

### 本地编译

```bash
cargo build --release
```

### 交叉编译 Windows（从 Linux）

```bash
cargo build --release --target x86_64-pc-windows-gnu
```

## 许可证

MIT
