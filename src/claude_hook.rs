use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Sentinel embedded in the hook command so we can detect-and-skip when re-applying.
const MARKER_TAG: &str = "# arta-notify";

/// Hook command: self-discover the multiplexer session name (tmux or zellij) and
/// touch the corresponding marker file under `~/.local/share/arta/bells/`.
/// Only fires for sessions whose name starts with `arta_` — invoking claude in
/// an unrelated tmux/zellij session (or outside any multiplexer) is a silent no-op.
const HOOK_COMMAND: &str = concat!(
    "S=; ",
    r#"if [ -n "$TMUX" ]; then S=$(tmux display-message -p '#S' 2>/dev/null); "#,
    r#"elif [ -n "$ZELLIJ_SESSION_NAME" ]; then S="$ZELLIJ_SESSION_NAME"; fi; "#,
    r#"case "$S" in arta_*) mkdir -p "$HOME/.local/share/arta/bells" && touch "$HOME/.local/share/arta/bells/$S" ;; esac  "#,
    "# arta-notify"
);

/// Path to the user-scope claude-code settings file.
fn user_settings_path() -> PathBuf {
    let mut p = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    p.push(".claude/settings.json");
    p
}

/// Ensures `~/.claude/settings.json` has a `Notification` hook that writes to
/// `$ARTA_BELL_MARKER`. Idempotent: detects an existing arta entry via
/// `MARKER_TAG` in the command string.
///
/// Merge-aware: preserves the user's other hooks and top-level keys.
/// Silently skips on malformed JSON (logged via Err) rather than clobbering.
pub fn ensure_user_notify_hook() -> io::Result<()> {
    ensure_notify_hook_at(&user_settings_path())
}

/// Test-oriented entry point — writes to an arbitrary settings.json path.
pub fn ensure_notify_hook_at(settings_path: &Path) -> io::Result<()> {
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut settings: Value = match fs::read_to_string(settings_path) {
        Ok(contents) if !contents.trim().is_empty() => serde_json::from_str(&contents)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
        _ => json!({}),
    };

    if !settings.is_object() {
        // Unknown structure — don't clobber the user's file.
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "settings.local.json is not a JSON object",
        ));
    }

    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks".to_string())
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "hooks is not a JSON object",
        ));
    }

    let notif = hooks
        .as_object_mut()
        .unwrap()
        .entry("Notification".to_string())
        .or_insert_with(|| json!([]));
    if !notif.is_array() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "hooks.Notification is not an array",
        ));
    }

    let arr = notif.as_array_mut().unwrap();
    let arta_pos = arr.iter().position(entry_is_arta);
    let new_entry = json!({
        "matcher": ".*",
        "hooks": [{
            "type": "command",
            "command": HOOK_COMMAND,
        }]
    });

    match arta_pos {
        Some(i) => {
            if arr[i] == new_entry {
                // Already up to date — no write.
                return Ok(());
            }
            arr[i] = new_entry;
        }
        None => arr.push(new_entry),
    }

    let pretty = serde_json::to_string_pretty(&settings)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    fs::write(settings_path, pretty + "\n")?;
    Ok(())
}

fn entry_is_arta(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .map(|c| c.contains(MARKER_TAG))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn tempdir() -> std::path::PathBuf {
        let base = env::temp_dir().join(format!(
            "arta-hook-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn creates_file_and_hook_when_missing() {
        let dir = tempdir();
        let path = dir.join("settings.json");
        ensure_notify_hook_at(&path).unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains(MARKER_TAG));
        let v: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(v["hooks"]["Notification"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn idempotent_on_second_call() {
        let dir = tempdir();
        let path = dir.join("settings.json");
        ensure_notify_hook_at(&path).unwrap();
        ensure_notify_hook_at(&path).unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(
            v["hooks"]["Notification"].as_array().unwrap().len(),
            1,
            "should not duplicate on re-apply"
        );
    }

    #[test]
    fn replaces_outdated_arta_hook() {
        let dir = tempdir();
        let path = dir.join("settings.json");
        // Simulate an older arta hook (different command but same sentinel)
        let pre = json!({
            "hooks": {
                "Notification": [{
                    "matcher": ".*",
                    "hooks": [{
                        "type": "command",
                        "command": "touch /tmp/old-path  # arta-notify"
                    }]
                }]
            }
        });
        fs::write(&path, serde_json::to_string_pretty(&pre).unwrap()).unwrap();

        ensure_notify_hook_at(&path).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&contents).unwrap();
        let arr = v["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(arr.len(), 1, "should replace in place, not append");
        let cmd = arr[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, HOOK_COMMAND, "command should be updated to the current one");
    }

    #[test]
    fn preserves_existing_user_hook() {
        let dir = tempdir();
        let path = dir.join("settings.json");
        let pre = json!({
            "hooks": {
                "Notification": [{
                    "hooks": [{
                        "type": "command",
                        "command": "echo user-hook"
                    }]
                }]
            },
            "someOtherKey": "preserved"
        });
        fs::write(&path, serde_json::to_string_pretty(&pre).unwrap()).unwrap();

        ensure_notify_hook_at(&path).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&contents).unwrap();
        let arr = v["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(arr.len(), 2, "user hook + arta hook");
        assert_eq!(v["someOtherKey"], "preserved");
    }
}
