# ARTA Architecture Notes

## Overview

ARTA is a terminal workspace manager for AI coding agents. It provides a sidebar for managing projects and sessions, with tmux as the session backend.

## Current Implementation: Go + TUIOS + tmux

```
ARTA (Go binary)
├── Sidebar (BubbleTea component)
│   └── Project & session management
├── Input Panel (BubbleTea component)
│   └── Full-width bottom panel with textinput + directory browser
├── TUIOS (terminal multiplexer library)
│   └── Single PTY window for active tmux session
├── tmux (persistence + session isolation layer)
│   └── One tmux session per ARTA session
│       ├── Window "claude" → auto-launches `claude`
│       └── Window "terminal" → plain shell
└── workspace.json (~/.config/arta/data/)
    └── Projects & sessions state
```

## Known Issues & Limitations

### Input Lag (Reduced, Not Eliminated)
BubbleTea's event loop processes all keystrokes, then we forward them to the TUIOS PTY. Echoed PTY output arrives as a `PTYDataMsg` on the next message/frame, not the same turn as the keystroke. Mitigations applied:
- Switched from `tea.KeyMsg` to `tea.KeyPressMsg` with typed field access (avoids string parsing)
- Printable characters take a fast path: `key.Text` → `SendInput()` with no switch/string matching
- Raised FPS from 60 to 120 (halves worst-case render delay)
- Bypassed TUIOS's key handler, sending directly to PTY via `SendInput`
- Made bell checker async to avoid blocking the event loop

**Root cause:** BubbleTea serializes all messages through `eventLoop → model.Update → render`. This is architectural and cannot be fully eliminated without leaving BubbleTea's event loop (e.g., launcher model).

### Mouse Drag Passthrough
tmux pane resizing via mouse drag doesn't work. TUIOS intercepts drag events for its own window management before they reach the terminal process. Scrolling and clicks work, drags don't.

### TUIOS Mode System
TUIOS has Window Management Mode (mode=0) and Terminal Mode (mode=1). We manually set `m.tuios.Mode = 1` when the terminal is focused, but this is a hack. TUIOS's internal state management can conflict with our routing.

### Session Switching
When switching tmux sessions, we destroy and recreate the TUIOS window because `SendInput` would type into whatever app is running (e.g., Claude Code), not the shell. The new window needs a 200ms delay before sending the `tmux attach` command so the shell has time to start.

## Alternative Architectures Explored

### 1. Emacs + vterm + tmux (Implemented, Working)
**Location:** `~/.config/c-term/`

The original prototype. Uses Doom Emacs with vterm for terminal embedding and tmux for session persistence.

**Pros:**
- No input lag — vterm handles terminal I/O natively via libvterm (C library)
- Proper VT100 emulation
- Sidebar works via Elisp
- Full feature set working

**Cons:**
- Requires Emacs installed (~130MB)
- Not distributable as a single binary
- Elisp is niche
- Mouse drag passthrough to tmux doesn't work (same limitation)

### 2. Launcher Model (Not Implemented)
Drop TUIOS. Use BubbleTea only for the sidebar. When user selects a session, suspend BubbleTea and exec `tmux attach` directly. tmux gets raw terminal access. When user detaches, BubbleTea resumes.

**Pros:**
- Zero input lag (tmux runs natively)
- Full mouse support
- No TUIOS dependency
- Simplest code

**Cons:**
- Sidebar not visible alongside terminal (alternates with tmux)
- UX is a launcher/switcher, not a persistent workspace view

### 3. Fork TUIOS (Not Implemented)
Fork TUIOS and add the sidebar directly into its codebase. Avoids the BubbleTea-TUIOS integration issues since everything is one app.

**Pros:**
- Native terminal handling
- Single binary
- Full mouse support

**Cons:**
- Maintaining a fork
- Significant development effort

### 4. Zellij Plugin (Not Feasible)
Zellij has a WASM plugin system but plugins can't control the host's panes or sessions. No persistent sidebar API. No bell detection from outside.

## Key Design Decisions

### tmux as Persistence Layer
tmux was chosen because:
- Sessions survive application crashes/restarts
- Client-server architecture allows bell detection via `tmux list-sessions`
- Each session is fully isolated (own windows, panes, env)
- `monitor-bell` + `bell-action` track alerts even with no client attached
- Widely installed, well-documented

### Per-Session tmux Layout
Each session creates a tmux session with two windows:
- **"claude"** — auto-runs `claude` command on creation
- **"terminal"** — plain shell for manual commands

### Bell Polling
Every 15 seconds, ARTA runs `tmux list-sessions -F "#{session_name} #{session_alerts}"` in a background goroutine to detect bells in non-active sessions. This avoids blocking the UI.

### Workspace Persistence
Projects and sessions stored in `~/.config/arta/data/workspace.json`. On startup, dead sessions (tmux sessions that no longer exist) are pruned.

### Nerd Font Detection
On startup, runs `fc-list` and checks for "Nerd Font" in the output. Uses icon codepoints when available, falls back to Unicode symbols.
