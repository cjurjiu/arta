use crate::workspace::Workspace;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashSet;
use std::process::Command;

pub enum SidebarAction {
    None,
    SelectSession(String),
    NewSession(String),
    CloseSession(String),
    AddProject,
    RemoveProject(String),
    RenameProject(String),
    RenameSession(String),
    MoveProject(i32),
    MoveSession(String, i32),
    OpenIde(String),
    ConfigureProject(String),
    FocusTerminal,
    Quit,
    CleanExit,
}

#[derive(Clone, Debug)]
enum SidebarItem {
    Project { name: String },
    Session { id: String, project: String },
}

pub struct Sidebar {
    items: Vec<SidebarItem>,
    cursor: usize,
    expanded: HashSet<String>,
    selected: Option<String>,
    attention: HashSet<String>,
    nerd_font: bool,
    focused: bool,
    height: u16,
}

impl Sidebar {
    pub fn new(workspace: &Workspace) -> Self {
        let mut expanded = HashSet::new();
        for p in &workspace.projects {
            expanded.insert(p.name.clone());
        }
        let mut s = Sidebar {
            items: Vec::new(),
            cursor: 0,
            expanded,
            selected: None,
            attention: HashSet::new(),
            nerd_font: detect_nerd_font(),
            focused: true,
            height: 24,
        };
        s.rebuild_items(workspace);
        s
    }

    pub fn set_size(&mut self, _w: u16, h: u16) {
        self.height = h;
    }

    pub fn set_focused(&mut self, f: bool) {
        self.focused = f;
    }

    pub fn nerd_font(&self) -> bool {
        self.nerd_font
    }

    pub fn set_selected(&mut self, id: &str) {
        self.selected = Some(id.to_string());
        self.attention.remove(id);
    }

    pub fn set_attention(&mut self, id: &str) {
        if self.selected.as_deref() != Some(id) {
            self.attention.insert(id.to_string());
        }
    }

    pub fn clear_attention(&mut self, id: &str) {
        self.attention.remove(id);
    }

    pub fn refresh(&mut self, workspace: &Workspace) {
        self.rebuild_items(workspace);
    }

    pub fn set_cursor_to_project(&mut self, name: &str) {
        for (i, item) in self.items.iter().enumerate() {
            if matches!(item, SidebarItem::Project { name: n } if n == name) {
                self.cursor = i;
                return;
            }
        }
    }

    pub fn set_cursor_to_session(&mut self, id: &str) {
        for (i, item) in self.items.iter().enumerate() {
            if matches!(item, SidebarItem::Session { id: sid, .. } if sid == id) {
                self.cursor = i;
                return;
            }
        }
    }

    fn rebuild_items(&mut self, workspace: &Workspace) {
        self.items.clear();
        for p in &workspace.projects {
            self.items.push(SidebarItem::Project {
                name: p.name.clone(),
            });
            if self.expanded.contains(&p.name) {
                for s in workspace.sessions_for_project(&p.name) {
                    self.items.push(SidebarItem::Session {
                        id: s.id.clone(),
                        project: p.name.clone(),
                    });
                }
            }
        }
        if self.cursor >= self.items.len() && !self.items.is_empty() {
            self.cursor = self.items.len() - 1;
        }
    }

    fn current_item(&self) -> Option<&SidebarItem> {
        self.items.get(self.cursor)
    }

    pub fn get_cursor_project(&self) -> Option<&str> {
        self.current_item().map(|item| match item {
            SidebarItem::Project { name } => name.as_str(),
            SidebarItem::Session { project, .. } => project.as_str(),
        })
    }

