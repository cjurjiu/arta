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
    let _ = Command::new("tmux")
        .args(["set-option", "-t", name, "monitor-bell", "on"])
        .output();
    let _ = Command::new("tmux")
        .args(["set-option", "-t", name, "bell-action", "any"])
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
