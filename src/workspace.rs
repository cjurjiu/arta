use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Project {
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_command: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub id: String,
    pub project: String,
    pub created: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Workspace {
    pub projects: Vec<Project>,
    pub sessions: Vec<Session>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_session: Option<String>,
    #[serde(default)]
    next_session_id: u64,
    #[serde(skip)]
    file_path: PathBuf,
}

pub fn sanitize_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

impl Workspace {
    pub fn new() -> Self {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("arta")
            .join("data");
        Workspace {
            projects: Vec::new(),
            sessions: Vec::new(),
            active_session: None,
            next_session_id: 0,
            file_path: dir.join("workspace.json"),
        }
    }

    pub fn load(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let data = match fs::read_to_string(&self.file_path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let loaded: Workspace = serde_json::from_str(&data)?;
        self.projects = loaded.projects;
        self.sessions = loaded.sessions;
        self.active_session = loaded.active_session;
        self.next_session_id = loaded.next_session_id;
        Ok(())
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(dir) = self.file_path.parent() {
            fs::create_dir_all(dir)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        fs::write(&self.file_path, data)?;
        Ok(())
    }

    pub fn add_project(&mut self, name: &str, path: &str, open_command: Option<&str>) {
        if self.projects.iter().any(|p| p.name == name) {
            return;
        }
        self.projects.push(Project {
            name: name.to_string(),
            path: path.to_string(),
            open_command: open_command.map(|s| s.to_string()),
        });
        let _ = self.save();
    }

    pub fn remove_project(&mut self, name: &str) {
        self.sessions.retain(|s| s.project != name);
        self.projects.retain(|p| p.name != name);
        let _ = self.save();
    }

    pub fn rename_project(&mut self, old_name: &str, new_name: &str) {
        for p in &mut self.projects {
            if p.name == old_name {
                p.name = new_name.to_string();
            }
        }
        for s in &mut self.sessions {
            if s.project == old_name {
                s.project = new_name.to_string();
            }
        }
        let _ = self.save();
    }

    pub fn swap_projects(&mut self, i: usize, j: usize) {
        if i < self.projects.len() && j < self.projects.len() {
            self.projects.swap(i, j);
            let _ = self.save();
        }
    }

    pub fn create_session(&mut self, project_name: &str) -> Option<&Session> {
        self.next_session_id += 1;
        let session = Session {
            id: format!("{}-{}", project_name, self.next_session_id),
            project: project_name.to_string(),
            created: chrono_now(),
        };
        self.sessions.push(session);
        let _ = self.save();
        self.sessions.last()
    }

    pub fn remove_session(&mut self, id: &str) {
        self.sessions.retain(|s| s.id != id);
        let _ = self.save();
    }

    pub fn rename_session(&mut self, old_id: &str, new_id: &str) -> bool {
        if self.sessions.iter().any(|s| s.id == new_id && s.id != old_id) {
            return false;
        }
        for s in &mut self.sessions {
            if s.id == old_id {
                s.id = new_id.to_string();
                break;
            }
        }
        let _ = self.save();
        true
    }

    pub fn swap_session_in_project(&mut self, id: &str, direction: i32) {
        let project = match self.sessions.iter().find(|s| s.id == id) {
            Some(s) => s.project.clone(),
            None => return,
        };

        let indices: Vec<usize> = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.project == project)
            .map(|(i, _)| i)
            .collect();

        for (pos, &idx) in indices.iter().enumerate() {
            if self.sessions[idx].id == id {
                let target_pos = pos as i32 + direction;
                if target_pos >= 0 && (target_pos as usize) < indices.len() {
                    let target_idx = indices[target_pos as usize];
                    self.sessions.swap(idx, target_idx);
                    let _ = self.save();
                }
                return;
            }
        }
    }

    pub fn sessions_for_project(&self, name: &str) -> Vec<&Session> {
        self.sessions.iter().filter(|s| s.project == name).collect()
    }

    pub fn get_project_path(&self, name: &str) -> Option<&str> {
        self.projects
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.path.as_str())
    }

    pub fn get_project_open_command(&self, name: &str) -> Option<&str> {
        self.projects
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.open_command.as_deref())
    }

    pub fn set_project_path(&mut self, name: &str, path: &str) {
        if let Some(p) = self.projects.iter_mut().find(|p| p.name == name) {
            p.path = path.to_string();
            let _ = self.save();
        }
    }

    pub fn set_active_session(&mut self, id: Option<&str>) {
        let new = id.map(|s| s.to_string());
        if self.active_session == new {
            return;
        }
        self.active_session = new;
        let _ = self.save();
    }

    pub fn set_project_open_command(&mut self, name: &str, cmd: &str) {
        if let Some(p) = self.projects.iter_mut().find(|p| p.name == name) {
            p.open_command = if cmd.is_empty() { None } else { Some(cmd.to_string()) };
            let _ = self.save();
        }
    }
}