    /// Ensure the project containing the given session is expanded.
    pub fn ensure_expanded(&mut self, session_id: &str, workspace: &Workspace) {
        if let Some(session) = workspace.sessions.iter().find(|s| s.id == session_id) {
            self.expanded.insert(session.project.clone());
            self.rebuild_items(workspace);
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent, workspace: &Workspace) -> SidebarAction {
        if !self.focused {
            return SidebarAction::None;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.cursor < self.items.len().saturating_sub(1) {
                    self.cursor += 1;
                }
                SidebarAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                SidebarAction::None
            }
            KeyCode::Enter => self.handle_action(workspace),
            KeyCode::Char('l') => SidebarAction::FocusTerminal,
            KeyCode::Tab => {
                self.handle_toggle(workspace);
                SidebarAction::None
            }
            KeyCode::Char('a') => SidebarAction::AddProject,
            KeyCode::Char('n') => self.with_current_item(|item| match item {
                SidebarItem::Project { name } => SidebarAction::NewSession(name.clone()),
                SidebarItem::Session { project, .. } => SidebarAction::NewSession(project.clone()),
            }),
            KeyCode::Char('d') => self.with_current_item(|item| match item {
                SidebarItem::Session { id, .. } => SidebarAction::CloseSession(id.clone()),
                _ => SidebarAction::None,
            }),
            KeyCode::Char('D') => self.with_current_item(|item| match item {
                SidebarItem::Project { name } => SidebarAction::RemoveProject(name.clone()),
                _ => SidebarAction::None,
            }),
            KeyCode::Char('r') => self.with_current_item(|item| match item {
                SidebarItem::Project { name } => SidebarAction::RenameProject(name.clone()),
                SidebarItem::Session { id, .. } => SidebarAction::RenameSession(id.clone()),
            }),
            KeyCode::Char('J') => self.with_current_item(|item| match item {
                SidebarItem::Project { .. } => SidebarAction::MoveProject(1),
                SidebarItem::Session { id, .. } => SidebarAction::MoveSession(id.clone(), 1),
            }),
            KeyCode::Char('K') => self.with_current_item(|item| match item {
                SidebarItem::Project { .. } => SidebarAction::MoveProject(-1),
                SidebarItem::Session { id, .. } => SidebarAction::MoveSession(id.clone(), -1),
            }),
            KeyCode::Char('o') => self.with_current_item(|item| match item {
                SidebarItem::Project { name } => SidebarAction::OpenIde(name.clone()),
                SidebarItem::Session { project, .. } => SidebarAction::OpenIde(project.clone()),
            }),
            KeyCode::Char('c') => self.with_current_item(|item| match item {
                SidebarItem::Project { name } => SidebarAction::ConfigureProject(name.clone()),
                SidebarItem::Session { project, .. } => {
                    SidebarAction::ConfigureProject(project.clone())
                }
            }),
            KeyCode::Char('q') => SidebarAction::Quit,
            KeyCode::Char('Q') => SidebarAction::CleanExit,
            _ => SidebarAction::None,
        }
    }

    fn with_current_item(&self, f: impl FnOnce(&SidebarItem) -> SidebarAction) -> SidebarAction {
        match self.current_item() {
            Some(item) => f(item),
            None => SidebarAction::None,
        }
    }

    pub fn handle_mouse_click(&mut self, y: u16, workspace: &Workspace) -> SidebarAction {
        if !self.focused {
            return SidebarAction::None;
        }
        let mut line: u16 = 4;
        for (i, item) in self.items.iter().enumerate() {
            match item {
                SidebarItem::Project { .. } => line += 2,
                SidebarItem::Session { .. } => line += 1,
            }
            if line > y && y > 0 {
                self.cursor = i;
                return self.handle_action(workspace);
            }
        }
        SidebarAction::None
    }

    fn handle_action(&mut self, workspace: &Workspace) -> SidebarAction {
        let item = match self.current_item() {
            Some(item) => item.clone(),
            None => return SidebarAction::None,
        };
        match item {
            SidebarItem::Project { .. } => {
                self.handle_toggle(workspace);
                SidebarAction::None
            }
            SidebarItem::Session { id, .. } => SidebarAction::SelectSession(id),
        }
    }

