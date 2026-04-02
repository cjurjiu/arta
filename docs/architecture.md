# ARTA Architecture Notes

## Overview

ARTA is a terminal workspace manager for AI coding agents. It provides a sidebar for managing projects and sessions, with each session backed by a tmux session containing an embedded PTY rendered via ratatui.

## Architecture

```
ARTA (Rust binary)
├── App (main event loop + focus routing)
│   ├── Sidebar (ratatui widget)
│   │   └── Project & session tree, keyboard nav, mouse clicks
│   ├── TerminalPane (one per session)
│   │   ├── portable-pty → spawns `tmux attach-session -t <name>`
│   │   ├── vt100::Parser (fed by background reader thread)
│   │   └── tui-term::PseudoTerminal (renders parser screen)
│   ├── InputPanel (ratatui widget)
│   │   └── Text input with path completion + directory browser
│   └── WelcomeScreen (shown when no session is active)
├── Workspace (serde JSON persistence)
│   └── ~/.config/arta/data/workspace.json
└── tmux (external, unchanged)
    └── One tmux session per ARTA session
        ├── Window "claude" → auto-launches claude
        └── Window "terminal" → plain shell
```

## Threading Model

- **Main thread:** crossterm event poll → dispatch to focused widget → render (~60fps)
- **Per-session reader thread:** blocking `read()` on PTY master → feed bytes into `Arc<Mutex<vt100::Parser>>` → if BEL (0x07) detected, send notification via `mpsc::Sender`
- **Communication:** `mpsc::channel` carries bell notifications + session death signals from reader threads to main thread

No async runtime (tokio, etc.) is used. Plain `std::thread` + `std::sync::mpsc`.

## Input Path (Low Latency)

When the terminal pane is focused, crossterm key events are converted to raw bytes and written directly to the active session's PTY master fd. This happens synchronously in the event handler — no message queue, no frame delay.

## Session Switching

Instead of destroying/recreating terminal windows on switch, ARTA maintains a `HashMap<String, TerminalPane>`. Each session has its own PTY + vt100::Parser running continuously. Switching sessions just changes which parser's screen is rendered — instant, no teardown.

## Key Design Decisions

### tmux as Persistence Layer
tmux was chosen because:
- Sessions survive application crashes/restarts
- Each session is fully isolated (own windows, panes, env)
- Widely installed, well-documented

### Per-Session tmux Layout
Each session creates a tmux session with two windows:
- **"claude"** — auto-runs `claude` command on creation
- **"terminal"** — plain shell for manual commands

### Eager PTY Attachment
On startup, all surviving tmux sessions get PTYs attached immediately. This enables inline bell detection (BEL character in PTY output) without polling.

### Workspace Persistence
Projects and sessions stored in `~/.config/arta/data/workspace.json`. On startup, dead sessions (tmux sessions that no longer exist) are pruned.

### Prefix Key System
`Ctrl+Space` activates prefix mode. The next key is interpreted as a command — arrow keys for focus switching, or any sidebar keybinding (a, n, d, q, etc.). This allows accessing sidebar commands from the terminal without switching focus first.

### Nerd Font Detection
On startup, runs `fc-list` and checks for "Nerd Font" in the output. Uses icon codepoints when available, falls back to Unicode symbols.
