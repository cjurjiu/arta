use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::input_panel::{InputAction, InputMode, InputPanel};
use crate::keys;
use crate::sidebar::{Sidebar, SidebarAction};
use crate::terminal_pane::{PaneEvent, TerminalPane};
use crate::tmux;
use crate::welcome;
use crate::workspace::{self, Workspace};

const SIDEBAR_WIDTH: u16 = 30;
const INPUT_PANEL_HEIGHT: u16 = 15;

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Sidebar,
    Terminal,
    Input,
}

#[derive(Clone, Copy, PartialEq)]
enum InputPurpose {
    ProjectPath,
    ProjectName,
    ProjectOpenCommand,
    RenameProject,
    RenameSession,
    ConfirmCloseSession,
    ConfirmRemoveProject,
    ConfigureProjectPath,
    ConfigureOpenCommand,
}

const CONFIG_MENU_MAX_VISIBLE: usize = 5;

#[derive(Clone, Copy, PartialEq)]
enum ConfigOption {
    Rename,
    ProjectPath,
    OpenCommand,
}

struct ConfigMenu {
    project: String,
    items: Vec<(ConfigOption, &'static str)>,
    cursor: usize,
    scroll: usize,
}

impl ConfigMenu {
    fn new(project: String) -> Self {
        ConfigMenu {
            project,
            items: vec![
                (ConfigOption::Rename, "Rename"),
                (ConfigOption::ProjectPath, "Project path"),
                (ConfigOption::OpenCommand, "Open command"),
            ],
            cursor: 0,
            scroll: 0,
        }
    }

    fn height(&self) -> u16 {
        // separator + title + separator + visible items
        3 + self.items.len().min(CONFIG_MENU_MAX_VISIBLE) as u16
    }

    fn selected(&self) -> ConfigOption {
        self.items[self.cursor].0
    }

    fn handle_key(&mut self, key: &KeyEvent) -> ConfigMenuResult {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.cursor < self.items.len() - 1 {
                    self.cursor += 1;
                    if self.cursor >= self.scroll + CONFIG_MENU_MAX_VISIBLE {
                        self.scroll = self.cursor - CONFIG_MENU_MAX_VISIBLE + 1;
                    }
                }
                ConfigMenuResult::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    if self.cursor < self.scroll {
                        self.scroll = self.cursor;
                    }
                }
                ConfigMenuResult::None
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                ConfigMenuResult::Select(self.selected())
            }
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => ConfigMenuResult::Cancel,
            _ => ConfigMenuResult::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let dim = Style::default().add_modifier(Modifier::DIM);
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let w = area.width as usize;
        let mut y = area.y;

        // Separator
        let sep = "\u{2500}".repeat(w.saturating_sub(2));
        buf.set_line(
            area.x,
            y,
            &Line::from(Span::styled(format!(" {} ", sep), dim)),
            area.width,
        );
        y += 1;

        // Title
        buf.set_line(
            area.x,
            y,
            &Line::from(Span::styled(
                format!(" Settings for {}", self.project),
                bold,
            )),
            area.width,
        );
        y += 1;

        // Separator
        buf.set_line(
            area.x,
            y,
            &Line::from(Span::styled(format!(" {} ", sep), dim)),
            area.width,
        );
        y += 1;

        // Items
        let end = (self.scroll + CONFIG_MENU_MAX_VISIBLE).min(self.items.len());
        for i in self.scroll..end {
            if y >= area.y + area.height {
                break;
            }
            let (_, label) = self.items[i];
            let is_selected = i == self.cursor;
            let prefix = if is_selected { " \u{25b8} " } else { "   " };
            let style = if is_selected {
                Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default()
            };
            let text = format!("{}{:<width$}", prefix, label, width = w.saturating_sub(3));
            buf.set_line(area.x, y, &Line::from(Span::styled(text, style)), area.width);
            y += 1;
        }
    }
}

enum ConfigMenuResult {
    None,
    Select(ConfigOption),
    Cancel,
}

