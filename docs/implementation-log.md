# Implementation Log

Bugs, fixes, and technical notes.

## BubbleTea v2 API Changes
- `View()` returns `tea.View` struct, not `string`. Use `tea.NewView(s)`.
- `AltScreen` and `MouseMode` are fields on `tea.View`, not program options.
- `tea.MouseMsg` is an interface with `.Mouse()` method, not a struct.
- Escape key is `"esc"`, not `"escape"`.
- `tea.WithAltScreen()` and `tea.WithMouseCellMotion()` don't exist in v2.

### TUIOS Internal Access
- `tuios.Model` is a type alias for `internal/app.OS` — all internal fields are exported.
- `AddWindow()`, `DeleteWindow()`, `FocusWindow()` work on the public Model.
- `Windows` field gives direct access to `[]*terminal.Window`.
- `Window.SendInput([]byte)` sends raw bytes to the PTY.
- `Mode` field: 0 = WindowManagementMode, 1 = TerminalMode.
- Can't import `internal/app` directly — use integer constants.
- `SwitchWorkspace(n)` switches between workspaces 1-9.

### Input Lag Problem
**Symptom:** Characters appear one keystroke late, or are missed entirely.

**Attempted fixes:**
1. Set TUIOS to terminal mode automatically → reduced but didn't fix
2. Bypass TUIOS key handler, send directly to PTY via `SendInput` → still laggy
3. Made bell check async (goroutine + sleep instead of `tea.Tick` + sync exec) → helped but core issue remains

**Root cause:** BubbleTea's event loop is not designed for forwarding raw input to a child PTY in real-time. There's always a frame of delay.

### Session Switching Problem
**Symptom:** `tmux attach-session -t ...` command typed into Claude Code's input instead of shell.

**Fix:** Destroy and recreate the TUIOS window on every session switch. New window starts with a fresh shell. Added 200ms delay via `tea.Cmd` goroutine before sending the attach command.

### Ctrl+Space as Prefix Key
**Issue:** macOS captures Ctrl+Space for input source switching.

**Fix:** User disables in System Settings → Keyboard → Keyboard Shortcuts → Input Sources.

**Implementation:** BubbleTea sends individual key events, not chords. Built a prefix key system: first keypress sets `prefixActive=true`, next keypress checks for left/right arrow.

### Text Input
Originally used a hand-built input handler (single character at a time, no cursor movement). Replaced with Charm's `bubbles/v2/textinput` for proper cursor movement, and a full-width bottom panel for directory browsing with tab completion.

## Input Lag Reduction
Three changes to reduce input lag in the terminal pane:

1. **`tea.KeyPressMsg` instead of `tea.KeyMsg`** — BubbleTea v2's `KeyMsg` is an interface covering both presses and releases. Switching to `KeyPressMsg` avoids processing release events. Also uses typed fields (`Key.Code`, `Key.Mod`, `Key.Text`) instead of `msg.String()` string parsing, which is both faster and more correct for modified keys and non-ASCII input.

2. **FPS raised from 60 to 120** — BubbleTea's renderer is frame-driven. At 60 FPS, the render frame is ~16ms. At 120 FPS (the maximum), it's ~8ms. This halves the worst-case delay between PTY output arriving and being rendered.

3. **Minimal terminal key path** — Printable characters now take the fastest code path: `key.Text != ""` → `SendInput([]byte(key.Text))` with zero string parsing or switch statements. Only special keys (arrows, function keys, etc.) and Ctrl+letter combos go through the switch. Also added F1-F12 escape sequences that were previously missing.

**Root cause:** BubbleTea serializes all messages through `eventLoop → model.Update → render`. Echoed PTY output arrives as a `PTYDataMsg` on the next frame, not the same turn as the keystroke. The mitigations reduce overhead but can't eliminate the architectural delay.
