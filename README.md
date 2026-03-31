# ARTA

**Agent Runtime Terminal Application**

A terminal workspace manager for AI coding agents. Organize projects in a sidebar, manage multiple tmux-backed sessions, and switch between them — all from a single binary.

> **Status:** Early prototype. The sidebar, project/session management, and tmux integration work well. Terminal embedding via TUIOS has known input lag issues — see [Known Issues](#known-issues). A launcher-model rewrite (sidebar alternates with fullscreen tmux) is being considered to eliminate this.

```
┌──────────────┬─────────────────────────────────┐
│  ARTA        │                                  │
│              │  $ ~/projects/my-app             │
│ ▼ my-app (2) │  > claude                        │
│   session-1  │  ╭─────────────────────────────╮ │
│   session-2  │  │ claude> fix the login bug    │ │
│ ▶ api-svc    │  │ ...                          │ │
│              │  ╰─────────────────────────────╯ │
│              │  $ npm test                       │
│              │  ✓ 42 tests passed                │
│              │                                   │
│ a add  D rm  │  $ █                              │
│ n session    │                                   │
│ r rename     │                                   │
│ q quit       │                                   │
└──────────────┴─────────────────────────────────┘
     sidebar          tmux session (full PTY)
```

## Features

- **Project sidebar** — add, rename, reorder, and remove projects
- **Tmux-backed sessions** — each session is an isolated tmux session with "claude" and "terminal" windows
- **Session persistence** — quit ARTA, sessions keep running. Reopen and reattach instantly
- **Bell detection** — polls tmux every 15s for alerts in background sessions, shows indicator + plays sound
- **Nerd Font detection** — auto-detects and uses icons when available, falls back to Unicode
- **Full-width input panel** — tab-complete paths, browse directories, rename with full cursor support
- **Single binary** — built on [TUIOS](https://github.com/Gaurav-Gosain/tuios) and [BubbleTea](https://github.com/charmbracelet/bubbletea)

## Install

### From source

```bash
brew install go tmux  # if not already installed
git clone https://github.com/catalinj/arta.git
cd arta
go build -o build/bin/arta .
```

Add to your PATH:

```bash
export PATH="/path/to/arta/build/bin:$PATH"
```

### Requirements

- macOS or Linux
- Go 1.21+
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

### Navigation

| Key | Action |
|-----|--------|
| `Ctrl+Space` → `Left` | Focus sidebar |
| `Ctrl+Space` → `Right` | Focus terminal |

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
ARTA (Go binary)
├── Sidebar (BubbleTea component)
│   └── Project & session management
├── Input Panel (BubbleTea + bubbles/textinput)
│   └── Full-width bottom panel for path/rename input
├── TUIOS (terminal multiplexer library)
│   └── Single PTY window for active tmux session
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
- **TUIOS** provides a terminal pane that attaches to the active tmux session
- When you **switch sessions**, ARTA recreates the terminal pane with a fresh `tmux attach`
- **Bell detection** polls `tmux list-sessions` every 15s for alerts on background sessions
- When you **quit** (`q`), tmux sessions stay alive — reopen ARTA to reattach
- When you **clean exit** (`Q`), all `arta_*` tmux sessions are killed

## Known Issues

### Input lag in terminal pane
BubbleTea processes all keystrokes through its event loop before forwarding to the PTY. This causes characters to appear late or be missed. This is a fundamental limitation of embedding a terminal inside a BubbleTea application.

**Workaround:** Use tmux keybindings for navigation within sessions. The lag is most noticeable during fast typing.

**Potential fix:** Rewrite to a "launcher model" where selecting a session suspends ARTA and runs `tmux attach` fullscreen (native terminal, zero lag). Detaching returns to the ARTA sidebar.

### Mouse drag doesn't pass through to tmux
TUIOS intercepts drag events. Scrolling and clicks work. Use keyboard shortcuts for tmux pane resizing.

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

- [Architecture Notes](docs/architecture.md) — design decisions, tradeoffs, alternative approaches explored
- [Implementation Log](docs/implementation-log.md) — chronological record of bugs, fixes, and learnings

## Prior Art

ARTA was also prototyped as an Emacs application (Doom Emacs + vterm + tmux). The Emacs version has no input lag but requires Emacs installed. See `docs/architecture.md` for comparison.

Tools evaluated during design: [cmux](https://cmux.com/), [TUIOS](https://github.com/Gaurav-Gosain/tuios), [Zellij](https://zellij.dev/), [Toad](https://github.com/batrachianai/toad), [OpenCode/Crush](https://opencode.ai/).

## License

MIT
