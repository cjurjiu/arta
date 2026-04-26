use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Multiplexer {
    Tmux,
    Zellij,
}

impl Default for Multiplexer {
    fn default() -> Self {
        Multiplexer::Tmux
    }
}

impl Multiplexer {
    pub fn as_str(&self) -> &'static str {
        match self {
            Multiplexer::Tmux => "tmux",
            Multiplexer::Zellij => "zellij",
        }
    }

    /// Returns true if the underlying CLI is on PATH.
    ///
    /// We walk PATH directly instead of running `<bin> --version`: invoking
    /// `tmux --version` from inside a live tmux session can fail or hang in
    /// some setups, which would falsely report tmux as missing.
    pub fn is_installed(&self) -> bool {
        let bin = self.as_str();
        let path_env = match std::env::var_os("PATH") {
            Some(p) => p,
            None => return false,
        };
        for dir in std::env::split_paths(&path_env) {
            let candidate = dir.join(bin);
            if !candidate.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&candidate) {
                    if meta.permissions().mode() & 0o111 != 0 {
                        return true;
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return true;
            }
        }
        false
    }
}


fn default_coding_agent_command() -> String {
    "claude".to_string()
}

fn default_open_command() -> String {
    // `vi` is the only editor universally available across macOS and Linux
    // out of the box. Users typically override per-project to `code .`,
    // `webstorm .`, etc.
    "vi".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_coding_agent_command")]
    pub coding_agent_command: String,

    #[serde(default = "default_open_command")]
    pub default_open_command: String,

    #[serde(default)]
    pub multiplexer: Multiplexer,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            coding_agent_command: default_coding_agent_command(),
            default_open_command: default_open_command(),
            multiplexer: Multiplexer::default(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(data) => serde_yaml::from_str(&data).unwrap_or_default(),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = serde_yaml::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&path, yaml)
    }
}

/// Returns true if the user's config.yaml still contains the deprecated
/// `multiplexer_init_script` key. Used to surface a one-shot startup warning.
pub fn config_has_deprecated_init_script() -> bool {
    let path = config_path();
    let Ok(data) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&data) else {
        return false;
    };
    value
        .as_mapping()
        .and_then(|m| m.get(serde_yaml::Value::String("multiplexer_init_script".into())))
        .is_some()
}

/// Returns the ARTA config root directory.
/// Priority: ARTA_CONFIG_ROOT env var > ~/.arta/
pub fn config_root() -> PathBuf {
    if let Ok(root) = std::env::var("ARTA_CONFIG_ROOT") {
        PathBuf::from(root)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".arta")
    }
}

/// Returns the session prefix from ARTA_SESSION_PREFIX env var (empty by default).
pub fn session_prefix() -> String {
    std::env::var("ARTA_SESSION_PREFIX").unwrap_or_default()
}

/// Returns the path to workspace.yaml.
pub fn workspace_path() -> PathBuf {
    config_root().join("workspace.yaml")
}

/// Returns the path to config.yaml.
pub fn config_path() -> PathBuf {
    config_root().join("config.yaml")
}

/// Build the full multiplexer session name.
///
/// Format: `arta_{prefix}_{tag}_{session_id}` (prefix omitted when empty).
/// Examples: `arta_t_myproj-1`, `arta_work_z_myproj-1`
pub fn full_session_name(session_id: &str, prefix: &str, tag: &str) -> String {
    if prefix.is_empty() {
        format!("arta_{}_{}", tag, session_id)
    } else {
        format!("arta_{}_{}_{}", prefix, tag, session_id)
    }
}

/// Returns the common prefix that all session names for this profile share.
/// Used for filtering when listing sessions.
///
/// Examples: `arta_t_`, `arta_work_z_`
pub fn session_name_prefix(prefix: &str, tag: &str) -> String {
    if prefix.is_empty() {
        format!("arta_{}_", tag)
    } else {
        format!("arta_{}_{}_", prefix, tag)
    }
}

