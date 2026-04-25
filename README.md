# `🖼️ a r t a`

<code><u>a</u>gent <u>r</u>untime <u>t</u>erminal <u>a</u>pplication</code>

Terminal workspace manager for concurrent AI coding agent usage. Enables parallel agent usage over a single SSH session.

Organize projects in a sidebar, manage multiple tmux or zellij-backed sessions, and switch between them — all from a single binary. Close, reopen and continue from where you left off.

```
 ┌────────────────┬─────────────────────────────────────────────┐
 │ -------------- │ ─────────────────────────────────────────── │
 │                │  $ ~/projects/my-app                        │
 │   🖼 a r t a   │  > claude                                   │
 │                │  ╭─────────────────────────────────────╮    │
 │ -------------- │  │ claude> fix the login bug           │    │
 │ ▼ my-app (2)   │  │ ...                                 │    │
 │   session-1    │  ╰─────────────────────────────────────╯    │
 │   session-2 *  │  $ npm test                                 │
 │ ▶ api-svc      │  ✓ 42 tests passed                          │
 │ ▼ fe-web1 (1)  │                                             │
 │   session-1 *  │  $ █                                        │
 │                │ focused ─────────────────────────────────── │
 │ ---------------│               interactive | v0.2.0 | MIT   │
 │ ctrl+space run │ [] 1 claude * - 2 zsh       2026-04-02 18:27│
 │ J/K reorder    │                                             │
 └────────────────┴─────────────────────────────────────────────┘
      sidebar          tmux session (full PTY)
```

## Features

- **Project sidebar** — add, rename, reorder, and remove projects
- **Tmux or Zellij sessions** — each session is backed by tmux (default) or zellij, with an agent window and a terminal window
- **Configurable coding agent** — defaults to `claude`, but can be set to `codex`, `gemini`, `opencode`, or any command
- **Session persistence** — quit ARTA, sessions keep running. Reopen and reattach instantly
- **Session restore** — remembers and reopens your last active session on startup
- **Multiple profiles** — use `ARTA_CONFIG_ROOT` and `ARTA_SESSION_PREFIX` env vars to run independent ARTA instances with separate configs and sessions
- **Open IDE** — press `o` to launch your configured IDE (e.g., `webstorm .`, `idea .`) for any project
- **Project configuration** — press `c` to configure project name, path, or open command via a navigable menu
- **Bell notifications** — focus-aware bells via a Claude Code `Notification` hook (auto-installed into `~/.claude/settings.json`); shows a sidebar indicator and plays a sound for unfocused sessions, reliable across tmux and zellij
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
curl -fsSL https://github.com/cjurjiu/arta/releases/latest/download/arta-installer.sh | sh
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
| `n` | New session |
| `o` | Open IDE (runs configured open command) |
| `r` | Rename project or session |
| `a` | Add project (path → name → open command) |
| `c` | Configure project (rename, path, open command) |
| `d` | Delete selected item (project or session) |
| `g` | Copy GitHub link to clipboard |
| `q` | Quit (sessions survive) |
| `Q` | Clean exit (kill all sessions) |

The status bar shows `interactive` or `run` mode. When `Ctrl+Space` is pressed, it switches to `run` mode with "awaiting command..." until a key is pressed.

### Sidebar-only keys

These keys work only when the sidebar is focused (no prefix needed):

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `J` / `K` | Reorder project or session |
| `tab` | Expand/collapse project |
| `enter` | Select session |
| `l` | Focus terminal |

You can also click on either pane to switch focus.

Note: `Ctrl+Space` requires disabling macOS input source switching (see Requirements).

### Inside the terminal

Each session is a tmux (or zellij) session with two windows:

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

## Configuration

ARTA stores its configuration and workspace state under `~/.arta/` by default.

### Config file (`~/.arta/config.yaml`)

All fields are optional — ARTA uses sensible defaults if the file is missing or incomplete.

```yaml
# The command launched in the agent window of new sessions (default: "claude")
coding_agent_command: claude

# Terminal multiplexer backend: "tmux" (default) or "zellij"
multiplexer: tmux

# Custom script to run instead of the default session creation.
# Receives two arguments: <session_name> <project_directory>
# When set, coding_agent_command is ignored (the script controls what runs).
# The multiplexer setting is still used for attaching to the session.
# multiplexer_init_script: /path/to/your/script.sh
```

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ARTA_CONFIG_ROOT` | `~/.arta/` | Config directory. Set to a different path for separate profiles. |
| `ARTA_SESSION_PREFIX` | *(empty)* | Prefix added to session names. Prevents collisions between profiles. |

### Multiple profiles

Run independent ARTA instances with separate configs and sessions:

```bash
# Work profile
ARTA_CONFIG_ROOT=~/.arta-work ARTA_SESSION_PREFIX=work arta

# Personal profile
ARTA_CONFIG_ROOT=~/.arta-personal ARTA_SESSION_PREFIX=personal arta
```

Each profile has its own `config.yaml`, `workspace.yaml`, and distinctly-named multiplexer sessions (`arta_work_t_*` vs `arta_personal_t_*`).

### Migration from older versions

On first run, ARTA automatically migrates your workspace from the legacy location (`~/.config/arta/data/workspace.json`) to the new location (`~/.arta/workspace.yaml`). Existing tmux sessions are also renamed to the new naming scheme.

## Architecture

```
ARTA (Rust binary)
├── Config (YAML, ~/.arta/config.yaml)
│   └── Coding agent command, multiplexer choice, init script
├── Sidebar (ratatui widget)
│   └── Project & session management
├── Input Panel (ratatui widget)
│   └── Full-width bottom panel for path/rename input
├── Terminal Panes (one per session, via portable-pty + tui-term)
│   ├── vt100::Parser processes PTY output on background thread
│   └── tui-term::PseudoTerminal renders to ratatui buffer
├── Multiplexer (tmux or zellij, configurable)
│   └── One session per ARTA session
│       ├── Window/tab "agent" → auto-launches coding agent
│       └── Window/tab "terminal" → plain shell
└── workspace.yaml (~/.arta/)
    └── Projects, sessions, active session & per-project config
```

## How it works

- The **sidebar** manages projects and sessions in `~/.arta/workspace.yaml`
- Each **session** maps to a multiplexer session named `arta_{prefix?}_{t|z}_{project}-{n}` (e.g., `arta_t_myproj-1`)
- Each session has its own **PTY** (via `portable-pty`) attached to its multiplexer session
- A background **reader thread** per session feeds PTY output into a `vt100::Parser`
- **Switching sessions** just changes which parser's screen is rendered — instant, no teardown
- **Bell notifications** are driven by a Claude Code `Notification` hook (merged idempotently into `~/.claude/settings.json`) that touches a marker file under `~/.local/share/arta/bells/` for the current multiplexer session; ARTA polls that directory and raises attention for unfocused sessions. Raw PTY BEL is also handled as a fallback.
- **Key input** is written directly to the active PTY's master fd — no message queue, minimal latency
- **Session restore** — the last active session is saved to `workspace.yaml` and auto-reopened on startup; falls back to the first alive session if the saved one is gone
- When you **quit** (`q`), multiplexer sessions stay alive — reopen ARTA to reattach
- When you **clean exit** (`Q`), all tracked sessions are killed

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