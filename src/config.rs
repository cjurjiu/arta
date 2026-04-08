use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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


fn default_coding_agent_command() -> String {
    "claude".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_coding_agent_command")]
    pub coding_agent_command: String,

    #[serde(default)]
    pub multiplexer: Multiplexer,

    #[serde(default)]
    pub multiplexer_init_script: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            coding_agent_command: "claude".to_string(),
            multiplexer: Multiplexer::default(),
            multiplexer_init_script: None,
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
        assert_eq!(config.multiplexer, Multiplexer::Tmux);
        assert!(config.multiplexer_init_script.is_none());
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
multiplexer_init_script: "/path/to/script.sh"
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.coding_agent_command, "codex");
        assert_eq!(config.multiplexer, Multiplexer::Zellij);
        assert_eq!(
            config.multiplexer_init_script.as_deref(),
            Some("/path/to/script.sh")
        );
    }

    #[test]
    fn test_yaml_partial_config() {
        let yaml = "multiplexer: zellij\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.coding_agent_command, "claude");
        assert_eq!(config.multiplexer, Multiplexer::Zellij);
        assert!(config.multiplexer_init_script.is_none());
    }

    #[test]
    fn test_yaml_empty_config() {
        let yaml = "{}";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.coding_agent_command, "claude");
        assert_eq!(config.multiplexer, Multiplexer::Tmux);
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
