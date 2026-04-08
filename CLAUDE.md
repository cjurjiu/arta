# ARTA - Development Guidelines

## Build & Run

```bash
cargo build                  # dev build
cargo build --release        # release build
cargo run                    # run in dev mode
cargo test                   # run unit tests
```

Binary output: `target/debug/arta` (dev) or `target/release/arta` (release).

## Project Structure

```
src/
├── main.rs           # Entry point, terminal setup, event loop
├── app.rs            # App state, focus routing, event dispatch
├── config.rs         # Config loading (YAML), env vars, session naming
├── multiplexer.rs    # MultiplexerBackend trait, TmuxBackend, ZellijBackend
├── sidebar.rs        # Sidebar widget (project/session tree)
├── terminal_pane.rs  # PTY lifecycle, vt100 parser, reader thread
├── input_panel.rs    # Text input with path completion
├── welcome.rs        # Welcome screen ASCII art
├── workspace.rs      # Project/Session persistence (YAML)
└── keys.rs           # KeyEvent → raw PTY bytes
```

## Configuration

Config root: `~/.arta/` (override with `ARTA_CONFIG_ROOT` env var).

- `~/.arta/config.yaml` — user settings
- `~/.arta/workspace.yaml` — project/session state

### config.yaml

```yaml
coding_agent_command: claude     # default command sent to new sessions
multiplexer: tmux                # tmux | zellij
multiplexer_init_script: ~       # custom script path (overrides coding_agent_command)
```

### Environment Variables

- `ARTA_CONFIG_ROOT` — config directory (default: `~/.arta/`)
- `ARTA_SESSION_PREFIX` — session name prefix for profile isolation (default: empty)

### Session Naming

Format: `arta_{prefix?}_{tag}_{session_id}` where tag is `t` (tmux) or `z` (zellij).
Examples: `arta_t_myproj-1`, `arta_work_z_myproj-1`

## Commits

- Never include "Co-Authored-By" lines in commit messages
