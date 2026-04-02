# 🖼️ a r t a

**agent runtime terminal application**

A terminal workspace manager for AI coding agents. Organize projects in a sidebar, manage multiple tmux-backed sessions, and switch between them — all from a single binary.

> **Status:** Early prototype. Rewritten in Rust with [Ratatui](https://ratatui.rs/) for low-latency terminal embedding.

```
 ┌────────────────┬─────────────────────────────────────────────┐
 │   🖼 arta      │  $ ~/projects/my-app                        │
 │ -------------- │  > claude                                   │
 │ ▼ my-app (2)   │  ╭─────────────────────────────────────╮    │
 │   session-1    │  │ claude> fix the login bug           │    │
 │   session-2 *  │  │ ...                                 │    │
 │ ▶ api-svc      │  ╰─────────────────────────────────────╯    │
 │ ▼ fe-web1 (1)  │  $ npm test                                 │
 │   session-1 *  │  ✓ 42 tests passed                          │
 │                │                                             │
 │                │  $ █                                        │
 │ ---------------│                                             │
 │ a add  D rm    │                                             │
 │ n session      │                                             │
 │ r rename       │                                             │
 │ q quit         │ [] 1 claude * - 2 zsh       2026-04-02 18:27│
 └────────────────┴─────────────────────────────────────────────┘
      sidebar          tmux session (full PTY)
```

## Features

- **Project sidebar** — add, rename, reorder, and remove projects
- **Tmux-backed sessions** — each session is an isolated tmux session with "claude" and "terminal" windows
- **Session persistence** — quit ARTA, sessions keep running. Reopen and reattach instantly
- **Bell detection** — detects BEL character inline from PTY output, shows indicator + plays sound
- **Nerd Font detection** — auto-detects and uses icons when available, falls back to Unicode
- **Full-width input panel** — tab-complete paths, browse directories, rename with full cursor support
- **Single binary** — built with [Ratatui](https://ratatui.rs/) + [tui-term](https://github.com/a-kenji/tui-term) + [portable-pty](https://docs.rs/portable-pty)

## Install

### From source

```bash
brew install tmux     # if not already installed
# Install Rust: https://rustup.rs/
git clone https://github.com/catalinj/arta.git
cd arta
cargo build --release
```

The binary is at `target/release/arta`. Add it to your PATH or copy it somewhere convenient.

### Requirements

- macOS or Linux
- Rust 1.70+ (via [rustup](https://rustup.rs/))
- tmux
- A terminal emulator (iTerm2, Ghostty, Kitty, etc.)
- Optional: a [Nerd Font](https://www.nerdfonts.com/) for icons
- Optional: disable macOS Ctrl+Space shortcut (System Settings → Keyboard → Keyboard Shortcuts → Input Sources)

## Usage

```bash
arta
```

### Sidebar keybindings

| Key | Action |
|-----|--------|
| `a` | Add project |
| `D` | Remove project (+ kills all sessions) |
| `n` | New session |
| `d` | Close session (kills tmux) |
| `r` | Rename project or session |
| `J` / `K` | Reorder project or session |
| `j` / `k` | Navigate up/down |
| `tab` | Expand/collapse project |
| `enter` | Select session |
| `l` | Focus terminal |
| `q` | Quit (sessions survive) |
| `Q` | Clean exit (kill all sessions) |

### Prefix key (`Ctrl+Space`)

`Ctrl+Space` is the prefix key. Press it, then press a command:

| After `Ctrl+Space` | Action |
|-----|--------|
| `Left` | Focus sidebar |
| `Right` | Focus terminal |
| Any sidebar key | Executes that command (e.g., `a`, `n`, `d`, `q`) |

When the sidebar is focused, all keys work directly without the prefix. The prefix is how you access sidebar commands from the terminal.

You can also click on either pane to switch focus.

Note: `Ctrl+Space` requires disabling macOS input source switching (see Requirements).

### Inside the terminal

Each session is a tmux session with two windows:

- **claude** — launches `claude` (Claude Code) automatically
- **terminal** — a plain shell

Switch between them with your tmux prefix (e.g., `Ctrl+A 1` / `Ctrl+A 2`).

### Input panel

When adding a project or renaming:

| Key | Action |
|-----|--------|
| `Tab` | Autocomplete path |
| `Up` / `Down` | Browse directory listing |
| `Enter` | Confirm / descend into directory |
| `Esc` | Cancel |
| `Left` / `Right` | Move cursor in text |

## Architecture

```
ARTA (Rust binary)
├── Sidebar (ratatui widget)
│   └── Project & session management
├── Input Panel (ratatui widget)
│   └── Full-width bottom panel for path/rename input
├── Terminal Panes (one per session, via portable-pty + tui-term)
│   ├── vt100::Parser processes PTY output on background thread
│   └── tui-term::PseudoTerminal renders to ratatui buffer
├── tmux (persistence + isolation layer)
│   └── One tmux session per ARTA session
│       ├── Window "claude" → auto-launches claude
│       └── Window "terminal" → plain shell
└── workspace.json (~/.config/arta/data/)
    └── Projects & sessions state
```

## How it works

- The **sidebar** manages projects and sessions in `~/.config/arta/data/workspace.json`
- Each **session** maps to a tmux session named `arta_<project>-<n>`
- Each session has its own **PTY** (via `portable-pty`) attached to its tmux session
- A background **reader thread** per session feeds PTY output into a `vt100::Parser`
- **Switching sessions** just changes which parser's screen is rendered — instant, no teardown
- **Bell detection** is inline: when BEL (0x07) appears in PTY output, the reader thread notifies the main thread
- **Key input** is written directly to the active PTY's master fd — no message queue, minimal latency
- When you **quit** (`q`), tmux sessions stay alive — reopen ARTA to reattach
- When you **clean exit** (`Q`), all `arta_*` tmux sessions are killed

## Known Issues

### Ctrl+Space captured by macOS
Disable in System Settings → Keyboard → Keyboard Shortcuts → Input Sources.

## Platforms

| Platform | Status |
|----------|--------|
| macOS | Works |
| Linux | Works |
| Windows + WSL | Works (it's Linux) |
| Windows native | Not supported (no tmux) |

## Documentation

- [Architecture Notes](docs/architecture.md) — design decisions and known issues
- [Implementation Log](docs/implementation-log.md) — bugs, fixes, and learnings

## License

MIT