    fn handle_toggle(&mut self, workspace: &Workspace) {
        if let Some(SidebarItem::Project { name }) = self.current_item().cloned() {
            if !self.expanded.remove(&name) {
                self.expanded.insert(name);
            }
            self.rebuild_items(workspace);
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, workspace: &Workspace) {
        let dim = Style::default().add_modifier(Modifier::DIM);
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let sep_style = if self.focused {
            Style::default()
                .fg(Color::Rgb(0xFF, 0x6C, 0x6B))
                .add_modifier(Modifier::BOLD)
        } else {
            dim
        };

        let w = area.width as usize;
        let mut y = area.y;

        // Header
        y += 1;

        let icon_style = Style::default()
            .fg(Color::Rgb(0xEC, 0xBE, 0x7B))
            .add_modifier(Modifier::BOLD);
        let name_style = Style::default()
            .fg(Color::Rgb(0xFF, 0x6C, 0x6B))
            .add_modifier(Modifier::BOLD);

        let icon = if self.nerd_font {
            "\u{f03e}  "
        } else {
            "\u{1f5bc}\u{fe0f}  "
        };
        let header_text = "a r t a";
        let header_len = 3 + header_text.len();
        let h_pad = w.saturating_sub(header_len) / 2;

        if y < area.y + area.height {
            let line = Line::from(vec![
                Span::raw(" ".repeat(h_pad)),
                Span::styled(icon, icon_style),
                Span::styled(header_text, name_style),
            ]);
            buf.set_line(area.x, y, &line, area.width);
            y += 1;
        }

        // Separator — red when focused, dim when not
        y += 1;
        if y < area.y + area.height {
            let sep = "\u{2500}".repeat(w.saturating_sub(2));
            let line = Line::from(Span::styled(format!(" {} ", sep), sep_style));
            buf.set_line(area.x, y, &line, area.width);
            y += 1;
        }

        if self.items.is_empty() {
            y += 1;
            if y < area.y + area.height {
                buf.set_line(
                    area.x,
                    y,
                    &Line::from(Span::styled("  No projects yet.", dim)),
                    area.width,
                );
                y += 1;
            }
            if y < area.y + area.height {
                buf.set_line(
                    area.x,
                    y,
                    &Line::from(Span::styled("  Press 'a' to add one.", dim)),
                    area.width,
                );
            }
        } else {
            for (idx, item) in self.items.iter().enumerate() {
                match item {
                    SidebarItem::Project { name } => {
                        let arrow = if self.expanded.contains(name) {
                            if self.nerd_font {
                                "\u{f115}"
                            } else {
                                "\u{25bc}"
                            }
                        } else if self.nerd_font {
                            "\u{f114}"
                        } else {
                            "\u{25b6}"
                        };

                        let count = workspace.sessions_for_project(name).len();

                        y += 1;
                        if y >= area.y + area.height {
                            break;
                        }

                        let mut spans = vec![Span::raw(format!(" {} {}", arrow, name))];
                        if count > 0 {
                            spans.push(Span::styled(format!(" ({})", count), dim));
                        }

                        let mut style = bold;
                        if self.focused && self.cursor == idx {
                            style = style.add_modifier(Modifier::REVERSED);
                        }

                        buf.set_line(area.x, y, &Line::from(spans).style(style), area.width);
                        y += 1;
                    }
                    SidebarItem::Session { id, .. } => {
                        if y >= area.y + area.height {
                            break;
                        }

                        let is_selected = self.selected.as_deref() == Some(id.as_str());
                        let has_attention = self.attention.contains(id);

                        let icon = if has_attention {
                            if self.nerd_font {
                                "\u{f0f3}"
                            } else {
                                "*"
                            }
                        } else if is_selected {
                            if self.nerd_font {
                                "\u{f120}"
                            } else {
                                "\u{25cf}"
                            }
                        } else if self.nerd_font {
                            "\u{f489}"
                        } else {
                            "\u{25cb}"
                        };

                        let color = if has_attention {
                            Color::Rgb(0xFF, 0x6C, 0x6B)
                        } else if is_selected {
                            Color::Rgb(0x51, 0xAF, 0xEF)
                        } else {
                            Color::Rgb(0xA9, 0xA1, 0xE1)
                        };

                        let mut style = Style::default().fg(color);
                        if is_selected {
                            style = style
                                .add_modifier(Modifier::BOLD)
                                .bg(Color::Rgb(0x1E, 0x2A, 0x3A));
                        }
                        if self.focused && self.cursor == idx {
                            style = style.add_modifier(Modifier::REVERSED);
                        }

                        // Fill the full line width so the background extends
                        let text = format!("   {} {}", icon, id);
                        let padded = format!("{:<width$}", text, width = w);
                        buf.set_line(
                            area.x,
                            y,
                            &Line::from(Span::styled(padded, style)),
                            area.width,
                        );
                        y += 1;
                    }
                }
            }
        }

        // Footer
        let footer_y = area.y + area.height - 7;
        if footer_y > y {
            y = footer_y;
        }

        if y < area.y + area.height {
            let sep = "\u{2500}".repeat(w.saturating_sub(2));
            buf.set_line(
                area.x,
                y,
                &Line::from(Span::styled(format!(" {} ", sep), sep_style)),
                area.width,
            );
            y += 1;
        }

        let footer_lines = [
            vec![
                Span::styled(" a", bold),
                Span::styled(" add project  ", dim),
                Span::styled("D", bold),
                Span::styled(" remove", dim),
            ],
            vec![
                Span::styled(" n", bold),
                Span::styled(" new thread  ", dim),
                Span::styled("d", bold),
                Span::styled(" delete", dim),
            ],
            vec![
                Span::styled(" r", bold),
                Span::styled(" rename  ", dim),
                Span::styled("J/K", bold),
                Span::styled(" reorder", dim),
            ],
            vec![
                Span::styled(" o", bold),
                Span::styled(" open ide  ", dim),
                Span::styled("c", bold),
                Span::styled(" configure", dim),
            ],
            vec![
                Span::styled(" q", bold),
                Span::styled(" quit  ", dim),
                Span::styled("Q", bold),
                Span::styled(" clean exit", dim),
            ],
        ];

        for spans in &footer_lines {
            if y < area.y + area.height {
                buf.set_line(area.x, y, &Line::from(spans.clone()), area.width);
                y += 1;
            }
        }
    }
}

fn detect_nerd_font() -> bool {
    Command::new("fc-list")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Nerd Font"))
        .unwrap_or(false)
}
