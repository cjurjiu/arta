use std::process::Command;

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
        let _ = Command::new("tmux")
            .args([
                "new-session", "-d", "-s", name, "-x", &cols_str, "-y", &rows_str, "-c", dir,
            ])
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
    }

    fn check_bell_flags(&self, name_prefix: &str) -> Vec<(String, bool)> {
        let output = Command::new("tmux")
            .args([
                "list-windows",
                "-a",
                "-F",
                "#{session_name} #{window_bell_flag}",
            ])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let mut seen = std::collections::HashMap::new();
                for line in stdout.lines() {
                    if let Some((sess_name, flag_str)) = line.split_once(' ') {
                        if sess_name.starts_with(name_prefix) {
                            let has_bell = flag_str == "1";
                            let entry = seen.entry(sess_name.to_string()).or_insert(false);
                            *entry = *entry || has_bell;
                        }
                    }
                }
                seen.into_iter().collect()
            }
            _ => Vec::new(),
        }
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

        // Dismiss the "About Zellij" startup floating pane
        zj(&["action", "close-pane"]);
        // cd + launch agent in the main pane
        zj(&["action", "write-chars", &format!("cd {} && clear", dir)]);
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
    }
}

/// Instantiate the appropriate backend for the given multiplexer choice.
pub fn backend_for(mux: crate::config::Multiplexer) -> Box<dyn MultiplexerBackend + Send> {
    match mux {
        crate::config::Multiplexer::Tmux => Box::new(TmuxBackend),
        crate::config::Multiplexer::Zellij => Box::new(ZellijBackend),
    }
}
