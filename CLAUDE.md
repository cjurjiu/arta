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
├── sidebar.rs        # Sidebar widget (project/session tree)
├── terminal_pane.rs  # PTY lifecycle, vt100 parser, reader thread
├── input_panel.rs    # Text input with path completion
├── welcome.rs        # Welcome screen ASCII art
├── workspace.rs      # Project/Session persistence (JSON)
├── tmux.rs           # tmux command helpers
└── keys.rs           # KeyEvent → raw PTY bytes
```

## Commits

- Never include "Co-Authored-By" lines in commit messages
