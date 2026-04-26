# ARTA - Development Guidelines

## Build & Run

```bash
cargo build                  # dev build
cargo build --release        # release build
cargo run                    # run in dev mode
cargo test                   # run unit tests
```

Binary output: `target/debug/arta` (dev) or `target/release/arta` (release). The crate is published to crates.io as `arta-tui` (the binary stays `arta` via `[[bin]]`).

## Project Structure

```
src/
├── main.rs           # Entry point, terminal setup, event loop
├── app.rs            # App state, focus routing, event dispatch, agent-title polling
├── claude_hook.rs    # Idempotent merge of Notification hook into ~/.claude/settings.json
├── config.rs         # Config loading (YAML), env vars, session naming
├── multiplexer.rs    # MultiplexerBackend trait, TmuxBackend, ZellijBackend
├── sidebar.rs        # Sidebar widget (project/thread tree)
├── terminal_pane.rs  # PTY lifecycle, vt100 parser, reader thread
├── input_panel.rs    # Text input with path completion
├── welcome.rs        # Welcome screen ASCII art
├── workspace.rs      # Project/Thread persistence (YAML, with serde aliases for legacy `sessions:` keys)
└── keys.rs           # KeyEvent → raw PTY bytes
```

## Vocabulary

- **thread** — the user-facing unit (what's shown in the sidebar). State type is `Thread` in `workspace.rs`.
- **session** — preserved for tmux/zellij vocabulary only: CLI args, env vars (`ARTA_SESSION_PREFIX`), backing-session names (`arta_t_myproj-1`), and `MultiplexerBackend` methods.

## Configuration

Config root: `~/.arta/` (override with `ARTA_CONFIG_ROOT` env var).

- `~/.arta/config.yaml` — user settings
- `~/.arta/workspace.yaml` — project/session state

### config.yaml

```yaml
coding_agent_command: claude     # default command sent to new threads (args allowed, e.g. "claude --resume")
default_open_command: vi         # fallback for "open ide" when a project has no override; "" disables fallback
multiplexer: tmux                # tmux | zellij
```

All fields are editable in-app via `Ctrl+Space s` (ARTA settings); the menu
calls `Config::save()` which writes the full file (no comment preservation).

Per-project agent overrides (`workspace.yaml`: `agent_command:` on a project) take
precedence over the global `coding_agent_command`. Per-project `open_command`
overrides take precedence over `default_open_command`. Both are read at
thread-create / open-IDE time respectively (not cached on App). Helpers:
`effective_agent_command()` and `effective_open_command()` in `app.rs`.

Multiplexer changes require a restart — the backend is built once at startup
and stored on `App.mux`. `Multiplexer::is_installed()` walks `PATH` directly
(no `--version` subprocess; tmux's `--version` flag spelling differs).

The legacy `multiplexer_init_script` config key is no longer supported. If still
present in `config.yaml` it is silently ignored, with a startup warning surfaced
as a timed message.

### Environment Variables

- `ARTA_CONFIG_ROOT` — config directory (default: `~/.arta/`)
- `ARTA_SESSION_PREFIX` — session name prefix for profile isolation (default: empty)

### Session Naming

Format: `arta_{prefix?}_{tag}_{session_id}` where tag is `t` (tmux) or `z` (zellij).
Examples: `arta_t_myproj-1`, `arta_work_z_myproj-1`

## Commits

- Never include "Co-Authored-By" lines in commit messages