/// Extract the session_id from a full session name given the known prefix and tag.
pub fn extract_session_id(full_name: &str, prefix: &str, tag: &str) -> Option<String> {
    let pfx = session_name_prefix(prefix, tag);
    full_name.strip_prefix(&pfx).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert_eq!(config.coding_agent_command, "claude");
        assert_eq!(config.default_open_command, "vi");
        assert_eq!(config.multiplexer, Multiplexer::Tmux);
    }

    #[test]
    fn test_multiplexer_variants() {
        assert_eq!(Multiplexer::default(), Multiplexer::Tmux);
        assert_ne!(Multiplexer::Tmux, Multiplexer::Zellij);
    }

    #[test]
    fn test_yaml_full_config() {
        let yaml = r#"
coding_agent_command: "codex"
multiplexer: zellij
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.coding_agent_command, "codex");
        assert_eq!(config.multiplexer, Multiplexer::Zellij);
    }

    #[test]
    fn test_yaml_partial_config() {
        let yaml = "multiplexer: zellij\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.coding_agent_command, "claude");
        assert_eq!(config.multiplexer, Multiplexer::Zellij);
    }

    #[test]
    fn test_deprecated_init_script_is_ignored() {
        // Old config files with multiplexer_init_script should still parse
        // (the unknown field is silently dropped by serde) — they just don't
        // get the field.
        let yaml = r#"
coding_agent_command: "codex"
multiplexer_init_script: "/path/to/script.sh"
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.coding_agent_command, "codex");
    }

    #[test]
    fn test_yaml_empty_config() {
        let yaml = "{}";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.coding_agent_command, "claude");
        assert_eq!(config.default_open_command, "vi");
        assert_eq!(config.multiplexer, Multiplexer::Tmux);
    }

    #[test]
    fn test_config_serialize_roundtrip() {
        let cfg = Config {
            coding_agent_command: "codex".to_string(),
            default_open_command: "code .".to_string(),
            multiplexer: Multiplexer::Zellij,
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.coding_agent_command, "codex");
        assert_eq!(parsed.default_open_command, "code .");
        assert_eq!(parsed.multiplexer, Multiplexer::Zellij);
        // Ensure multiplexer round-trips as the lowercase form expected by humans.
        assert!(yaml.contains("multiplexer: zellij"));
    }

    #[test]
    fn test_multiplexer_as_str() {
        assert_eq!(Multiplexer::Tmux.as_str(), "tmux");
        assert_eq!(Multiplexer::Zellij.as_str(), "zellij");
    }

    #[test]
    fn test_full_session_name_no_prefix() {
        assert_eq!(full_session_name("proj-1", "", "t"), "arta_t_proj-1");
        assert_eq!(full_session_name("proj-1", "", "z"), "arta_z_proj-1");
    }

    #[test]
    fn test_full_session_name_with_prefix() {
        assert_eq!(
            full_session_name("proj-1", "work", "t"),
            "arta_work_t_proj-1"
        );
        assert_eq!(
            full_session_name("proj-1", "work", "z"),
            "arta_work_z_proj-1"
        );
    }

    #[test]
    fn test_session_name_prefix_fn() {
        assert_eq!(session_name_prefix("", "t"), "arta_t_");
        assert_eq!(session_name_prefix("work", "z"), "arta_work_z_");
    }

    #[test]
    fn test_extract_session_id() {
        assert_eq!(
            extract_session_id("arta_t_proj-1", "", "t"),
            Some("proj-1".to_string())
        );
        assert_eq!(
            extract_session_id("arta_work_z_proj-1", "work", "z"),
            Some("proj-1".to_string())
        );
        assert_eq!(extract_session_id("arta_t_proj-1", "work", "t"), None);
        assert_eq!(extract_session_id("arta_proj-1", "", "t"), None);
    }

    #[test]
    fn test_session_name_roundtrip() {
        let id = "myproject-42";
        let prefix = "dev";
        let tag = "t";
        let full = full_session_name(id, prefix, tag);
        let extracted = extract_session_id(&full, prefix, tag);
        assert_eq!(extracted.as_deref(), Some(id));
    }
}
