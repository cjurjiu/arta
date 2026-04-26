# `🖼️ a r t a`

<code><u>a</u>gent <u>r</u>untime <u>t</u>erminal <u>a</u>pplication</code>

Terminal workspace manager for concurrent AI coding agent usage. Enables parallel agent usage over a single SSH session.

Organize projects in a sidebar, run multiple threads (each backed by tmux or zellij), and switch between them — all from a single binary. Close, reopen and continue from where you left off.

```
 ┌────────────────┬─────────────────────────────────────────────┐
 │ -------------- │ ─────────────────────────────────────────── │
 │                │  $ ~/projects/my-app                        │
 │   🖼 a r t a   │  > claude                                   │
 │                │  ╭─────────────────────────────────────╮    │
 │ -------------- │  │ claude> fix the login bug           │    │
 │ ▼ my-app (2)   │  │ ...                                 │    │
 │   Fix login bug│  ╰─────────────────────────────────────╯    │
 │   Add OAuth  * │  $ npm test                                 │
 │ ▶ api-svc      │  ✓ 42 tests passed                          │
 │ ▼ fe-web1 (1)  │                                             │
 │   my-app-1   * │  $ █                                        │
 │                │ focused ─────────────────────────────────── │
 │ ---------------│               interactive | v0.2.1 | MIT   │
 │ ctrl+space run │ [] 1 claude * - 2 zsh       2026-04-25 13:46│
 │ J/K reorder    │                                             │
 └────────────────┴─────────────────────────────────────────────┘
      sidebar          tmux session (full PTY)
```

## Features

