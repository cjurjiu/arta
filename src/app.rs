use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

use crate::claude_hook;
use crate::config::{self, Config};
use crate::input_panel::{InputAction, InputMode, InputPanel};
use crate::keys;
use crate::multiplexer::{self, MultiplexerBackend};
use crate::sidebar::{Sidebar, SidebarAction};
use crate::terminal_pane::{PaneEvent, TerminalPane};
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
    RenameThread,
    ConfirmCloseThread,
    ConfirmRemoveProject,
    ConfigureProjectPath,
    ConfigureOpenCommand,
    ConfigureAgentCommand,
}

const CONFIG_MENU_MAX_VISIBLE: usize = 5;

#[derive(Clone, Copy, PartialEq)]
enum ConfigOption {
    Rename,
    ProjectPath,
    AgentCommand,
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
                (ConfigOption::AgentCommand, "Agent command"),
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
    config: Config,
    mux: Box<dyn MultiplexerBackend>,
    session_prefix: String,
    session_name_prefix: String,
    sidebar: Sidebar,
    panes: HashMap<String, TerminalPane>,
    input_panel: InputPanel,
    workspace: Workspace,
    focus: Focus,
    active_thread: Option<String>,
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
    /// Last OSC title observed per thread id, so we only react when it changes.
    last_seen_title: HashMap<String, String>,
    width: u16,
    height: u16,
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let cfg = Config::load();
        let mux = multiplexer::backend_for(cfg.multiplexer);
        let session_prefix = config::session_prefix();
        let session_name_prefix =
            config::session_name_prefix(&session_prefix, mux.tag());

        // Migrate workspace from legacy location if needed
        let ws_path = config::workspace_path();
        workspace::migrate_workspace_if_needed(&ws_path);

        let mut workspace = Workspace::new(ws_path);
        let _ = workspace.load();

        // Auto-migrate old-format session names (arta_X -> arta_t_X)
        Self::migrate_old_sessions(&workspace, &session_prefix, mux.tag());

        // Warn about sessions from the other multiplexer
        let other_tag = if mux.tag() == "t" { "z" } else { "t" };
        let other_prefix = config::session_name_prefix(&session_prefix, other_tag);
        let other_mux = if mux.tag() == "t" {
            multiplexer::backend_for(config::Multiplexer::Zellij)
        } else {
            multiplexer::backend_for(config::Multiplexer::Tmux)
        };
        let other_sessions = other_mux.list_sessions(&other_prefix);
        let timed_message = if config::config_has_deprecated_init_script() {
            Some((
                "`multiplexer_init_script` in config.yaml is no longer supported and is ignored. ARTA now always uses its default layout."
                    .to_string(),
                Instant::now(),
            ))
        } else if other_sessions.is_empty() {
            None
        } else {
            Some((
                format!(
                    "{} thread(s) from other multiplexer still running",
                    other_sessions.len()
                ),
                Instant::now(),
            ))
        };

        // Prune dead threads (no live multiplexer session)
        let live = mux.list_sessions(&session_name_prefix);
        workspace.threads.retain(|t| {
            let full = config::full_session_name(&t.id, &session_prefix, mux.tag());
            live.contains(&full)
        });
        let _ = workspace.save();

        // Get actual terminal size so PTYs are created at the right dimensions
        let (term_w, term_h) = crossterm::terminal::size().unwrap_or((80, 24));

        let sidebar = Sidebar::new(&workspace);
        let input_panel = InputPanel::new();
        let (bell_tx, bell_rx) = mpsc::channel();

        let pane_width = term_w.saturating_sub(SIDEBAR_WIDTH + 1).max(10);
        let pane_height = term_h.saturating_sub(4).max(5);

        // Inject claude-code's Notification hook into user-scope settings once per
        // arta launch. Idempotent — safe to re-run. The hook writes to the per-session
        // ARTA_BELL_MARKER env var (set by the multiplexer at session create time);
        // if that env var isn't set (e.g. claude invoked outside arta), the hook is
        // a no-op. Install if the global agent or any per-project override mentions
        // claude — over-installing is harmless because the hook short-circuits when
        // the marker env var is absent.
        let global_uses_claude = cfg
            .coding_agent_command
            .split_whitespace()
            .next()
            .unwrap_or("")
            .contains("claude");
        let any_project_uses_claude = workspace.projects.iter().any(|p| {
            p.agent_command
                .as_deref()
                .and_then(|c| c.split_whitespace().next())
                .map(|first| first.contains("claude"))
                .unwrap_or(false)
        });
        if global_uses_claude || any_project_uses_claude {
            if let Err(e) = claude_hook::ensure_user_notify_hook() {
                bell_log(&format!("claude_hook: ensure_user_notify_hook failed: {}", e));
            }
        }

