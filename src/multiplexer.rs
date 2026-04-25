use std::path::PathBuf;
use std::process::Command;

/// Directory where tmux's `alert-bell` hook deposits a marker file per session.
/// arta polls this directory to pick up bell events (see `TmuxBackend::check_bell_flags`).
pub fn bell_marker_dir() -> PathBuf {
    let mut p = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    p.push(".local/share/arta/bells");
    p
}

pub fn bell_marker_path(session: &str) -> PathBuf {
    let mut p = bell_marker_dir();
    p.push(session);
    p
}

pub trait MultiplexerBackend {
    /// The tag character for session naming ("t" or "z").
    fn tag(&self) -> &str;

    /// Create a new session with the given name in the given directory,
    /// launching the agent command in the primary window/tab.
    /// `rows` and `cols` are the PTY dimensions for accurate split sizing.
    fn create_session(&self, name: &str, dir: &str, agent_command: &str, rows: u16, cols: u16);

    /// List all multiplexer sessions whose name starts with `name_prefix`.
    fn list_sessions(&self, name_prefix: &str) -> Vec<String>;

    /// Kill a session by name.
    fn kill_session(&self, name: &str);

    /// Rename a session.
    fn rename_session(&self, old: &str, new: &str);

    /// Apply bell/notification settings to an existing session.
    fn apply_bell_settings(&self, name: &str);

    /// Check bell/notification flags across all sessions matching `name_prefix`.
    /// Returns `(session_full_name, has_bell)` pairs.
    fn check_bell_flags(&self, name_prefix: &str) -> Vec<(String, bool)>;

    /// The command + args to attach to a session (for PTY spawning).
    fn attach_command(&self, session_name: &str) -> (String, Vec<String>);

    /// Optional setup to run after the PTY connects to a new session.
    /// Called on a background thread. Default: no-op.
    fn post_attach_setup(&self, _name: &str, _dir: &str, _agent_command: &str, _rows: u16) {}

    /// Query the current OSC title of the agent pane (the top pane where the
    /// coding agent runs) in the given multiplexer session. Returns `None` if
    /// unavailable — pane gone, multiplexer not responding, no title set yet.
    /// Called from the App's poll loop to drive thread auto-renaming.
    fn agent_pane_title(&self, session_name: &str) -> Option<String>;
}

// ---------- Tmux ----------

pub struct TmuxBackend;

impl MultiplexerBackend for TmuxBackend {
    fn tag(&self) -> &str {
        "t"
    }

    fn create_session(&self, name: &str, dir: &str, agent_command: &str, rows: u16, cols: u16) {
        // Single window with top/bottom split: agent (top 75%) + terminal (bottom 25%).
        // Create at the exact PTY dimensions so the split ratio is correct
        // from the start (before the PTY attaches and resizes the session).
        let rows_str = rows.to_string();
        let cols_str = cols.to_string();
        // ARTA_BELL_MARKER lets the claude-code Notification hook write to the
        // correct per-session marker file. See claude_hook::ensure_notify_hook.
        let marker_env = format!("ARTA_BELL_MARKER={}", bell_marker_path(name).display());
        let _ = Command::new("tmux")
            .args([
                "new-session", "-d", "-s", name,
                "-x", &cols_str, "-y", &rows_str,
                "-c", dir,
                "-e", &marker_env,
            ])
            .output();
        // Also apply to the session env so any later-spawned processes see it.
        let _ = Command::new("tmux")
            .args(["set-environment", "-t", name, "ARTA_BELL_MARKER",
                   &bell_marker_path(name).display().to_string()])
            .output();
        // Split top/bottom — the new (bottom) pane gets 25%.
        // Minimum 3 rows for the bottom pane; skip the split entirely if the
        // window is too small to fit both panes (need at least 3+1+3 = 7 rows).
        let bottom = (rows / 4).max(3);
        if rows >= 7 {
            let _ = Command::new("tmux")
                .args([
                    "split-window", "-v", "-t", name, "-l", &bottom.to_string(), "-c", dir,
                ])
                .output();
        }
        // After split, the bottom pane is active. Name it, then select the
        // top pane, name it, and send the agent command.
        let _ = Command::new("tmux")
            .args(["select-pane", "-t", name, "-T", "terminal"])
            .output();
        let _ = Command::new("tmux")
            .args(["select-pane", "-t", name, "-U"])
            .output();
        let _ = Command::new("tmux")
            .args(["select-pane", "-t", name, "-T", "agent"])
            .output();
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", name, agent_command, "Enter"])
            .output();
        let _ = Command::new("tmux")
            .args(["set-option", "-t", name, "mouse", "on"])
            .output();
        let _ = Command::new("tmux")
            .args(["set-option", "-t", name, "monitor-activity", "on"])
            .output();
        self.apply_bell_settings(name);
    }