fn chrono_now() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    // Simple ISO-8601 approximation without chrono dependency
    let secs = duration.as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to y/m/d (simplified)
    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md as i64 {
            m = i + 1;
            break;
        }
        remaining -= md as i64;
    }
    let d = remaining + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_workspace() -> Workspace {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "arta-test-{}-{}",
            std::process::id(),
            id
        ));
        let _ = fs::create_dir_all(&dir);
        Workspace {
            projects: Vec::new(),
            sessions: Vec::new(),
            active_session: None,
            next_session_id: 0,
            file_path: dir.join("workspace.json"),
        }
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("hello world"), "hello-world");
        assert_eq!(sanitize_name("my-project_1"), "my-project_1");
        assert_eq!(sanitize_name("a/b.c"), "a-b-c");
    }

    #[test]
    fn test_add_remove_project() {
        let mut ws = temp_workspace();
        ws.add_project("test", "/tmp/test", None);
        assert_eq!(ws.projects.len(), 1);
        assert_eq!(ws.projects[0].name, "test");

        // Duplicate should be ignored
        ws.add_project("test", "/tmp/test2", None);
        assert_eq!(ws.projects.len(), 1);

        ws.remove_project("test");
        assert_eq!(ws.projects.len(), 0);
    }

    #[test]
    fn test_create_session() {
        let mut ws = temp_workspace();
        ws.add_project("proj", "/tmp/proj", None);
        ws.create_session("proj");
        ws.create_session("proj");
        assert_eq!(ws.sessions.len(), 2);
        assert_eq!(ws.sessions[0].id, "proj-1");
        assert_eq!(ws.sessions[1].id, "proj-2");
    }

    #[test]
    fn test_rename_project() {
        let mut ws = temp_workspace();
        ws.add_project("old", "/tmp/old", None);
        ws.create_session("old");
        ws.rename_project("old", "new");
        assert_eq!(ws.projects[0].name, "new");
        assert_eq!(ws.sessions[0].project, "new");
    }

    #[test]
    fn test_json_roundtrip() {
        let mut ws = temp_workspace();
        ws.add_project("proj", "/tmp/proj", None);
        ws.create_session("proj");
        ws.save().unwrap();

        let mut ws2 = Workspace {
            file_path: ws.file_path.clone(),
            ..Default::default()
        };
        ws2.load().unwrap();
        assert_eq!(ws2.projects.len(), 1);
        assert_eq!(ws2.sessions.len(), 1);
        assert_eq!(ws2.projects[0].name, "proj");
        assert_eq!(ws2.sessions[0].id, "proj-1");

        // Clean up
        let _ = fs::remove_dir_all(ws.file_path.parent().unwrap());
    }

    #[test]
    fn test_swap_projects() {
        let mut ws = temp_workspace();
        ws.add_project("a", "/tmp/a", None);
        ws.add_project("b", "/tmp/b", None);
        ws.swap_projects(0, 1);
        assert_eq!(ws.projects[0].name, "b");
        assert_eq!(ws.projects[1].name, "a");
    }
}
