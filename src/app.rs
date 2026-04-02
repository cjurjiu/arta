use std::collections::HashMap;
use std::sync::mpsc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
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
    RenameProject,
    RenameSession,
    ConfirmCloseSession,
    ConfirmRemoveProject,
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
    prefix_active: bool,
    bell_tx: mpsc::Sender<PaneEvent>,
    bell_rx: mpsc::Receiver<PaneEvent>,
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
        let pane_height = term_h.saturating_sub(2).max(5);

        // Eagerly attach PTYs for all surviving sessions
        let mut panes = HashMap::new();
        for session in &workspace.sessions {
            let tmux_name = tmux::session_name(&session.id);
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

        App {
            sidebar,
            panes,
            input_panel,
            workspace,
            focus: Focus::Sidebar,
            active_session: None,
            input_purpose: None,
            input_context: None,
            pending_path: None,
            prefix_active: false,
            bell_tx,
            bell_rx,
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
                if self.input_panel.is_active() {
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
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Input panel gets priority
        if self.input_panel.is_active() {
            let action = self.input_panel.handle_key(&key);
            match action {
                InputAction::Submit(value) => self.handle_input_submit(value),
                InputAction::Cancel => {
                    self.input_purpose = None;
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
                // All sidebar commands work from any focus via prefix
                _ => {
                    let action = self.sidebar.handle_key(&key, &self.workspace);
                    self.process_sidebar_action(action);
                    return;
                }
            }
        }

        // Ctrl+Space activates prefix
        if key.code == KeyCode::Char(' ') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.prefix_active = true;
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
                self.active_session = Some(id);
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
                        }
                    }
                }
                self.sidebar.refresh(&self.workspace);
            }
            SidebarAction::MoveSession(id, direction) => {
                self.workspace.swap_session_in_project(&id, direction);
                self.sidebar.refresh(&self.workspace);
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
                    let path = self.pending_path.take().unwrap_or_default();
                    self.workspace.add_project(&name, &path);
                    self.sidebar.refresh(&self.workspace);
                }
                self.pending_path = None;
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
                        tmux::rename_session(
                            &tmux::session_name(&old_id),
                            &tmux::session_name(&new_id),
                        );
                        self.workspace.rename_session(&old_id, &new_id);

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
            None => {}
        }

        self.focus = Focus::Sidebar;
        self.sidebar.set_focused(true);
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
        }
    }

    pub fn check_pane_events(&mut self) {
        while let Ok(event) = self.bell_rx.try_recv() {
            match event {
                PaneEvent::Bell(session_id) => {
                    if self.active_session.as_deref() != Some(&session_id) {
                        self.sidebar.set_attention(&session_id);
                        play_bell();
                    }
                }
                PaneEvent::Death(session_id) => {
                    self.detach_pane(&session_id);
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

            if term_focused {
                let term_border = "\u{2500}".repeat(right_width as usize);
                frame.buffer_mut().set_line(
                    right_x,
                    0,
                    &Line::from(Span::styled(&term_border, focus_red_bold)),
                    right_width,
                );
            }

            // Terminal content fills available space
            let term_content_y: u16 = 1; // 1 line top padding
            let reserved: u16 = if term_focused { 2 } else { 1 };
            let term_content_height = main_height.saturating_sub(reserved);
            let term_area = Rect::new(right_x, term_content_y, right_width, term_content_height);

            let id = self.active_session.as_ref().unwrap();
            self.panes
                .get(id.as_str())
                .unwrap()
                .render(term_area, frame.buffer_mut());

            if term_focused {
                let term_border = "\u{2500}".repeat(right_width as usize);
                let bottom_y = main_height.saturating_sub(1);
                frame.buffer_mut().set_line(
                    right_x,
                    bottom_y,
                    &Line::from(Span::styled(&term_border, focus_red_bold)),
                    right_width,
                );
            }
        } else {
            let right_area = Rect::new(right_x, 0, right_width, main_height);
            welcome::render_welcome(right_area, frame.buffer_mut());
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
    }

    fn pane_size(&self) -> (u16, u16) {
        let right_width = self.width.saturating_sub(SIDEBAR_WIDTH + 1);
        let mut main_height = self.height;
        if self.input_panel.is_active() {
            main_height = main_height.saturating_sub(INPUT_PANEL_HEIGHT + 1);
        }
        // 1 line top padding + 1 line bottom border (when focused)
        let pane_height = main_height.saturating_sub(2);
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

fn expand_tilde(path: &str) -> String {
    match path.strip_prefix('~') {
        Some(rest) => format!("{}{}", home_dir_string(), rest),
        None => path.to_string(),
    }
}

fn play_bell() {
    let _ = std::process::Command::new("afplay")
        .args(["-v", "0.5", "/System/Library/Sounds/Tink.aiff"])
        .spawn();
}