pub struct App {
    sidebar: Sidebar,
    panes: HashMap<String, TerminalPane>,
    input_panel: InputPanel,
    workspace: Workspace,
    focus: Focus,
    active_session: Option<String>,
    input_purpose: Option<InputPurpose>,
    input_context: Option<String>,
    pending_path: Option<String>,
    pending_name: Option<String>,
    status_message: Option<String>,
    timed_message: Option<(String, Instant)>,
    config_menu: Option<ConfigMenu>,
    prefix_active: bool,
    bell_tx: mpsc::Sender<PaneEvent>,
    bell_rx: mpsc::Receiver<PaneEvent>,
    last_bell_poll: Instant,
    bell_notified: HashSet<String>,
    width: u16,
    height: u16,
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let mut workspace = Workspace::new();
        let _ = workspace.load();

        // Prune dead sessions
        let live = tmux::list_sessions();
        workspace
            .sessions
            .retain(|s| live.contains(&tmux::session_name(&s.id)));
        let _ = workspace.save();

        // Get actual terminal size so PTYs are created at the right dimensions
        let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));

        let sidebar = Sidebar::new(&workspace);
        let input_panel = InputPanel::new();
        let (bell_tx, bell_rx) = mpsc::channel();

        let pane_width = term_w.saturating_sub(SIDEBAR_WIDTH + 1).max(10);
        let pane_height = term_h.saturating_sub(4).max(5);

        // Eagerly attach PTYs for all surviving sessions
        let mut panes = HashMap::new();
        for session in &workspace.sessions {
            let tmux_name = tmux::session_name(&session.id);
            // Re-apply bell settings on every attach (user's global tmux.conf
            // may override session-level settings between restarts)
            tmux::apply_bell_settings(&tmux_name);
            if let Ok(pane) = TerminalPane::new(
                session.id.clone(),
                &tmux_name,
                pane_height,
                pane_width,
                bell_tx.clone(),
            ) {
                panes.insert(session.id.clone(), pane);
            }
        }

        // Restore last active session, or fall back to first alive session
        let mut active_session = None;
        if let Some(ref saved_id) = workspace.active_session {
            if panes.contains_key(saved_id) {
                active_session = Some(saved_id.clone());
            }
        }
        if active_session.is_none() {
            // Find first alive session by project order
            'outer: for project in &workspace.projects {
                for session in workspace.sessions_for_project(&project.name) {
                    if panes.contains_key(&session.id) {
                        active_session = Some(session.id.clone());
                        break 'outer;
                    }
                }
            }
        }

        let (focus, sidebar_focused) = if active_session.is_some() {
            (Focus::Terminal, false)
        } else {
            (Focus::Sidebar, true)
        };

        let mut sidebar = sidebar;
        if let Some(ref id) = active_session {
            sidebar.set_selected(id);
            sidebar.ensure_expanded(id, &workspace);
            sidebar.set_focused(sidebar_focused);
        }

        App {
            sidebar,
            panes,
            input_panel,
            workspace,
            focus,
            active_session,
            input_purpose: None,
            input_context: None,
            pending_path: None,
            pending_name: None,
            status_message: None,
            timed_message: None,
            config_menu: None,
            prefix_active: false,
            bell_tx,
            bell_rx,
            last_bell_poll: Instant::now(),
            bell_notified: HashSet::new(),
            width: term_w,
            height: term_h,
            should_quit: false,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Resize(w, h) => {
                self.width = w;
                self.height = h;
                self.sidebar.set_size(SIDEBAR_WIDTH, h);
                let (pane_cols, pane_rows) = self.pane_size();
                for pane in self.panes.values_mut() {
                    pane.resize(pane_rows, pane_cols);
                }
            }
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => {
                if self.input_panel.is_active() || self.config_menu.is_some() {
                    return;
                }
                if mouse.column < SIDEBAR_WIDTH {
                    self.focus = Focus::Sidebar;
                    self.sidebar.set_focused(true);
                    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                        let action = self.sidebar.handle_mouse_click(mouse.row, &self.workspace);
                        self.process_sidebar_action(action);
                    }
                } else if self.active_session.is_some() {
                    self.focus = Focus::Terminal;
                    self.sidebar.set_focused(false);
                    self.forward_mouse_to_pane(mouse);
                }
            }
            Event::Paste(text) => {
                if self.focus == Focus::Terminal {
                    if let Some(session_id) = &self.active_session {
                        if let Some(pane) = self.panes.get_mut(session_id) {
                            // Send bracketed paste to the PTY so the application
                            // receives the entire paste as a single block
                            let mut buf = Vec::new();
                            buf.extend_from_slice(b"\x1b[200~");
                            buf.extend_from_slice(text.as_bytes());
                            buf.extend_from_slice(b"\x1b[201~");
                            pane.write_input(&buf);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        self.status_message = None;

        // Config menu gets priority
        if let Some(menu) = &mut self.config_menu {
            let result = menu.handle_key(&key);
            match result {
                ConfigMenuResult::Select(option) => {
                    let project = menu.project.clone();
                    self.config_menu = None;
                    self.handle_config_select(option, &project);
                }
                ConfigMenuResult::Cancel => {
                    self.config_menu = None;
                    self.focus = Focus::Sidebar;
                    self.sidebar.set_focused(true);
                }
                ConfigMenuResult::None => {}
            }
            return;
        }

        // Input panel gets priority
        if self.input_panel.is_active() {
            let action = self.input_panel.handle_key(&key);
            match action {
                InputAction::Submit(value) => self.handle_input_submit(value),
                InputAction::Cancel => {
                    self.input_purpose = None;
                    self.pending_path = None;
                    self.pending_name = None;
                    self.focus = Focus::Sidebar;
                    self.sidebar.set_focused(true);
                }
                InputAction::None => {}
            }
            return;
        }

        // Prefix mode: Ctrl+Space was pressed, next key is a command
        if self.prefix_active {
            self.prefix_active = false;
            self.sidebar.set_prefix_active(false);
            match key.code {
                // Focus switching
                KeyCode::Left => {
                    self.focus = Focus::Sidebar;
                    self.sidebar.set_focused(true);
                    return;
                }
                KeyCode::Right => {
                    if self.active_session.is_some() {
                        self.focus = Focus::Terminal;
                        self.sidebar.set_focused(false);
                    }
                    return;
                }
                // Action commands work from any focus via prefix
                _ => {
                    let action = self.sidebar.handle_prefix_key(&key);
                    self.process_sidebar_action(action);
                    return;
                }
            }
        }

        // Ctrl+Space activates prefix
        if key.code == KeyCode::Char(' ') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.prefix_active = true;
            self.sidebar.set_prefix_active(true);
            return;
        }

        match self.focus {
            Focus::Sidebar => {
                let action = self.sidebar.handle_key(&key, &self.workspace);
                self.process_sidebar_action(action);
            }
            Focus::Terminal => {
                if let Some(session_id) = &self.active_session {
                    if let Some(bytes) = keys::key_event_to_bytes(&key) {
                        if let Some(pane) = self.panes.get_mut(session_id) {
                            pane.write_input(&bytes);
                        }
                    }
                }
            }
            Focus::Input => {}
        }
    }

    fn process_sidebar_action(&mut self, action: SidebarAction) {
        match action {
            SidebarAction::None => {}
            SidebarAction::SelectSession(id) => {
                self.sidebar.set_selected(&id);
                self.sidebar.clear_attention(&id);
                self.bell_notified.remove(&id);
                self.active_session = Some(id.clone());
                self.workspace.set_active_session(Some(&id));
                self.focus = Focus::Terminal;
                self.sidebar.set_focused(false);
            }
            SidebarAction::NewSession(project) => {
                self.create_session(&project);
            }
            SidebarAction::CloseSession(id) => {
                self.open_input(
                    InputPurpose::ConfirmCloseSession,
                    &format!("Close session {}? (y/n)", id),
                    "",
                    &id,
                );
            }
            SidebarAction::AddProject => {
                let home = home_dir_string();
                self.open_input_path(
                    InputPurpose::ProjectPath,
                    "Project directory",
                    &format!("{}/", home),
                );
            }
            SidebarAction::RemoveProject(name) => {
                self.open_input(
                    InputPurpose::ConfirmRemoveProject,
                    &format!("Remove {} and all sessions? (y/n)", name),
                    "",
                    &name,
                );
            }
            SidebarAction::RenameProject(old) => {
                self.open_input(InputPurpose::RenameProject, "Rename project", &old, &old);
            }
            SidebarAction::RenameSession(id) => {
                self.open_input(InputPurpose::RenameSession, "Rename session", &id, &id);
            }
            SidebarAction::MoveProject(direction) => {
                if let Some(project_name) = self.sidebar.get_cursor_project() {
                    let project_name = project_name.to_string();
                    if let Some(i) = self
                        .workspace
                        .projects
                        .iter()
                        .position(|p| p.name == project_name)
                    {
                        let target = i as i32 + direction;
                        if target >= 0 && (target as usize) < self.workspace.projects.len() {
                            self.workspace.swap_projects(i, target as usize);
                            self.sidebar.refresh(&self.workspace);
                            self.sidebar.set_cursor_to_project(&project_name);
                        }
                    }
                }
            }
            SidebarAction::MoveSession(id, direction) => {
                self.workspace.swap_session_in_project(&id, direction);
                self.sidebar.refresh(&self.workspace);
                self.sidebar.set_cursor_to_session(&id);
            }
            SidebarAction::OpenIde(project) => {
                if let Some(cmd) = self.workspace.get_project_open_command(&project) {
                    let cmd = cmd.to_string();
                    let dir = self
                        .workspace
                        .get_project_path(&project)
                        .unwrap_or("")
                        .to_string();
                    let parts: Vec<&str> = cmd.split_whitespace().collect();
                    if let Some((&program, args)) = parts.split_first() {
                        let _ = std::process::Command::new(program)
                            .args(args)
                            .current_dir(&dir)
                            .spawn();
                    }
                } else {
                    self.status_message =
                        Some("No open command configured. Press 'c' to configure.".to_string());
                }
            }
            SidebarAction::ConfigureProject(name) => {
                self.config_menu = Some(ConfigMenu::new(name));
                self.focus = Focus::Input;
                self.sidebar.set_focused(false);
            }
            SidebarAction::CopyGithubLink => {
                let url = "https://github.com/cjurjiu/arta";
                let copied = std::process::Command::new("pbcopy")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .and_then(|mut child| {
                        use std::io::Write;
                        if let Some(mut stdin) = child.stdin.take() {
                            stdin.write_all(url.as_bytes())?;
                        }
                        child.wait()
                    })
                    .is_ok();
                if copied {
                    self.timed_message =
                        Some(("github link copied to clipboard".to_string(), Instant::now()));
                }
            }
            SidebarAction::FocusTerminal => {
                if self.active_session.is_some() {
                    self.focus = Focus::Terminal;
                    self.sidebar.set_focused(false);
                }
            }
            SidebarAction::Quit => {
                let _ = self.workspace.save();
                // Drop all panes cleanly — this detaches tmux clients
                // but leaves the tmux sessions alive
                self.panes.clear();
                self.should_quit = true;
            }
            SidebarAction::CleanExit => {
                // Only kill sessions tracked in our workspace, not all arta_ sessions
                let session_ids: Vec<String> =
                    self.workspace.sessions.iter().map(|s| s.id.clone()).collect();
                self.panes.clear();
                for id in &session_ids {
                    tmux::kill_session(&tmux::session_name(id));
                }
                self.workspace.sessions.clear();
                let _ = self.workspace.save();
                self.should_quit = true;
            }
        }
    }

    fn open_input(&mut self, purpose: InputPurpose, title: &str, initial: &str, context: &str) {
        self.input_purpose = Some(purpose);
        self.input_context = Some(context.to_string());
        self.focus = Focus::Input;
        self.sidebar.set_focused(false);
        self.input_panel
            .activate(InputMode::Text, title, initial, self.width, INPUT_PANEL_HEIGHT);
    }

    fn open_input_path(&mut self, purpose: InputPurpose, title: &str, initial: &str) {
        self.input_purpose = Some(purpose);
        self.focus = Focus::Input;
        self.sidebar.set_focused(false);
        self.input_panel
            .activate(InputMode::Path, title, initial, self.width, INPUT_PANEL_HEIGHT);
    }

    fn handle_input_submit(&mut self, value: String) {
        let purpose = self.input_purpose.take();
        let context = self.input_context.take();

        match purpose {
            Some(InputPurpose::ProjectPath) => {
                let path = expand_tilde(&value);
                self.pending_path = Some(path.clone());
                let default_name = path
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or(&path)
                    .to_string();
                self.open_input(InputPurpose::ProjectName, "Project name", &default_name, "");
                return;
            }
            Some(InputPurpose::ProjectName) => {
                if !value.is_empty() {
                    let name = workspace::sanitize_name(&value);
                    self.pending_name = Some(name.clone());
                    self.open_input(
                        InputPurpose::ProjectOpenCommand,
                        "Open command (optional, e.g. \"webstorm .\")",
                        "",
                        "",
                    );
                    return;
                }
                self.pending_path = None;
                self.pending_name = None;
            }
            Some(InputPurpose::ProjectOpenCommand) => {
                let name = self.pending_name.take().unwrap_or_default();
                let path = self.pending_path.take().unwrap_or_default();
                let open_cmd = if value.is_empty() { None } else { Some(value.as_str()) };
                self.workspace.add_project(&name, &path, open_cmd);
                self.sidebar.refresh(&self.workspace);
            }
            Some(InputPurpose::RenameProject) => {
                let new_name = workspace::sanitize_name(&value);
                if let Some(old) = context {
                    if !new_name.is_empty() && new_name != old {
                        self.workspace.rename_project(&old, &new_name);
                        self.sidebar.refresh(&self.workspace);
                    }
                }
            }
            Some(InputPurpose::RenameSession) => {
                let new_id = workspace::sanitize_name(&value);
                if let Some(old_id) = context {
                    if !new_id.is_empty() && new_id != old_id {
                        if !self.workspace.rename_session(&old_id, &new_id) {
                            self.timed_message = Some((
                                format!("Session '{}' already exists", new_id),
                                Instant::now(),
                            ));
                        } else {
                            tmux::rename_session(
                                &tmux::session_name(&old_id),
                                &tmux::session_name(&new_id),
                            );

                            if let Some(pane) = self.panes.remove(&old_id) {
                                self.panes.insert(new_id.clone(), pane);
                            }

                            if self.active_session.as_deref() == Some(&old_id) {
                                self.active_session = Some(new_id.clone());
                            }
                            self.sidebar.set_selected(&new_id);
                            self.sidebar.refresh(&self.workspace);
                        }
                    }
                }
            }
            Some(InputPurpose::ConfirmCloseSession) => {
                if value == "y" || value == "Y" {
                    if let Some(id) = context {
                        self.close_session(&id);
                    }
                }
            }
            Some(InputPurpose::ConfirmRemoveProject) => {
                if value == "y" || value == "Y" {
                    if let Some(name) = context {
                        let session_ids: Vec<String> = self
                            .workspace
                            .sessions_for_project(&name)
                            .iter()
                            .map(|s| s.id.clone())
                            .collect();
                        for sid in &session_ids {
                            tmux::kill_session(&tmux::session_name(sid));
                            self.detach_pane(sid);
                        }
                        self.workspace.remove_project(&name);
                        self.sidebar.refresh(&self.workspace);
                    }
                }
            }
            Some(InputPurpose::ConfigureProjectPath) => {
                if let Some(project) = context {
                    let path = expand_tilde(&value);
                    if !path.is_empty() {
                        self.workspace.set_project_path(&project, &path);
                    }
                }
            }
            Some(InputPurpose::ConfigureOpenCommand) => {
                if let Some(project) = context {
                    self.workspace.set_project_open_command(&project, &value);
                }
            }
            None => {}
        }

        self.focus = Focus::Sidebar;
        self.sidebar.set_focused(true);
    }

    fn handle_config_select(&mut self, option: ConfigOption, project: &str) {
        match option {
            ConfigOption::Rename => {
                let title = format!("Rename project \u{2014} {}", project);
                self.open_input(InputPurpose::RenameProject, &title, project, project);
            }
            ConfigOption::ProjectPath => {
                let current_path = self
                    .workspace
                    .get_project_path(project)
                    .unwrap_or("")
                    .to_string();
                let title = format!("Project path \u{2014} {}", project);
                self.input_context = Some(project.to_string());
                self.open_input_path(
                    InputPurpose::ConfigureProjectPath,
                    &title,
                    &current_path,
                );
            }
            ConfigOption::OpenCommand => {
                let current_cmd = self
                    .workspace
                    .get_project_open_command(project)
                    .unwrap_or("")
                    .to_string();
                let title = format!("Open command \u{2014} {} (e.g. \"webstorm .\")", project);
                self.open_input(
                    InputPurpose::ConfigureOpenCommand,
                    &title,
                    &current_cmd,
                    project,
                );
            }
        }
    }

    fn create_session(&mut self, project_name: &str) {
        let session = match self.workspace.create_session(project_name) {
            Some(s) => s.clone(),
            None => return,
        };

        let tmux_name = tmux::session_name(&session.id);
        let path = self
            .workspace
            .get_project_path(project_name)
            .unwrap_or("")
            .to_string();
        let dir = if path.is_empty() {
            home_dir_string()
        } else {
            path
        };

        tmux::create_session(&tmux_name, &dir);

        let (pane_cols, pane_rows) = self.pane_size();
        if let Ok(pane) = TerminalPane::new(
            session.id.clone(),
            &tmux_name,
            pane_rows,
            pane_cols,
            self.bell_tx.clone(),
        ) {
            self.panes.insert(session.id.clone(), pane);
            self.active_session = Some(session.id.clone());
            self.workspace.set_active_session(Some(&session.id));
            self.sidebar.set_selected(&session.id);
            self.sidebar.ensure_expanded(&session.id, &self.workspace);
            self.focus = Focus::Terminal;
            self.sidebar.set_focused(false);
        }
    }

    fn close_session(&mut self, id: &str) {
        tmux::kill_session(&tmux::session_name(id));
        self.detach_pane(id);
        self.workspace.remove_session(id);
        self.sidebar.refresh(&self.workspace);
    }

    /// Remove a pane and clear active_session if it was the one removed.
    fn detach_pane(&mut self, id: &str) {
        self.panes.remove(id);
        if self.active_session.as_deref() == Some(id) {
            self.active_session = None;
            self.workspace.set_active_session(None);
        }
    }

    pub fn check_pane_events(&mut self) {
        // Clear expired timed messages
        if let Some((_, created)) = &self.timed_message {
            if created.elapsed() >= std::time::Duration::from_secs(3) {
                self.timed_message = None;
            }
        }

        // Process bell and death events from the PTY reader channel
        while let Ok(event) = self.bell_rx.try_recv() {
            match event {
                PaneEvent::Bell(session_id) => {
                    if self.active_session.as_deref() != Some(&session_id) {
                        bell_log(&format!(
                            "bell(pty): session={} (notifying)",
                            session_id
                        ));
                        self.sidebar.set_attention(&session_id);
                        play_bell();
                    }
                }
                PaneEvent::Death(session_id) => {
                    self.detach_pane(&session_id);
                    self.bell_notified.remove(&session_id);
                    self.sidebar.clear_attention(&session_id);
                    self.workspace.remove_session(&session_id);
                    self.sidebar.refresh(&self.workspace);
                }
            }
        }

        // Poll tmux for bell flags every 500ms (skip if no sessions)
        if !self.panes.is_empty() && self.last_bell_poll.elapsed() >= Duration::from_millis(500) {
            self.last_bell_poll = Instant::now();
            let flags = tmux::check_bell_flags();
            for (session_id, has_bell) in flags {
                if has_bell {
                    if !self.bell_notified.contains(&session_id) {
                        self.bell_notified.insert(session_id.clone());
                        if self.active_session.as_deref() != Some(&session_id) {
                            bell_log(&format!(
                                "bell: session={} (notifying, active={:?})",
                                session_id, self.active_session
                            ));
                            self.sidebar.set_attention(&session_id);
                            play_bell();
                        } else {
                            bell_log(&format!(
                                "bell: session={} (skipped, is active)",
                                session_id
                            ));
                        }
                    }
                } else {
                    self.bell_notified.remove(&session_id);
                }
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let size = frame.area();
        self.width = size.width;
        self.height = size.height;

        let mut main_height = size.height;
        if self.input_panel.is_active() {
            main_height = size.height.saturating_sub(INPUT_PANEL_HEIGHT + 1).max(5);
        } else if let Some(menu) = &self.config_menu {
            main_height = size.height.saturating_sub(menu.height()).max(5);
        }

        self.sidebar.set_size(SIDEBAR_WIDTH, main_height);

        let sidebar_area = Rect::new(0, 0, SIDEBAR_WIDTH, main_height);
        self.sidebar
            .render(sidebar_area, frame.buffer_mut(), &self.workspace);

        // Separator
        let sep_x = SIDEBAR_WIDTH;
        let dim = Style::default().add_modifier(Modifier::DIM);
        for y in 0..main_height {
            frame.buffer_mut().set_line(
                sep_x,
                y,
                &Line::from(Span::styled("\u{2502}", dim)),
                1,
            );
        }

        // Right pane
        let right_x = SIDEBAR_WIDTH + 1;
        let right_width = size.width.saturating_sub(right_x);

        let show_terminal = self
            .active_session
            .as_ref()
            .and_then(|id| self.panes.get(id.as_str()))
            .is_some();

        if show_terminal {
            let focus_red_bold = Style::default()
                .fg(Color::Rgb(0xFF, 0x6C, 0x6B))
                .add_modifier(Modifier::BOLD);
            let term_focused = self.focus == Focus::Terminal;
            let border_style = if term_focused { focus_red_bold } else { dim };

            // Top border (always visible, red when focused)
            {
                let border = "\u{2500}".repeat(right_width as usize);
                frame.buffer_mut().set_line(
                    right_x,
                    0,
                    &Line::from(Span::styled(&border, border_style)),
                    right_width,
                );
            }

            // Terminal content (reserved is always 4: top + bottom border + 2 status)
            let term_content_y: u16 = 1;
            let term_content_height = main_height.saturating_sub(4);
            let term_area = Rect::new(right_x, term_content_y, right_width, term_content_height);

            let id = self.active_session.as_ref().unwrap();
            self.panes
                .get(id.as_str())
                .unwrap()
                .render(term_area, frame.buffer_mut());

            // Bottom border (always visible, "focused" label when active)
            let border_y = term_content_y + term_content_height;
            if term_focused {
                let rest = "\u{2500}".repeat((right_width as usize).saturating_sub(9));
                frame.buffer_mut().set_line(
                    right_x,
                    border_y,
                    &Line::from(vec![
                        Span::styled(" focused ", border_style),
                        Span::styled(rest, border_style),
                    ]),
                    right_width,
                );
            } else {
                let border = "\u{2500}".repeat(right_width as usize);
                frame.buffer_mut().set_line(
                    right_x,
                    border_y,
                    &Line::from(Span::styled(border, border_style)),
                    right_width,
                );
            }

        } else {
            let right_area = Rect::new(right_x, 0, right_width, main_height);
            welcome::render_welcome(right_area, frame.buffer_mut());
        }

        // Status bar (always visible, right-aligned on second-to-last line)
        let bar_y = main_height.saturating_sub(2);
        if bar_y > 0 {
            let gray = Style::default().fg(Color::Rgb(0x88, 0x88, 0x88));
            let gray_bold = gray.add_modifier(Modifier::BOLD);
            let mode = if self.prefix_active { "run" } else { "interactive" };
            let version_text = format!("v{}", env!("CARGO_PKG_VERSION"));
            let bar = Line::from(vec![
                Span::styled(mode, gray_bold),
                Span::styled(" | ", gray),
                Span::styled(version_text, gray),
                Span::styled(" | ", gray),
                Span::styled("MIT", gray),
                Span::styled(" | ", gray),
                Span::styled("g", gray_bold),
                Span::styled(" - github ", gray),
            ]);
            let bar_width: u16 = (mode.len() + 30) as u16;
            let bar_x = right_x + right_width.saturating_sub(bar_width);
            frame.buffer_mut().set_line(bar_x, bar_y, &bar, bar_width);

            // Left side: timed message or "awaiting command..." in prefix mode
            if let Some((msg, _)) = &self.timed_message {
                frame.buffer_mut().set_line(
                    right_x,
                    bar_y,
                    &Line::from(Span::styled(format!(" {}", msg), Style::default())),
                    right_width.saturating_sub(bar_width),
                );
            } else if self.prefix_active {
                frame.buffer_mut().set_line(
                    right_x,
                    bar_y,
                    &Line::from(Span::styled(" awaiting command...", gray)),
                    right_width.saturating_sub(bar_width),
                );
            }
        }

        // Status message line (last line of right pane)
        if let Some(msg) = &self.status_message {
            let status_y = main_height.saturating_sub(1);
            if status_y > 0 {
                let status_style = Style::default()
                    .fg(Color::Rgb(0xFF, 0x6C, 0x6B))
                    .add_modifier(Modifier::DIM);
                frame.buffer_mut().set_line(
                    right_x,
                    status_y,
                    &Line::from(Span::styled(format!(" {}", msg), status_style)),
                    right_width,
                );
            }
        }

        // Input panel
        if self.input_panel.is_active() {
            let panel_y = main_height;
            let panel_height = size.height.saturating_sub(main_height);
            let panel_area = Rect::new(0, panel_y, size.width, panel_height);
            self.input_panel.render(panel_area, frame.buffer_mut());

            let (cx, cy) = self.input_panel.cursor_position(panel_area);
            frame.set_cursor_position((cx, cy));
        }

        // Config menu
        if let Some(menu) = &self.config_menu {
            let panel_y = main_height;
            let panel_height = size.height.saturating_sub(main_height);
            let panel_area = Rect::new(0, panel_y, size.width, panel_height);
            menu.render(panel_area, frame.buffer_mut());
        }
    }

    fn pane_size(&self) -> (u16, u16) {
        let right_width = self.width.saturating_sub(SIDEBAR_WIDTH + 1);
        let mut main_height = self.height;
        if self.input_panel.is_active() {
            main_height = main_height.saturating_sub(INPUT_PANEL_HEIGHT + 1);
        } else if let Some(menu) = &self.config_menu {
            main_height = main_height.saturating_sub(menu.height());
        }
        // 1 line top padding + 2 status lines + 1 line bottom border (when focused)
        let pane_height = main_height.saturating_sub(4);
        (right_width.max(10), pane_height.max(5))
    }

    fn forward_mouse_to_pane(&mut self, mouse: crossterm::event::MouseEvent) {
        let session_id = match &self.active_session {
            Some(id) => id.clone(),
            None => return,
        };
        let pane = match self.panes.get_mut(&session_id) {
            Some(p) => p,
            None => return,
        };

        let col = mouse.column.saturating_sub(SIDEBAR_WIDTH + 1) + 1;
        let row = mouse.row + 1;

        let (button, suffix) = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => (0, 'M'),
            MouseEventKind::Down(MouseButton::Right) => (2, 'M'),
            MouseEventKind::Down(MouseButton::Middle) => (1, 'M'),
            MouseEventKind::Up(MouseButton::Left) => (0, 'm'),
            MouseEventKind::Up(MouseButton::Right) => (2, 'm'),
            MouseEventKind::Up(MouseButton::Middle) => (1, 'm'),
            MouseEventKind::Drag(MouseButton::Left) => (32, 'M'),
            MouseEventKind::Drag(MouseButton::Right) => (34, 'M'),
            MouseEventKind::Drag(MouseButton::Middle) => (33, 'M'),
            MouseEventKind::ScrollUp => (64, 'M'),
            MouseEventKind::ScrollDown => (65, 'M'),
            MouseEventKind::Moved => (35, 'M'),
            _ => return,
        };

        let seq = format!("\x1b[<{};{};{}{}", button, col, row, suffix);
        pane.write_input(seq.as_bytes());
    }
}

fn home_dir_string() -> String {
    dirs::home_dir()
        .map(|h| h.display().to_string())
        .unwrap_or_default()
}

pub fn expand_tilde(path: &str) -> String {
    match path.strip_prefix('~') {
        Some(rest) => format!("{}{}", home_dir_string(), rest),
        None => path.to_string(),
    }
}

fn play_bell() {
    if let Ok(child) = std::process::Command::new("afplay")
        .args(["-v", "0.5", "/System/Library/Sounds/Tink.aiff"])
        .spawn()
    {
        std::thread::spawn(move || {
            let mut child = child;
            let _ = child.wait();
        });
    }
}

fn bell_log(msg: &str) {
    use std::io::Write;
    let home = home_dir_string();
    let dir = format!("{}/.local/share/arta", home);
    let _ = std::fs::create_dir_all(&dir);
    let path = format!("{}/bell.log", dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(f, "[{}] {}", secs, msg);
    }
}
