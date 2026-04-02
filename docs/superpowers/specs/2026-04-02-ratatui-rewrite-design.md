# ARTA Ratatui Rewrite — Design Spec

## Context

ARTA is a terminal workspace manager for AI coding agents. The current implementation uses Go + BubbleTea + TUIOS + tmux. BubbleTea's serialized event loop causes unavoidable input lag when forwarding keystrokes to an embedded PTY, and TUIOS has reliability issues (mode conflicts, limited workspaces, mouse drag passthrough broken, session switching requires destroy/recreate). This rewrite moves to Rust + Ratatui to eliminate these architectural limitations while maintaining 1:1 feature parity.

## Decisions

- **Language/framework:** Rust + Ratatui + crossterm
- **PTY embedding:** `tui-term` widget + `vt100` parser + `portable-pty`
- **Session backend:** tmux (unchanged — persistence, isolation, bell support)
- **Async runtime:** None. Plain `std::thread` + `std::sync::mpsc`
- **Repo layout:** Replace Go code in-place on a new branch
- **Workspace format:** Same `~/.config/arta/data/workspace.json` (compatible with Go version)

## Architecture

```
ARTA (Rust binary)
├── App (main event loop + focus routing)
│   ├── Sidebar (ratatui widget)
│   │   └── Project & session tree, keyboard nav, mouse clicks
│   ├── TerminalPane (per-session, created eagerly on startup)
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

### Key architectural difference from Go version

Instead of a single TUIOS window that gets destroyed/recreated on session switch, we maintain a `HashMap<String, TerminalPane>`. Each session has its own PTY + vt100::Parser running continuously. Switching sessions changes which parser's screen is rendered — no teardown, no 200ms delay, no "tmux attach" command typed into the wrong app.

### Threading model

- **Main thread:** crossterm event poll → dispatch to focused widget → render
- **Per-session reader thread:** blocking `read()` on PTY master → feed bytes into `Arc<Mutex<vt100::Parser>>` → if BEL (0x07) detected, send notification via `mpsc::Sender`
- **Communication:** `mpsc::channel` carries bell notifications + session death signals from reader threads to main thread

### Input path (latency fix)

When the terminal pane is focused, crossterm key events are converted to raw bytes and written directly to the active session's PTY master fd. This happens synchronously in the event handler — no message queue, no frame delay. The vt100::Parser on the reader thread picks up the echoed output and the next render frame displays it.

Contrast with BubbleTea: keystroke → event loop queue → model.Update() → SendInput() → PTY → echo → PTYDataMsg → next frame render. At least 2 frame delays.

Ratatui: keystroke → write to PTY fd → PTY → echo → reader thread updates parser → next render reads parser screen. At most 1 frame delay, and the write itself is zero-delay.

## Startup flow

1. Load `~/.config/arta/data/workspace.json`
2. Run `tmux list-sessions -F "#{session_name}"` to get live sessions
3. Prune any workspace sessions whose tmux session no longer exists
4. Save pruned workspace
5. For every surviving session, create a `TerminalPane`:
   - Spawn PTY via `portable-pty` with size matching the terminal pane area
   - Run `tmux attach-session -t arta_<session_id>` in the PTY
   - Start reader thread feeding `vt100::Parser`
6. Enter main event loop, render sidebar + welcome screen (or last-active session)

**Runtime session creation** follows the same path: `tmux::create_session()` → `TerminalPane::new()` → insert into `panes` HashMap → set as active. Same code path as startup attachment, just triggered by sidebar action instead of startup pruning.

## Module structure

```
src/
├── main.rs           # Entry point, terminal setup/teardown, run loop
├── app.rs            # App struct, focus enum, Update/View dispatch
├── sidebar.rs        # Sidebar widget: project/session tree
├── terminal_pane.rs  # TerminalPane: PTY lifecycle, reader thread, bell detection
├── input_panel.rs    # InputPanel: text input, path completion, directory browser
├── welcome.rs        # Welcome screen widget (ASCII art + instructions)
├── workspace.rs      # Project, Session, Workspace structs + JSON serde
├── tmux.rs           # tmux command helpers (create, kill, list, rename, exists)
└── keys.rs           # crossterm::KeyEvent → raw bytes for PTY
```

## Components

### App (`app.rs`)

Top-level state and routing.

```rust
enum Focus { Sidebar, Terminal, Input }
enum InputPurpose { ProjectPath, ProjectName, RenameProject, RenameSession, ConfirmClose, ConfirmRemove }

