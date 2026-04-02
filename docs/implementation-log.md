# Implementation Log

Bugs, fixes, and technical notes for the Rust/Ratatui implementation.

## PTY Lifecycle

- `portable-pty` spawns `tmux attach-session -t <name>` in each PTY
- Reader thread blocks on `read()`, feeds data to `vt100::Parser` behind `Arc<Mutex<>>`
- On Drop, `alive` flag is set to false and reader handle is detached (not joined — joining would deadlock since the PTY master fd hasn't closed yet)
- Dropping the PTY master closes the fd, which causes the reader to get EOF and exit naturally

## Bell Detection

BEL character (0x07) is detected inline in the PTY output stream by the reader thread. When found, a `PaneEvent::Bell` is sent via `mpsc::channel` to the main thread. No polling required — every attached session is monitored continuously.

## Session ID Generation

Session IDs are `<project>-<N>` where N is the count of existing sessions for that project + 1. Note: deleting session 1 of 3 then creating a new one produces `project-3` (not a gap-filling scheme).

## Ctrl+Space as Prefix Key

macOS captures Ctrl+Space for input source switching by default. Users must disable this in System Settings → Keyboard → Keyboard Shortcuts → Input Sources.

Ctrl+Space activates prefix mode. The next keypress is interpreted as a command (sidebar keybindings or arrow keys for focus). When sidebar is focused, keys work directly without prefix.

## Focus Indicators

- Sidebar focused: existing separators (below header, above footer) turn red+bold
- Terminal focused: red+bold top/bottom border lines appear
- Unfocused pane: no visible borders (separators go dim for sidebar)
- Clicking on either pane switches focus