        // Eagerly attach PTYs for all surviving threads
        let mut panes = HashMap::new();
        for thread in &workspace.threads {
            let full_name =
                config::full_session_name(&thread.id, &session_prefix, mux.tag());
            // Re-apply bell settings on every attach
            mux.apply_bell_settings(&full_name);
            // (Re)export the marker into the tmux session environment so any
            // newly-spawned process (e.g. claude restarted by the user) inherits it.
            if mux.tag() == "t" {
                let marker = multiplexer::bell_marker_path(&full_name);
                let _ = std::process::Command::new("tmux")
                    .args([
                        "set-environment",
                        "-t",
                        &full_name,
                        "ARTA_BELL_MARKER",
                        &marker.display().to_string(),
                    ])
                    .output();
            }
            let (cmd, args) = mux.attach_command(&full_name);
            if let Ok(pane) = TerminalPane::new(
                thread.id.clone(),
                &cmd,
                &args,
                pane_height,
                pane_width,
                bell_tx.clone(),
            ) {
                panes.insert(thread.id.clone(), pane);
            }
        }

        // Restore last active thread, or fall back to first alive thread
        let mut active_thread = None;
        if let Some(ref saved_id) = workspace.active_thread {
            if panes.contains_key(saved_id) {
                active_thread = Some(saved_id.clone());
            }
        }
        if active_thread.is_none() {
            'outer: for project in &workspace.projects {
                for thread in workspace.threads_for_project(&project.name) {
                    if panes.contains_key(&thread.id) {
                        active_thread = Some(thread.id.clone());
                        break 'outer;
                    }
                }
            }
        }

        let (focus, sidebar_focused) = if active_thread.is_some() {
            (Focus::Terminal, false)
        } else {
            (Focus::Sidebar, true)
        };

        let mut sidebar = sidebar;
        if let Some(ref id) = active_thread {
            sidebar.set_selected(id);
            sidebar.ensure_expanded(id, &workspace);
            sidebar.set_focused(sidebar_focused);
        }

        App {
            config: cfg,
            mux,
            session_prefix,
            session_name_prefix,
            sidebar,
            panes,
            input_panel,
            workspace,
            focus,
            active_thread,
            input_purpose: None,
            input_context: None,
            pending_path: None,
            pending_name: None,
            status_message: None,
            timed_message,
            config_menu: None,
            prefix_active: false,
            bell_tx,
            bell_rx,
            last_bell_poll: Instant::now(),
            last_seen_title: HashMap::new(),
            width: term_w,
            height: term_h,
            should_quit: false,
        }
    }

    /// Rename old-format tmux sessions (arta_X) to the new format (arta_t_X).
    fn migrate_old_sessions(workspace: &Workspace, prefix: &str, tag: &str) {
        let tmux = multiplexer::TmuxBackend;
        // List all tmux sessions starting with "arta_"
        let all = tmux.list_sessions("arta_");
        let new_prefix = config::session_name_prefix(prefix, tag);
        for full_name in &all {
            // Skip sessions that already match the new format
            if full_name.starts_with(&new_prefix) {
                continue;
            }
            // Check if this is an old-format name for a known thread
            if let Some(thread_id) = full_name.strip_prefix("arta_") {
                if workspace.threads.iter().any(|t| t.id == thread_id) {
                    let new_name = config::full_session_name(thread_id, prefix, tag);
                    tmux.rename_session(full_name, &new_name);
                }
            }
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
                } else if self.active_thread.is_some() {
                    self.focus = Focus::Terminal;
                    self.sidebar.set_focused(false);
                    self.forward_mouse_to_pane(mouse);
                }
            }
            Event::Paste(text) => {
                if self.focus == Focus::Terminal {
                    if let Some(thread_id) = &self.active_thread {
                        if let Some(pane) = self.panes.get_mut(thread_id) {
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
                    if self.active_thread.is_some() {
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
                if let Some(thread_id) = &self.active_thread {
                    if let Some(bytes) = keys::key_event_to_bytes(&key) {
                        if let Some(pane) = self.panes.get_mut(thread_id) {
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
            SidebarAction::SelectThread(id) => {
                self.sidebar.set_selected(&id);
                self.sidebar.clear_attention(&id);
                self.active_thread = Some(id.clone());
                self.workspace.set_active_thread(Some(&id));
                self.focus = Focus::Terminal;
                self.sidebar.set_focused(false);
            }
            SidebarAction::NewThread(project) => {
                self.create_thread(&project);
            }
            SidebarAction::CloseThread(id) => {
                let display = self.workspace.display_name_for(&id).to_string();
                self.open_input(
                    InputPurpose::ConfirmCloseThread,
                    &format!("Close thread {}? (y/n)", display),
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
                    &format!("Remove {} and all threads? (y/n)", name),
                    "",
                    &name,
                );
            }
            SidebarAction::RenameProject(old) => {
                self.open_input(InputPurpose::RenameProject, "Rename project", &old, &old);
            }
            SidebarAction::RenameThread(id) => {
                let current = self.workspace.display_name_for(&id).to_string();
                self.open_input(InputPurpose::RenameThread, "Rename thread", &current, &id);
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
            SidebarAction::MoveThread(id, direction) => {
                self.workspace.swap_thread_in_project(&id, direction);
                self.sidebar.refresh(&self.workspace);
                self.sidebar.set_cursor_to_thread(&id);
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
                if self.active_thread.is_some() {
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
                let thread_ids: Vec<String> =
                    self.workspace.threads.iter().map(|t| t.id.clone()).collect();
                self.panes.clear();
                for id in &thread_ids {
                    let full = config::full_session_name(
                        id,
                        &self.session_prefix,
                        self.mux.tag(),
                    );
                    self.mux.kill_session(&full);
                }
                self.workspace.threads.clear();
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
            Some(InputPurpose::RenameThread) => {
                // The thread's stable id and multiplexer session are NOT touched —
                // only the user-visible display name. Setting `lock=true` permanently
                // disables auto-rename from the agent's OSC title for this thread.
                if let Some(id) = context {
                    let trimmed = value.trim();
                    let current = self.workspace.display_name_for(&id).to_string();
                    if !trimmed.is_empty() && trimmed != current {
                        self.workspace
                            .set_thread_display_name(&id, trimmed, true);
                        self.sidebar.refresh(&self.workspace);
                    }
                }
            }
            Some(InputPurpose::ConfirmCloseThread) => {
                if value == "y" || value == "Y" {
                    if let Some(id) = context {
                        self.close_thread(&id);
                    }
                }
            }
            Some(InputPurpose::ConfirmRemoveProject) => {
                if value == "y" || value == "Y" {
                    if let Some(name) = context {
                        let thread_ids: Vec<String> = self
                            .workspace
                            .threads_for_project(&name)
                            .iter()
                            .map(|t| t.id.clone())
                            .collect();
                        for tid in &thread_ids {
                            let full = config::full_session_name(
                                tid,
                                &self.session_prefix,
                                self.mux.tag(),
                            );
                            self.mux.kill_session(&full);
                            self.detach_pane(tid);
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
            Some(InputPurpose::ConfigureAgentCommand) => {
                if let Some(project) = context {
                    self.workspace.set_project_agent_command(&project, &value);
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
            ConfigOption::AgentCommand => {
                let current_cmd = self
                    .workspace
                    .get_project_agent_command(project)
                    .unwrap_or("")
                    .to_string();
                let title = format!(
                    "Agent command \u{2014} {} (e.g. \"claude\", \"codex\", \"gemini\"). Empty = inherit global.",
                    project
                );
                self.open_input(
                    InputPurpose::ConfigureAgentCommand,
                    &title,
                    &current_cmd,
                    project,
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

    fn create_thread(&mut self, project_name: &str) {
        let thread = match self.workspace.create_thread(project_name) {
            Some(t) => t.clone(),
            None => return,
        };

        let full_name = config::full_session_name(
            &thread.id,
            &self.session_prefix,
            self.mux.tag(),
        );
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

        let (pane_cols, pane_rows) = self.pane_size();
        let agent = effective_agent_command(&self.workspace, &self.config, project_name).to_string();

        self.mux.create_session(
            &full_name,
            &dir,
            &agent,
            pane_rows,
            pane_cols,
        );

        let (cmd, args) = self.mux.attach_command(&full_name);
        if let Ok(pane) = TerminalPane::new(
            thread.id.clone(),
            &cmd,
            &args,
            pane_rows,
            pane_cols,
            self.bell_tx.clone(),
        ) {
            // Run post-attach setup on a background thread (e.g. zellij needs
            // to dismiss popups, split panes, and send the agent command after
            // the PTY connects).
            let mux = multiplexer::backend_for(self.config.multiplexer);
            let setup_name = full_name.clone();
            let setup_dir = dir.clone();
            let setup_agent = agent.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                mux.post_attach_setup(&setup_name, &setup_dir, &setup_agent, pane_rows);
            });
            self.panes.insert(thread.id.clone(), pane);
            self.active_thread = Some(thread.id.clone());
            self.workspace.set_active_thread(Some(&thread.id));
            self.sidebar.set_selected(&thread.id);
            self.sidebar.ensure_expanded(&thread.id, &self.workspace);
            self.focus = Focus::Terminal;
            self.sidebar.set_focused(false);
        }
    }

    fn close_thread(&mut self, id: &str) {
        let full = config::full_session_name(id, &self.session_prefix, self.mux.tag());
        self.mux.kill_session(&full);
        self.detach_pane(id);
        self.workspace.remove_thread(id);
        self.last_seen_title.remove(id);
        self.sidebar.refresh(&self.workspace);
    }

    /// Remove a pane and clear active_thread if it was the one removed.
    fn detach_pane(&mut self, id: &str) {
        self.panes.remove(id);
        if self.active_thread.as_deref() == Some(id) {
            self.active_thread = None;
            self.workspace.set_active_thread(None);
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
                PaneEvent::Bell(thread_id) => {
                    let is_active_and_watching = self.active_thread.as_deref()
                        == Some(&thread_id)
                        && self.focus == Focus::Terminal;
                    if !is_active_and_watching {
                        bell_log(&format!(
                            "bell(pty): thread={} (notifying)",
                            thread_id
                        ));
                        self.sidebar.set_attention(&thread_id);
                        play_bell();
                    }
                }
                PaneEvent::Death(thread_id) => {
                    self.detach_pane(&thread_id);
                    self.sidebar.clear_attention(&thread_id);
                    self.workspace.remove_thread(&thread_id);
                    self.last_seen_title.remove(&thread_id);
                    self.sidebar.refresh(&self.workspace);
                }
            }
        }

        // 500ms tick: poll the multiplexer for bell markers AND for agent pane
        // titles so we can auto-rename threads when the agent advertises a title.
        if !self.panes.is_empty() && self.last_bell_poll.elapsed() >= Duration::from_millis(500) {
            self.last_bell_poll = Instant::now();

            // Bell markers (existing behaviour)
            let flags = self.mux.check_bell_flags(&self.session_name_prefix);
            for (full_name, _) in flags {
                let thread_id = match config::extract_session_id(
                    &full_name,
                    &self.session_prefix,
                    self.mux.tag(),
                ) {
                    Some(id) => id,
                    None => continue,
                };
                let is_active_and_watching = self.active_thread.as_deref()
                    == Some(&thread_id)
                    && self.focus == Focus::Terminal;
                if !is_active_and_watching {
                    bell_log(&format!(
                        "bell(hook): thread={} (notifying)",
                        thread_id
                    ));
                    self.sidebar.set_attention(&thread_id);
                    play_bell();
                }
            }

            // Agent pane title polling
            self.poll_agent_titles();
        }
    }

    /// Walk all attached threads, query the agent pane's OSC title from the
    /// multiplexer, and apply it as the thread's display name when it changes
    /// — unless the user has manually renamed (locked) the thread.
    fn poll_agent_titles(&mut self) {
        let thread_ids: Vec<String> = self.panes.keys().cloned().collect();
        for id in thread_ids {
            let full = config::full_session_name(&id, &self.session_prefix, self.mux.tag());
            let title = match self.mux.agent_pane_title(&full) {
                Some(t) => t,
                None => continue,
            };
            let cleaned = clean_agent_title(&title);
            if cleaned.is_empty() {
                continue;
            }
            // No-op fast-path: skip if the cleaned title hasn't changed.
            // Without this, a cycling spinner glyph in the raw title would
            // trigger a rename + YAML save every 500ms.
            if self
                .last_seen_title
                .get(&id)
                .map(|s| s.as_str() == cleaned.as_str())
                .unwrap_or(false)
            {
                continue;
            }
            self.last_seen_title.insert(id.clone(), cleaned.clone());

            // Skip if the user has locked the name.
            if self.workspace.is_thread_name_locked(&id) {
                continue;
            }
            // Skip if the title is just the thread id, or already the display name.
            if cleaned == id {
                continue;
            }
            if self.workspace.display_name_for(&id) == cleaned {
                continue;
            }
            // Don't regress: a generic agent label ("Claude Code", "codex", …)
            // is allowed as the *initial* auto-name (when nothing's been set yet,
            // or when the current name is itself generic), but once a thread has
            // a real, specific name we never swap it back to a generic one.
            // Use the per-project effective agent so a `codex` project doesn't
            // get a "Claude Code" title frozen as its name (and vice versa).
            let project = self
                .workspace
                .threads
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.project.clone())
                .unwrap_or_default();
            let project_agent =
                effective_agent_command(&self.workspace, &self.config, &project).to_string();
            if is_generic_agent_title(&cleaned, &project_agent) {
                let current = self.workspace.get_thread_name(&id);
                let current_is_generic = current
                    .map(|c| is_generic_agent_title(c, &project_agent))
                    .unwrap_or(true);
                if !current_is_generic {
                    continue;
                }
            }
            self.workspace
                .set_thread_display_name(&id, &cleaned, false);
            self.sidebar.refresh(&self.workspace);
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
            .active_thread
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

            let id = self.active_thread.as_ref().unwrap();
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
        let thread_id = match &self.active_thread {
            Some(id) => id.clone(),
            None => return,
        };
        let pane = match self.panes.get_mut(&thread_id) {
            Some(p) => p,
            None => return,
        };

        let col = mouse.column.saturating_sub(SIDEBAR_WIDTH + 1) + 1;
        let row = mouse.row.saturating_sub(1) + 1;

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

/// Returns the agent command that should be used for a given project: the
/// per-project override if set, otherwise the global default from config.
fn effective_agent_command<'a>(
    workspace: &'a Workspace,
    config: &'a Config,
    project: &str,
) -> &'a str {
    workspace
        .get_project_agent_command(project)
        .unwrap_or(&config.coding_agent_command)
}

/// Strip leading non-alphanumeric characters from an agent's terminal title
/// so the displayed thread name stays stable across spinner glyphs (braille
/// `⠂` while thinking, `✳` while idle, `.` while loading, etc.). Trailing
/// whitespace is also removed.
fn clean_agent_title(s: &str) -> String {
    s.trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim_end()
        .to_string()
}

/// Generic agent labels that we don't want to overwrite an already-meaningful
/// thread name with. Comparison is case-insensitive against this set plus the
/// first word of the user's `coding_agent_command` config.
fn is_generic_agent_title(title: &str, agent_command: &str) -> bool {
    const STATIC: &[&str] = &[
        "claude code",
        "claude",
        "codex",
        "gemini",
        "aider",
        "cursor",
    ];
    let lower = title.trim().to_ascii_lowercase();
    if STATIC.iter().any(|s| *s == lower) {
        return true;
    }
    if let Some(first) = agent_command.split_whitespace().next() {
        if first.to_ascii_lowercase() == lower {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod title_tests {
    use super::{clean_agent_title, is_generic_agent_title};

    #[test]
    fn strips_braille_spinner_prefix() {
        assert_eq!(clean_agent_title("\u{2802} hello"), "hello");
        assert_eq!(clean_agent_title("\u{2810} my-task"), "my-task");
        assert_eq!(clean_agent_title("  \u{2800}  spaced  "), "spaced");
    }

    #[test]
    fn strips_other_glyph_prefixes() {
        // U+2733 ✳ (Claude idle), U+25CF ●, U+25C6 ◆, plus dots/punctuation
        assert_eq!(clean_agent_title("\u{2733} Claude Code"), "Claude Code");
        assert_eq!(clean_agent_title("\u{25CF} working"), "working");
        assert_eq!(clean_agent_title("... loading"), "loading");
        assert_eq!(clean_agent_title("  ."), "");
    }

    #[test]
    fn passes_through_alphanumeric_start() {
        assert_eq!(clean_agent_title("Refactoring auth"), "Refactoring auth");
        // Note: leading punctuation is intentionally stripped.
        assert_eq!(clean_agent_title("[WIP] foo"), "WIP] foo");
    }

    #[test]
    fn empty_after_strip_is_empty() {
        assert_eq!(clean_agent_title("\u{2800}\u{28FF}"), "");
        assert_eq!(clean_agent_title("   "), "");
    }

    #[test]
    fn detects_generic_titles() {
        assert!(is_generic_agent_title("Claude Code", "claude"));
        assert!(is_generic_agent_title("claude code", "claude"));
        assert!(is_generic_agent_title("CLAUDE", "claude"));
        assert!(is_generic_agent_title("codex", "claude"));
        // Configured agent command's first word counts too
        assert!(is_generic_agent_title("myagent", "myagent --foo"));
        // Real titles are not generic
        assert!(!is_generic_agent_title("Refactoring auth", "claude"));
        assert!(!is_generic_agent_title("threads-auto-rename", "claude"));
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