struct App {
    sidebar: Sidebar,
    panes: HashMap<String, TerminalPane>,  // session_id → pane
    input_panel: InputPanel,
    workspace: Workspace,
    focus: Focus,
    active_session: Option<String>,
    input_purpose: Option<InputPurpose>,
    bell_rx: mpsc::Receiver<BellEvent>,
    bell_tx: mpsc::Sender<BellEvent>,  // cloned into each TerminalPane
}
```

Main loop:
1. `crossterm::event::poll(Duration::from_millis(16))` — ~60fps render rate
2. If event available: `crossterm::event::read()` → dispatch based on `self.focus`
3. Check `bell_rx.try_recv()` for bell/death notifications
4. Render: sidebar | separator | active terminal pane (or welcome screen), plus input panel if active

### Sidebar (`sidebar.rs`)

Port of the Go sidebar. Same visual design, same keybindings.

State:
- `items: Vec<SidebarItem>` — flattened visible items (projects + expanded sessions)
- `cursor: usize`
- `expanded: HashSet<String>`
- `selected: Option<String>` — active session ID
- `attention: HashSet<String>` — sessions with pending bells
- `nerd_font: bool`
- `focused: bool`

Returns `SidebarAction` enum from key/mouse handling (instead of BubbleTea Cmd/Msg pattern):
```rust
enum SidebarAction {
    None,
    SelectSession(String),
    NewSession(String),       // project name
    CloseSession(String),
    AddProject,
    RemoveProject(String),
    RenameProject(String),
    RenameSession(String),
    MoveProject(i32),
    MoveSession(String, i32),
    FocusTerminal,
    Quit,
    CleanExit,
}
```

### TerminalPane (`terminal_pane.rs`)

One per session. Manages the PTY + parser + reader thread.

```rust
struct TerminalPane {
    parser: Arc<Mutex<vt100::Parser>>,
    pty_master: Box<dyn MasterPty + Send>,  // from portable-pty
    reader_handle: Option<JoinHandle<()>>,
    session_id: String,
    alive: Arc<AtomicBool>,
}
```

Key operations:
- `new(session_id, tmux_name, size, bell_tx)` — spawn PTY, start reader thread
- `write_input(&mut self, bytes: &[u8])` — write to PTY master (for key forwarding)
- `render(&self, area: Rect, buf: &mut Buffer)` — lock parser, render via tui-term widget
- `resize(&mut self, rows: u16, cols: u16)` — resize PTY + update parser size
- `kill(&mut self)` — signal reader thread to stop, close PTY

Reader thread pseudocode:
```
loop {
    match pty_reader.read(&mut buf) {
        Ok(0) | Err(_) => { alive.store(false); bell_tx.send(Death(session_id)); break; }
        Ok(n) => {
            let data = &buf[..n];
            if data.contains(&0x07) { bell_tx.send(Bell(session_id)); }
            parser.lock().process(data);
        }
    }
}
```

### InputPanel (`input_panel.rs`)

Port of the Go input panel. Text input with two modes:
- **ModeText:** simple text input (rename, project name, y/n confirm)
- **ModePath:** text input + directory listing below with tab completion

Uses ratatui's built-in cursor positioning. No external text input crate needed — the Go version's textinput was simple enough to reimplement.

Returns `InputAction` enum:
```rust
enum InputAction {
    None,
    Submit(String),
    Cancel,
}
```

### Workspace (`workspace.rs`)

Direct port of the Go workspace. Same JSON format for compatibility.

```rust
#[derive(Serialize, Deserialize)]
struct Project { name: String, path: String }