- **Project sidebar** — add, rename, reorder, and remove projects
- **Tmux or Zellij threads** — each thread runs your coding agent + a shell, backed by a tmux (default) or zellij session for persistence
- **Configurable coding agent** — defaults to `claude`, but can be set globally to `codex`, `gemini`, `opencode`, or any command (and arguments — e.g. `claude --resume`), with per-project overrides (use a different agent — or a different invocation of the same agent — on a per-project basis)
- **In-app settings** — change the global agent command, default IDE open command, or multiplexer backend from inside ARTA (`Ctrl+Space s`); no need to edit YAML by hand
- **Auto-named threads** — threads pick up their name from the agent's terminal title (e.g., `Refactoring auth module`) instead of showing generic IDs. Manual rename (`Ctrl+Space r`) pins the name and disables further auto-updates
- **Thread persistence** — quit ARTA, threads keep running. Reopen and reattach instantly
- **Thread restore** — remembers and reopens your last active thread on startup
- **Multiple profiles** — use `ARTA_CONFIG_ROOT` and `ARTA_SESSION_PREFIX` env vars to run independent ARTA instances with separate configs and threads
- **Open IDE** — press `o` to launch your configured IDE (e.g., `webstorm .`, `idea .`) for any project; falls back to a global default (`vi` out of the box) when no per-project override is set
- **Project configuration** — press `c` to configure project name, path, or open command via a navigable menu
- **Bell notifications** — focus-aware bells via a Claude Code `Notification` hook (auto-installed into `~/.claude/settings.json`); shows a sidebar indicator and plays a sound for unfocused threads, reliable across tmux and zellij
- **Nerd Font detection** — auto-detects and uses icons when available, falls back to Unicode
- **Full-width input panel** — tab-complete paths, browse directories, rename with full cursor support
- **Single binary** — built with [Ratatui](https://ratatui.rs/) + [tui-term](https://github.com/a-kenji/tui-term) + [portable-pty](https://docs.rs/portable-pty)

## Install

arta needs `tmux` (the default backend) or `zellij`. The Homebrew install pulls in `tmux` automatically; for other install methods, install one yourself — see [Requirements](#requirements).

### Homebrew (macOS, Linux)

```bash
brew install cjurjiu/arta/arta   # also installs tmux
```

### Shell installer (macOS, Linux)

```bash
curl -fsSL https://github.com/cjurjiu/arta/releases/latest/download/arta-tui-installer.sh | sh
```

### Cargo

```bash
cargo install arta-tui          # compile from crates.io (binary is `arta`)
cargo binstall arta-tui         # download prebuilt binary, no compile
```

### mise / ubi

```bash
mise use -g ubi:cjurjiu/arta@latest
```

### From source

```bash
# Install Rust: https://rustup.rs/
git clone https://github.com/cjurjiu/arta.git
cd arta
cargo build --release
```

The binary is at `target/release/arta`. Add it to your PATH or copy it somewhere convenient.

### Requirements

- macOS or Linux
- Rust 1.85+ (via [rustup](https://rustup.rs/))
- tmux (default) or [zellij](https://zellij.dev/)
- A terminal emulator (iTerm2, Ghostty, Kitty, etc.)
- Optional: a [Nerd Font](https://www.nerdfonts.com/) for icons
- Optional: disable macOS Ctrl+Space shortcut (System Settings → Keyboard → Keyboard Shortcuts → Input Sources)

## Usage

```bash
arta
```

### Prefix key (`Ctrl+Space`)

`Ctrl+Space` is the prefix key. Press it from **any focus**, then press a command. The sidebar footer shows available commands while the prefix is active.

| After `Ctrl+Space` | Action |
|-----|--------|
| `←` / `→` | Change focus (sidebar / terminal) |
| `n` | New thread |
| `o` | Open IDE (runs configured open command) |
| `r` | Rename project or thread (pins thread name, disables auto-rename) |
| `a` | Add project (path → name → open command) |
| `c` | Configure project (rename, path, agent command, open command) |
| `s` | ARTA settings (global agent command, default open command, multiplexer) |
| `d` | Delete selected item (project or thread) |
| `g` | Copy GitHub link to clipboard |
| `q` | Quit (threads survive in the background) |
| `Q` | Clean exit (kill all threads) |

The status bar shows `interactive` or `run` mode. When `Ctrl+Space` is pressed, it switches to `run` mode with "awaiting command..." until a key is pressed.

### Sidebar-only keys

These keys work only when the sidebar is focused (no prefix needed):

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `J` / `K` | Reorder project or thread |
| `tab` | Expand/collapse project |
| `enter` | Select thread |
| `l` | Focus terminal |

You can also click on either pane to switch focus.

Note: `Ctrl+Space` requires disabling macOS input source switching (see Requirements).

### Inside the terminal

Each thread is backed by a tmux (or zellij) session with two windows:

- **agent** — launches your configured coding agent (`claude` by default) automatically
- **terminal** — a plain shell

Switch between them with your tmux prefix (e.g., `Ctrl+A 1` / `Ctrl+A 2`) or zellij tab switching.

### Input panel

When adding a project or renaming:

| Key | Action |
|-----|--------|
| `Tab` | Autocomplete path |
| `Up` / `Down` | Browse directory listing |
| `Enter` | Confirm / descend into directory |
| `Esc` | Cancel |
| `Left` / `Right` | Move cursor in text |

When picking a folder, every directory listing has a pinned **`▸ Select folder`** row at the top — highlight it and press `Enter` to use the current path as the project root.

## Configuration

ARTA stores its configuration and workspace state under `~/.arta/` by default.

### In-app settings (`Ctrl+Space s`)

You can change every global setting from inside ARTA — open the **ARTA settings** menu with `Ctrl+Space s`. Each row shows the current value and a one-line description; selecting a row opens an editor (a text input for free-form values, a tmux/zellij picker for the multiplexer). Changes are persisted to `~/.arta/config.yaml` immediately. The multiplexer change requires a restart; the others apply to the next thread you create.

The multiplexer picker also detects which backend is installed on your `PATH` and dims the unavailable one with `(needs install)`.

### Config file (`~/.arta/config.yaml`)

All fields are optional — ARTA uses sensible defaults if the file is missing or incomplete. You can edit by hand or use the in-app settings menu above.

```yaml
# The command launched in the agent window of new threads (default: "claude").
# Arguments are allowed — e.g. "claude --resume" to auto-resume the last
# session. This is the global default; individual projects can override it
# via Ctrl+Space c → Agent command.
coding_agent_command: claude

# The default command for "open ide" (Ctrl+Space o) when a project has no
# per-project override. Defaults to "vi" (universal across macOS/Linux).
# Common picks: "code .", "webstorm .", "idea .", "open ." (macOS Finder).
# Set to an empty string to disable the global fallback.
default_open_command: vi

# Terminal multiplexer backend: "tmux" (default) or "zellij"
multiplexer: tmux
```

### Per-project agent command

Each project can override the global `coding_agent_command`. From the sidebar,
select a project and press `Ctrl+Space c` → **Agent command**. Examples:

- `claude` — plain Claude Code
- `codex` — OpenAI Codex CLI
- `gemini`, `opencode`, `aider`, … — anything you can run from a shell

Submit an empty value to clear the override and fall back to the global. The
override is read at thread-create time; existing threads are not affected.

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ARTA_CONFIG_ROOT` | `~/.arta/` | Config directory. Set to a different path for separate profiles. |
| `ARTA_SESSION_PREFIX` | *(empty)* | Prefix added to session names. Prevents collisions between profiles. |

### Multiple profiles

Run independent ARTA instances with separate configs and threads:

```bash
# Work profile
ARTA_CONFIG_ROOT=~/.arta-work ARTA_SESSION_PREFIX=work arta

# Personal profile
ARTA_CONFIG_ROOT=~/.arta-personal ARTA_SESSION_PREFIX=personal arta
```

Each profile has its own `config.yaml`, `workspace.yaml`, and distinctly-named multiplexer sessions backing its threads (`arta_work_t_*` vs `arta_personal_t_*`).

### Migration from older versions

On first run, ARTA automatically migrates your workspace from the legacy location (`~/.config/arta/data/workspace.json`) to the new location (`~/.arta/workspace.yaml`). Existing tmux sessions are also renamed to the new naming scheme. Workspace files using the older `sessions:` / `active_session:` / `next_session_id:` keys are loaded transparently and rewritten with the new `threads:` keys on first save.

## Architecture

```
ARTA (Rust binary)
├── Config (YAML, ~/.arta/config.yaml)
│   └── Coding agent (global default + per-project overrides), multiplexer choice
├── Sidebar (ratatui widget)
│   └── Project & thread management
├── Input Panel (ratatui widget)
│   └── Full-width bottom panel for path/rename input
├── Terminal Panes (one per thread, via portable-pty + tui-term)
│   ├── vt100::Parser processes PTY output on background thread
│   └── tui-term::PseudoTerminal renders to ratatui buffer
├── Multiplexer (tmux or zellij, configurable)
│   └── One session backs each thread
│       ├── Window/tab "agent" → auto-launches coding agent
│       └── Window/tab "terminal" → plain shell
└── workspace.yaml (~/.arta/)
    └── Projects, threads, active thread & per-project config
```

Vocabulary: **thread** is the user-facing concept (what shows in the sidebar). Under the hood, each thread maps to a tmux/zellij **session** — the term is preserved everywhere ARTA talks to the multiplexer (CLI args, env vars, persistence keys for backward compat).

## How it works

- The **sidebar** manages projects and threads in `~/.arta/workspace.yaml`
- Each **thread** maps to a multiplexer session named `arta_{prefix?}_{t|z}_{project}-{n}` (e.g., `arta_t_myproj-1`); the thread's display name is independent and updates from the agent's terminal title
- Each thread has its own **PTY** (via `portable-pty`) attached to its multiplexer session
- A background **reader thread** per pane feeds PTY output into a `vt100::Parser`
- **Switching threads** just changes which parser's screen is rendered — instant, no teardown
- **Auto-rename** — every 500ms, ARTA queries the agent pane's terminal title (`tmux display-message #{pane_title}` / `zellij action list-panes`); if it changed and the user hasn't pinned the name with `Ctrl+Space r`, the sidebar updates. Generic agent labels like `Claude Code` are accepted as the *initial* name but never overwrite a more specific one (the "generic" check uses the thread's effective agent — global default or per-project override)
- **Per-project agent command** — each project can override `coding_agent_command` via `Ctrl+Space c` → Agent command. Stored on the project in `workspace.yaml`. Resolved at thread-create time only — existing threads keep their original agent
- **Bell notifications** are driven by a Claude Code `Notification` hook (merged idempotently into `~/.claude/settings.json`) that touches a marker file under `~/.local/share/arta/bells/` for the current multiplexer session; ARTA polls that directory and raises attention for unfocused threads. Raw PTY BEL is also handled as a fallback
- **Key input** is written directly to the active PTY's master fd — no message queue, minimal latency
- **Thread restore** — the last active thread is saved to `workspace.yaml` and auto-reopened on startup; falls back to the first alive thread if the saved one is gone
- When you **quit** (`q`), multiplexer sessions stay alive — reopen ARTA to reattach
- When you **clean exit** (`Q`), all tracked threads (and their backing sessions) are killed

## Known Issues

### Ctrl+Space captured by macOS
Disable in System Settings → Keyboard → Keyboard Shortcuts → Input Sources.

## Platforms

| Platform | Status |
|----------|--------|
| macOS | Works |
| Linux | Works |
| Windows + WSL | Works (it's Linux) |
| Windows native | Not supported (no tmux/zellij) |

## License

MIT. PRs are welcome.