    fn list_sessions(&self, name_prefix: &str) -> Vec<String> {
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_name}"])
            .output();
        match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|line| line.starts_with(name_prefix))
                .map(|s| s.to_string())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn kill_session(&self, name: &str) {
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", name])
            .output();
    }

    fn rename_session(&self, old: &str, new: &str) {
        let _ = Command::new("tmux")
            .args(["rename-session", "-t", old, new])
            .output();
    }

    fn apply_bell_settings(&self, name: &str) {
        let _ = Command::new("tmux")
            .args(["set-window-option", "-t", name, "monitor-bell", "on"])
            .output();
        let _ = Command::new("tmux")
            .args(["set-option", "-t", name, "bell-action", "any"])
            .output();
        let _ = Command::new("tmux")
            .args(["set-option", "-t", name, "visual-bell", "off"])
            .output();

        // alert-bell hook: marker file is edge-triggered and consumed by arta's poll.
        // Reliable even when the belling window is current in an attached client —
        // unlike window_bell_flag, which tmux clears before our 500ms poll samples it.
        let _ = std::fs::create_dir_all(bell_marker_dir());
        let marker = bell_marker_path(name);
        let hook_cmd = format!("run-shell \"touch '{}'\"", marker.display());
        let _ = Command::new("tmux")
            .args(["set-hook", "-t", name, "alert-bell", &hook_cmd])
            .output();
    }

    fn check_bell_flags(&self, name_prefix: &str) -> Vec<(String, bool)> {
        // Read marker files deposited by the `alert-bell` hook. Each entry is
        // edge-triggered: we consume it by deleting the file, so the caller
        // sees exactly one event per bell.
        let entries = match std::fs::read_dir(bell_marker_dir()) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(name_prefix) {
                let _ = std::fs::remove_file(entry.path());
                out.push((name, true));
            }
        }
        out
    }

    fn attach_command(&self, session_name: &str) -> (String, Vec<String>) {
        (
            "tmux".to_string(),
            vec![
                "attach-session".to_string(),
                "-t".to_string(),
                session_name.to_string(),
            ],
        )
    }

    fn agent_pane_title(&self, session_name: &str) -> Option<String> {
        // The agent pane is the top pane (the bottom split is added after).
        // `-f '#{pane_at_top}'` filters list-panes to top panes only — this is
        // robust against `pane-base-index` differences across user configs.
        let out = Command::new("tmux")
            .args([
                "list-panes",
                "-t",
                session_name,
                "-f",
                "#{pane_at_top}",
                "-F",
                "#{pane_title}",
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let title = stdout.lines().next()?.trim().to_string();
        if title.is_empty() {
            None
        } else {
            Some(title)
        }
    }
}

// ---------- Zellij ----------

pub struct ZellijBackend;

impl MultiplexerBackend for ZellijBackend {
    fn tag(&self) -> &str {
        "z"
    }

    fn create_session(&self, name: &str, _dir: &str, _agent_command: &str, _rows: u16, _cols: u16) {
        // Zellij actions don't work on detached sessions (no connected client).
        // Session setup (dismiss About popup, split, send agent command) is
        // done after the PTY connects — see ZellijBackend::setup_new_session().
        let _ = name;
    }

    fn list_sessions(&self, name_prefix: &str) -> Vec<String> {
        let output = Command::new("zellij")
            .args(["list-sessions", "--short"])
            .output();
        match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|line| line.starts_with(name_prefix))
                .map(|s| s.to_string())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn kill_session(&self, name: &str) {
        let _ = Command::new("zellij")
            .args(["kill-session", name])
            .output();
    }

    fn rename_session(&self, old: &str, new: &str) {
        // Zellij doesn't support renaming sessions directly.
        // Best effort: kill old, but caller should recreate.
        let _ = Command::new("zellij")
            .args(["kill-session", old])
            .output();
        // The caller is responsible for creating the new session if needed.
        let _ = new; // suppress unused warning
    }

    fn apply_bell_settings(&self, _name: &str) {
        // Zellij handles bell natively; no extra settings needed.
    }

    fn check_bell_flags(&self, _name_prefix: &str) -> Vec<(String, bool)> {
        // Zellij doesn't expose bell flags via CLI.
        // PTY-based BEL detection in terminal_pane.rs handles this instead.
        Vec::new()
    }

    fn attach_command(&self, session_name: &str) -> (String, Vec<String>) {
        (
            "zellij".to_string(),
            vec![
                "attach".to_string(),
                "--create".to_string(),
                session_name.to_string(),
            ],
        )
    }

    fn post_attach_setup(&self, name: &str, dir: &str, agent_command: &str, rows: u16) {
        let zj = |args: &[&str]| {
            let mut full_args = vec!["-s", name];
            full_args.extend_from_slice(args);
            let _ = Command::new("zellij").args(&full_args).output();
        };

        // Marker path for claude-code's Notification hook (see claude_hook module).
        let marker = bell_marker_path(name);

        // Dismiss the "About Zellij" startup floating pane
        zj(&["action", "close-pane"]);
        // cd + export ARTA_BELL_MARKER + launch agent in the main pane
        zj(&["action", "write-chars", &format!(
            "cd {} && export ARTA_BELL_MARKER={} && clear",
            dir,
            marker.display()
        )]);
        zj(&["action", "write", "10"]);
        zj(&["action", "write-chars", agent_command]);
        zj(&["action", "write", "10"]);
        // Split: bottom pane for terminal (gets focus after creation)
        zj(&["action", "new-pane", "--direction", "down", "--name", "terminal"]);
        // cd + clear the bottom pane
        zj(&["action", "write-chars", &format!("cd {} && clear", dir)]);
        zj(&["action", "write", "10"]);
        // Resize top pane to 75%: focus it, then increase downward
        zj(&["action", "focus-previous-pane"]);
        let steps = (rows / 4).max(1);
        for _ in 0..steps {
            zj(&["action", "resize", "increase", "down"]);
        }
        // Name the agent pane so we can identify it in `list-panes --json` later
        // for OSC-title polling. We read the *title* (OSC) field, not the *name*
        // — naming is just a stable identifier. Done last so focus is on the top pane.
        zj(&["action", "rename-pane", "agent"]);
    }

    fn agent_pane_title(&self, session_name: &str) -> Option<String> {
        // `zellij action list-panes --json` returns an array of pane objects.
        // Field shapes vary by zellij version, so we walk the JSON defensively:
        // find the entry whose name is "agent" (set in post_attach_setup) and
        // return its title field.
        let out = Command::new("zellij")
            .args(["-s", session_name, "action", "list-panes", "--json"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let value: serde_json::Value = serde_json::from_str(&stdout).ok()?;
        find_agent_pane_title(&value)
    }
}

/// Walk a parsed `zellij action list-panes --json` value and return the title
/// of the pane named "agent". Field names are matched case-insensitively to
/// tolerate zellij version differences in the JSON shape.
fn find_agent_pane_title(value: &serde_json::Value) -> Option<String> {
    fn name_field(obj: &serde_json::Map<String, serde_json::Value>) -> Option<&str> {
        for k in ["name", "Name", "pane_name", "PaneName"] {
            if let Some(v) = obj.get(k).and_then(|v| v.as_str()) {
                return Some(v);
            }
        }
        None
    }
    fn title_field(obj: &serde_json::Map<String, serde_json::Value>) -> Option<&str> {
        for k in ["title", "Title", "pane_title", "PaneTitle", "TITLE"] {
            if let Some(v) = obj.get(k).and_then(|v| v.as_str()) {
                return Some(v);
            }
        }
        None
    }

    let mut stack: Vec<&serde_json::Value> = vec![value];
    while let Some(v) = stack.pop() {
        match v {
            serde_json::Value::Object(obj) => {
                if name_field(obj) == Some("agent") {
                    if let Some(t) = title_field(obj) {
                        let trimmed = t.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_string());
                        }
                    }
                }
                for (_, child) in obj {
                    stack.push(child);
                }
            }
            serde_json::Value::Array(arr) => {
                for child in arr {
                    stack.push(child);
                }
            }
            _ => {}
        }
    }
    None
}

/// Instantiate the appropriate backend for the given multiplexer choice.
pub fn backend_for(mux: crate::config::Multiplexer) -> Box<dyn MultiplexerBackend + Send> {
    match mux {
        crate::config::Multiplexer::Tmux => Box::new(TmuxBackend),
        crate::config::Multiplexer::Zellij => Box::new(ZellijBackend),
    }
}
