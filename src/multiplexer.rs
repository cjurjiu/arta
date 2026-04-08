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
        // Single window with top/bottom split: agent (top 75%) + terminal (bottom 25%).
        // Avoid hardcoded window/pane indices — they depend on the user's
        // base-index and pane-base-index settings.
        let _ = Command::new("tmux")
            .args(["new-session", "-d", "-s", name, "-c", dir])
            .output();
        // Split top/bottom — the new (bottom) pane gets 25%
        let _ = Command::new("tmux")
            .args(["split-window", "-v", "-t", name, "-l", "25%", "-c", dir])
            .output();
        // After split, the bottom pane is active. Select the top pane first,
        // then send the agent command to it.
        let _ = Command::new("tmux")
            .args(["select-pane", "-t", name, "-U"])
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
        // Write a temporary layout file for a vertical split: agent (75%) on top,
        // terminal (25%) on bottom.
        let layout = format!(
            r#"layout {{
    pane size="75%" command="{}" cwd="{}"
    pane size="25%" cwd="{}"
}}"#,
            agent_command, dir, dir
        );
        let layout_path = std::env::temp_dir().join(format!("arta-zellij-{}.kdl", name));
        let _ = std::fs::write(&layout_path, &layout);

        let _ = Command::new("zellij")
            .args([
                "--session",
                name,
                "--layout",
                &layout_path.display().to_string(),
            ])
            .current_dir(dir)
            .env("ZELLIJ_AUTO_EXIT", "true")
            .spawn()
            .and_then(|mut child| {
                std::thread::sleep(std::time::Duration::from_millis(500));
                child.kill()
            });

        let _ = std::fs::remove_file(&layout_path);
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