#[derive(Serialize, Deserialize)]
struct Session { id: String, project: String, created: String }

#[derive(Serialize, Deserialize)]
struct Workspace { projects: Vec<Project>, sessions: Vec<Session> }
```

File path: `~/.config/arta/data/workspace.json` (via `dirs::config_dir()`).

### tmux helpers (`tmux.rs`)

Direct port of the Go tmux functions, using `std::process::Command`:
- `session_exists(name: &str) -> bool`
- `create_session(name: &str, dir: &str)` — creates session with "claude" + "terminal" windows
- `kill_session(name: &str)`
- `kill_all_arta()` — kills all `arta_*` sessions
- `list_sessions() -> Vec<String>` — returns live `arta_*` session names
- `rename_session(old: &str, new: &str)`

All commands use the `arta_` prefix (same as Go version's `tmuxPrefix`).

### Key conversion (`keys.rs`)

Maps `crossterm::event::KeyEvent` to raw bytes for PTY input. Same mappings as the Go version:
- Printable chars → UTF-8 bytes
- Ctrl+letter → control character (byte 1-26)
- Enter → `\r`, Backspace → `0x7f`, Tab → `\t`, Escape → `0x1b`
- Arrow keys → ANSI escape sequences (`\x1b[A` etc.)
- F1-F12 → escape sequences
- Home, End, Delete, PgUp, PgDown → escape sequences

## Keybindings

Identical to the Go version. All sidebar keybindings, Ctrl+Space prefix for focus switching, and full key passthrough when terminal is focused.

## Features checklist (1:1 parity)

- [ ] Sidebar: project tree with expand/collapse
- [ ] Sidebar: add/remove/rename projects
- [ ] Sidebar: new/close/rename sessions
- [ ] Sidebar: reorder projects (J/K) and sessions (J/K)
- [ ] Sidebar: mouse click navigation
- [ ] Sidebar: attention indicators for bell
- [ ] Sidebar: nerd font detection + fallback icons
- [ ] Terminal pane: embedded PTY rendering via tui-term
- [ ] Terminal pane: direct key input forwarding (low latency)
- [ ] Terminal pane: mouse passthrough (clicks, scroll)
- [ ] Terminal pane: resize handling
- [ ] Input panel: text mode for names/confirms
- [ ] Input panel: path mode with directory listing + tab complete
- [ ] Session management: tmux session creation (claude + terminal windows)
- [ ] Session management: tmux session persistence across restarts
- [ ] Session management: dead session pruning on startup
- [ ] Bell detection: inline from PTY output (BEL char), no polling
- [ ] Bell detection: attention indicator in sidebar + system sound
- [ ] Ctrl+Space prefix key for focus switching
- [ ] Welcome screen with ASCII art
- [ ] Workspace persistence: ~/.config/arta/data/workspace.json
- [ ] Quit (q) — sessions survive
- [ ] Clean exit (Q) — kill all arta_* sessions

## Crate dependencies

```toml
[dependencies]
ratatui = "0.29"
crossterm = "0.28"
tui-term = "0.2"
vt100 = "0.15"
portable-pty = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
dirs = "5"
```

(Exact versions to be confirmed at implementation time.)

## Verification

1. `cargo build` — compiles without errors
2. `cargo run` — launches ARTA, shows sidebar + welcome screen
3. Add a project via `a` → input panel appears, navigate to a directory, confirm
4. Create a session via `n` → tmux session created, terminal pane shows tmux with claude running
5. Switch between sessions → instant switch, no lag, no "typing into wrong app"
6. Type in terminal pane → characters appear immediately, no perceptible lag
7. Ctrl+Space → Left/Right switches focus between sidebar and terminal
8. Background session receives bell → attention indicator appears in sidebar
9. `q` to quit → relaunch ARTA → sessions reattach, state preserved
10. `Q` to clean exit → all arta_* tmux sessions killed
