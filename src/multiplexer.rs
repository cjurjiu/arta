use std::process::Command;

pub trait MultiplexerBackend {
    /// The tag character for session naming ("t" or "z").
    fn tag(&self) -> &str;

    /// Create a new session with the given name in the given directory,
    /// launching the agent command in the primary window/tab.
    fn create_session(&self, name: &str, dir: &str, agent_command: &str);

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
}

// ---------- Tmux ----------

pub struct TmuxBackend;

impl MultiplexerBackend for TmuxBackend {
    fn tag(&self) -> &str {
        "t"
    }

    fn create_session(&self, name: &str, dir: &str, agent_command: &str) {
        let _ = Command::new("tmux")
            .args(["new-session", "-d", "-s", name, "-n", agent_command, "-c", dir])
            .output();
        let _ = Command::new("tmux")
            .args([
                "send-keys",
                "-t",
                &format!("{}:{}", name, agent_command),
                agent_command,
                "Enter",
            ])
            .output();
        let _ = Command::new("tmux")
            .args(["new-window", "-t", name, "-n", "terminal", "-c", dir])
            .output();
        let _ = Command::new("tmux")
            .args([
                "select-window",
                "-t",
                &format!("{}:{}", name, agent_command),
            ])
            .output();
        let _ = Command::new("tmux")
            .args(["set-option", "-t", name, "mouse", "on"])
            .output();
        let _ = Command::new("tmux")
            .args(["set-option", "-t", name, "monitor-activity", "on"])
            .output();
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

    fn create_session(&self, name: &str, dir: &str, agent_command: &str) {
        // Create a zellij session in the background with a default layout.
        // Zellij doesn't support detached creation natively, so we use
        // `zellij run` to set up tabs similar to tmux's window approach.
        let _ = Command::new("zellij")
            .args(["--session", name])
            .current_dir(dir)
            .env("ZELLIJ_AUTO_EXIT", "true")
            .spawn()
            .and_then(|mut child| {
                // Give zellij a moment to start, then send agent command
                std::thread::sleep(std::time::Duration::from_millis(500));
                child.kill()
            });

        // Use zellij action to run the agent in the first tab
        let _ = Command::new("zellij")
            .args([
                "--session",
                name,
                "action",
                "write-chars",
                &format!("{}\n", agent_command),
            ])
            .output();

        // Create a second tab for the terminal
        let _ = Command::new("zellij")
            .args(["--session", name, "action", "new-tab"])
            .output();

        // Go back to first tab
        let _ = Command::new("zellij")
            .args(["--session", name, "action", "go-to-tab", "1"])
            .output();
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
            vec!["attach".to_string(), session_name.to_string()],
        )
    }
}

/// Instantiate the appropriate backend for the given multiplexer choice.
pub fn backend_for(mux: crate::config::Multiplexer) -> Box<dyn MultiplexerBackend> {
    match mux {
        crate::config::Multiplexer::Tmux => Box::new(TmuxBackend),
        crate::config::Multiplexer::Zellij => Box::new(ZellijBackend),
    }
}
