# ARTA

**Agent Runtime Terminal Application**

A terminal workspace manager for AI coding agents. Organize projects in a sidebar, manage multiple tmux-backed sessions, and switch between them instantly — all from a single binary.

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
- **Nerd Font detection** — auto-detects and uses icons when available, falls back to Unicode
- **Full-width input panel** — tab-complete paths, browse directories, rename with full cursor support
- **Single binary** — built on [TUIOS](https://github.com/Gaurav-Gosain/tuios) and [BubbleTea](https://github.com/charmbracelet/bubbletea), zero runtime dependencies (besides tmux)

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
export PATH="$HOME/work/projects/c-term-tuios/build/bin:$PATH"
```

### Requirements

- macOS or Linux
- Go 1.21+
- tmux
- A terminal emulator (iTerm2, Ghostty, Kitty, etc.)
- Optional: a [Nerd Font](https://www.nerdfonts.com/) for icons

## Usage

```bash
arta
```

### Sidebar keybindings

| Key | Action |
|-----|--------|
| `a` | Add project |
| `D` | Remove project |
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
| `Ctrl+Space` then `Left` | Focus sidebar |
| `Ctrl+Space` then `Right` | Focus terminal |

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
├── TUIOS (terminal multiplexer library)
│   └── Single PTY window for active session
├── tmux (persistence layer)
│   └── One tmux session per ARTA session
│       ├── Window "claude" → runs claude
│       └── Window "terminal" → plain shell
└── workspace.json (~/.config/arta/data/)
    └── Projects & sessions state
```

## How it works

- The **sidebar** manages your projects and sessions in `~/.config/arta/data/workspace.json`
- Each **session** maps to a tmux session named `arta_<project>-<n>`
- **TUIOS** provides a single terminal pane that attaches to the active tmux session
- When you **switch sessions**, ARTA destroys and recreates the terminal pane with a fresh `tmux attach`
- When you **quit** (`q`), tmux sessions stay alive. Reopen ARTA and they're still there
- When you **clean exit** (`Q`), all `arta_*` tmux sessions are killed

## License

MIT
