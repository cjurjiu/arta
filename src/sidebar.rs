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
    CopyGithubLink,
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
    scroll_offset: usize,
    expanded: HashSet<String>,
    selected: Option<String>,
    attention: HashSet<String>,
    nerd_font: bool,
    focused: bool,
    prefix_active: bool,
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
            scroll_offset: 0,
            expanded,
            selected: None,
            attention: HashSet::new(),
            nerd_font: detect_nerd_font(),
            focused: true,
            prefix_active: false,
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

    pub fn set_prefix_active(&mut self, active: bool) {
        self.prefix_active = active;
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
                self.ensure_cursor_visible();
                return;
            }
        }
    }

    pub fn set_cursor_to_session(&mut self, id: &str) {
        for (i, item) in self.items.iter().enumerate() {
            if matches!(item, SidebarItem::Session { id: sid, .. } if sid == id) {
                self.cursor = i;
                self.ensure_cursor_visible();
                return;
            }
        }
    }

    fn item_line_height(item: &SidebarItem) -> usize {
        match item {
            SidebarItem::Project { .. } => 2,
            SidebarItem::Session { .. } => 1,
        }
    }

    fn item_start_line(&self, idx: usize) -> usize {
        self.items[..idx]
            .iter()
            .map(Self::item_line_height)
            .sum()
    }

    /// Header: 5 lines, footer: 7 lines (fixed, keeps items area consistent)
    fn visible_item_lines(&self) -> usize {
        (self.height as usize).saturating_sub(12)
    }

    fn ensure_cursor_visible(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let visible = self.visible_item_lines();
        if visible == 0 {
            return;
        }
        let cursor_start = self.item_start_line(self.cursor);
        let cursor_end = cursor_start + Self::item_line_height(&self.items[self.cursor]);
        if cursor_start < self.scroll_offset {
            self.scroll_offset = cursor_start;
        }
        if cursor_end > self.scroll_offset + visible {
            self.scroll_offset = cursor_end.saturating_sub(visible);
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
        self.ensure_cursor_visible();
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

    /// Handles navigation-only keys (sidebar must be focused).
    /// Command keys (a, d, n, r, o, c, g, q, Q) require Ctrl+Space prefix.
    pub fn handle_key(&mut self, key: &KeyEvent, workspace: &Workspace) -> SidebarAction {
        if !self.focused {
            return SidebarAction::None;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.cursor < self.items.len().saturating_sub(1) {
                    self.cursor += 1;
                    self.ensure_cursor_visible();
                }
                SidebarAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.ensure_cursor_visible();
                }
                SidebarAction::None
            }
            KeyCode::Char('J') => self.with_current_item(|item| match item {
                SidebarItem::Project { .. } => SidebarAction::MoveProject(1),
                SidebarItem::Session { id, .. } => SidebarAction::MoveSession(id.clone(), 1),
            }),
            KeyCode::Char('K') => self.with_current_item(|item| match item {
                SidebarItem::Project { .. } => SidebarAction::MoveProject(-1),
                SidebarItem::Session { id, .. } => SidebarAction::MoveSession(id.clone(), -1),
            }),
            KeyCode::Enter => self.handle_action(workspace),
            KeyCode::Char('l') => SidebarAction::FocusTerminal,
            KeyCode::Tab => {
                self.handle_toggle(workspace);
                SidebarAction::None
            }
            _ => SidebarAction::None,
        }
    }

    fn with_current_item(&self, f: impl FnOnce(&SidebarItem) -> SidebarAction) -> SidebarAction {
        match self.current_item() {
            Some(item) => f(item),
            None => SidebarAction::None,
        }
    }

    /// Handle a key triggered via Ctrl+Space prefix (no focus check, action keys only).
    pub fn handle_prefix_key(&self, key: &KeyEvent) -> SidebarAction {
        match key.code {
            KeyCode::Char('a') => SidebarAction::AddProject,
            KeyCode::Char('d') => self.with_current_item(|item| match item {
                SidebarItem::Session { id, .. } => SidebarAction::CloseSession(id.clone()),
                SidebarItem::Project { name } => SidebarAction::RemoveProject(name.clone()),
            }),
            KeyCode::Char('n') => self.with_current_item(|item| match item {
                SidebarItem::Project { name } => SidebarAction::NewSession(name.clone()),
                SidebarItem::Session { project, .. } => SidebarAction::NewSession(project.clone()),
            }),
            KeyCode::Char('r') => self.with_current_item(|item| match item {
                SidebarItem::Project { name } => SidebarAction::RenameProject(name.clone()),
                SidebarItem::Session { id, .. } => SidebarAction::RenameSession(id.clone()),
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
            KeyCode::Char('g') => SidebarAction::CopyGithubLink,
            KeyCode::Char('q') => SidebarAction::Quit,
            KeyCode::Char('Q') => SidebarAction::CleanExit,
            _ => SidebarAction::None,
        }
    }

    pub fn handle_mouse_click(&mut self, y: u16, workspace: &Workspace) -> SidebarAction {
        if !self.focused || y < 5 {
            return SidebarAction::None;
        }
        // Convert screen y to virtual line: screen_y = 5 + (vline - scroll_offset)
        // so vline = (screen_y - 5) + scroll_offset
        // Header is 5 lines: top separator, blank, "arta", blank, separator
        let click_vline = (y as usize - 5) + self.scroll_offset;
        let mut vline: usize = 0;
        for (i, item) in self.items.iter().enumerate() {
            let h = Self::item_line_height(item);
            if click_vline < vline + h {
                self.cursor = i;
                self.ensure_cursor_visible();
                return self.handle_action(workspace);
            }
            vline += h;
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

        // Top separator — matches code pane top border
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

        y += 1; // empty line below top separator

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

        // Items area (7 lines always reserved for footer)
        let items_top_y = y;
        let footer_y = (area.y + area.height).saturating_sub(7);
        let visible_lines = (footer_y as usize).saturating_sub(items_top_y as usize);

        if self.items.is_empty() {
            if y + 1 < footer_y {
                y += 1;
                buf.set_line(
                    area.x,
                    y,
                    &Line::from(Span::styled("  No projects yet.", dim)),
                    area.width,
                );
                y += 1;
            }
            if y < footer_y {
                buf.set_line(
                    area.x,
                    y,
                    &Line::from(Span::styled("  Press 'a' to add one.", dim)),
                    area.width,
                );
            }
        } else {
            let mut vline: usize = 0;
            for (idx, item) in self.items.iter().enumerate() {
                let item_h = Self::item_line_height(item);

                // Skip items fully above scroll window
                if vline + item_h <= self.scroll_offset {
                    vline += item_h;
                    continue;
                }
                // Stop if past visible area
                if vline >= self.scroll_offset + visible_lines {
                    break;
                }

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

                        // Blank line (vline), name line (vline+1)
                        let name_vline = vline + 1;
                        if name_vline >= self.scroll_offset
                            && name_vline < self.scroll_offset + visible_lines
                        {
                            let sy =
                                items_top_y + (name_vline - self.scroll_offset) as u16;
                            let mut spans =
                                vec![Span::raw(format!(" {} {}", arrow, name))];
                            if count > 0 {
                                spans.push(Span::styled(format!(" ({})", count), dim));
                            }
                            let mut style = bold;
                            if self.focused && self.cursor == idx {
                                style = style.add_modifier(Modifier::REVERSED);
                            }
                            buf.set_line(
                                area.x,
                                sy,
                                &Line::from(spans).style(style),
                                area.width,
                            );
                        }
                    }
                    SidebarItem::Session { id, .. } => {
                        if vline >= self.scroll_offset
                            && vline < self.scroll_offset + visible_lines
                        {
                            let sy = items_top_y + (vline - self.scroll_offset) as u16;

                            let is_selected =
                                self.selected.as_deref() == Some(id.as_str());
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

                            let text = format!("   {} {}", icon, id);
                            let padded = format!("{:<width$}", text, width = w);
                            buf.set_line(
                                area.x,
                                sy,
                                &Line::from(Span::styled(padded, style)),
                                area.width,
                            );
                        }
                    }
                }

                vline += item_h;
            }
        }

        // Footer (content anchored to bottom, separator above content)
        if self.prefix_active {
            // 7 lines: separator + 6 command lines
            y = (area.y + area.height).saturating_sub(7);
            if y < area.y + area.height {
                let rest = "\u{2500}".repeat(w.saturating_sub(2));
                buf.set_line(
                    area.x,
                    y,
                    &Line::from(Span::styled(format!(" {} ", rest), sep_style)),
                    area.width,
                );
                y += 1;
            }
            let footer_lines: Vec<Vec<Span>> = vec![
                vec![
                    Span::styled(" \u{2190}/\u{2192}", bold),
                    Span::styled(" focus  ", dim),
                    Span::styled("n", bold),
                    Span::styled(" new thread", dim),
                ],
                vec![
                    Span::styled(" o", bold),
                    Span::styled(" open ide  ", dim),
                    Span::styled("r", bold),
                    Span::styled(" rename", dim),
                ],
                vec![
                    Span::styled(" a", bold),
                    Span::styled(" add project  ", dim),
                    Span::styled("c", bold),
                    Span::styled(" config", dim),
                ],
                vec![
                    Span::styled(" d", bold),
                    Span::styled(" delete  ", dim),
                    Span::styled("g", bold),
                    Span::styled(" github", dim),
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
        } else {
            // 3 lines: separator + 2 hint lines (anchored to bottom)
            y = (area.y + area.height).saturating_sub(3);
            if y < area.y + area.height {
                if self.focused {
                    let rest = "\u{2500}".repeat(w.saturating_sub(10));
                    buf.set_line(
                        area.x,
                        y,
                        &Line::from(vec![
                            Span::styled(" focused ", sep_style),
                            Span::styled(rest, sep_style),
                        ]),
                        area.width,
                    );
                } else {
                    let rest = "\u{2500}".repeat(w.saturating_sub(2));
                    buf.set_line(
                        area.x,
                        y,
                        &Line::from(Span::styled(format!(" {} ", rest), sep_style)),
                        area.width,
                    );
                }
                y += 1;
            }
            let footer_lines: Vec<Vec<Span>> = vec![
                vec![
                    Span::styled(" ctrl+space", bold),
                    Span::styled(" run commands", dim),
                ],
                vec![
                    Span::styled(" J/K", bold),
                    Span::styled(" reorder", dim),
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
}

fn detect_nerd_font() -> bool {
    Command::new("fc-list")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Nerd Font"))
        .unwrap_or(false)
}
