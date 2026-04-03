use std::process::Command;

pub const TMUX_PREFIX: &str = "arta_";

pub fn session_name(id: &str) -> String {
    format!("{}{}", TMUX_PREFIX, id)
}

pub fn create_session(name: &str, dir: &str) {
    let _ = Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "-n", "claude", "-c", dir])
        .output();
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", &format!("{}:claude", name), "claude", "Enter"])
        .output();
    let _ = Command::new("tmux")
        .args(["new-window", "-t", name, "-n", "terminal", "-c", dir])
        .output();
    let _ = Command::new("tmux")
        .args(["select-window", "-t", &format!("{}:claude", name)])
        .output();
    let _ = Command::new("tmux")
        .args(["set-option", "-t", name, "mouse", "on"])
        .output();
    let _ = Command::new("tmux")
        .args(["set-option", "-t", name, "monitor-activity", "on"])
        .output();
    // Window options: monitor-bell must be set per-window
    let _ = Command::new("tmux")
        .args(["set-window-option", "-t", name, "monitor-bell", "on"])
        .output();
    // Session options: bell-action and visual-bell
    let _ = Command::new("tmux")
        .args(["set-option", "-t", name, "bell-action", "any"])
        .output();
    let _ = Command::new("tmux")
        .args(["set-option", "-t", name, "visual-bell", "off"])
        .output();
}

/// Apply bell-related settings to an existing session.
/// Must be called on attach (not just create) because the user's global
/// tmux.conf may override session-level settings on restart.
pub fn apply_bell_settings(name: &str) {
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

pub fn kill_session(name: &str) {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output();
}

pub fn list_sessions() -> Vec<String> {
    let output = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|line| line.starts_with(TMUX_PREFIX))
            .map(|s| s.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

pub fn rename_session(old: &str, new: &str) {
    let _ = Command::new("tmux")
        .args(["rename-session", "-t", old, new])
        .output();
}

/// Returns session IDs (without the "arta_" prefix) that have bell flags set.
/// Queries all windows across all ARTA sessions in a single tmux call.
pub fn check_bell_flags() -> Vec<(String, bool)> {
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
                    if let Some(id) = sess_name.strip_prefix(TMUX_PREFIX) {
                        let has_bell = flag_str == "1";
                        let entry = seen.entry(id.to_string()).or_insert(false);
                        *entry = *entry || has_bell;
                    }
                }
            }
            seen.into_iter().collect()
        }
        _ => Vec::new(),
    }
}
