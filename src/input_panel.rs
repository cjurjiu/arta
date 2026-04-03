use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::fs;

#[derive(Clone, Copy, PartialEq)]
pub enum InputMode {
    Text,
    Path,
}

pub enum InputAction {
    None,
    Submit(String),
    Cancel,
}

struct DirEntry {
    name: String,
    is_dir: bool,
}

pub struct InputPanel {
    mode: InputMode,
    title: String,
    value: String,
    cursor_pos: usize,
    active: bool,
    width: u16,
    height: u16,
    dir_entries: Vec<DirEntry>,
    dir_cursor: usize,
    dir_scroll: usize,
}

impl InputPanel {
    pub fn new() -> Self {
        InputPanel {
            mode: InputMode::Text,
            title: String::new(),
            value: String::new(),
            cursor_pos: 0,
            active: false,
            width: 80,
            height: 15,
            dir_entries: Vec::new(),
            dir_cursor: 0,
            dir_scroll: 0,
        }
    }

    pub fn activate(
        &mut self,
        mode: InputMode,
        title: &str,
        initial_value: &str,
        width: u16,
        height: u16,
    ) {
        self.mode = mode;
        self.title = title.to_string();
        self.value = initial_value.to_string();
        self.cursor_pos = self.value.len();
        self.active = true;
        self.width = width;
        self.height = height;
        self.dir_cursor = 0;
        self.dir_scroll = 0;
        if mode == InputMode::Path {
            self.update_dir_listing();
        }
    }

    pub fn deactivate(&mut self) {
        self.active = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn cursor_position(&self, area: Rect) -> (u16, u16) {
        // Title is on line 1 (after border), input on line 2
        let x = area.x + self.cursor_pos as u16;
        let y = area.y + 2;
        (x, y)
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> InputAction {
        if !self.active {
            return InputAction::None;
        }

        match key.code {
            KeyCode::Esc => {
                self.deactivate();
                InputAction::Cancel
            }

            KeyCode::Enter => {
                if self.mode == InputMode::Path && !self.dir_entries.is_empty() {
                    let entry = &self.dir_entries[self.dir_cursor];
                    if entry.is_dir {
                        let current = self.expand_path(&self.value);
                        let dir = if current.ends_with('/') {
                            current
                        } else {
                            match current.rfind('/') {
                                Some(i) => current[..=i].to_string(),
                                None => current,
                            }
                        };
                        let new_path = format!("{}{}/", dir, entry.name);
                        self.value = new_path;
                        self.cursor_pos = self.value.len();
                        self.dir_cursor = 0;
                        self.dir_scroll = 0;
                        self.update_dir_listing();
                        return InputAction::None;
                    }
                }
                let val = self.value.clone();
                self.deactivate();
                InputAction::Submit(val)
            }

            KeyCode::Tab => {
                if self.mode == InputMode::Path {
                    self.tab_complete();
                }
                InputAction::None
            }

            KeyCode::Up => {
                if self.mode == InputMode::Path && self.dir_cursor > 0 {
                    self.dir_cursor -= 1;
                    if self.dir_cursor < self.dir_scroll {
                        self.dir_scroll = self.dir_cursor;
                    }
                }
                InputAction::None
            }

            KeyCode::Down => {
                if self.mode == InputMode::Path
                    && self.dir_cursor < self.dir_entries.len().saturating_sub(1)
                {
                    self.dir_cursor += 1;
                    let max_visible = self.max_visible_entries();
                    if self.dir_cursor >= self.dir_scroll + max_visible {
                        self.dir_scroll = self.dir_cursor - max_visible + 1;
                    }
                }
                InputAction::None
            }

            KeyCode::Char(c) => {
                self.value.insert(self.cursor_pos, c);
                self.cursor_pos += c.len_utf8();
                if self.mode == InputMode::Path {
                    self.update_dir_listing();
                }
                InputAction::None
            }

            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    // Find the previous char boundary
                    let prev = self.value[..self.cursor_pos]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.value.remove(prev);
                    self.cursor_pos = prev;
                    if self.mode == InputMode::Path {
                        self.update_dir_listing();
                    }
                }
                InputAction::None
            }

            KeyCode::Left => {
                if self.cursor_pos > 0 {
                    self.cursor_pos = self.value[..self.cursor_pos]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                InputAction::None
            }

            KeyCode::Right => {
                if self.cursor_pos < self.value.len() {
                    self.cursor_pos += self.value[self.cursor_pos..]
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                }
                InputAction::None
            }

            _ => InputAction::None,
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.active {
            return;
        }

        let dim = Style::default().add_modifier(Modifier::DIM);
        let title_style = Style::default()
            .fg(Color::Rgb(0x51, 0xAF, 0xEF))
            .add_modifier(Modifier::BOLD);
        let selected_style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        let dir_style = Style::default().fg(Color::Rgb(0x51, 0xAF, 0xEF));

        let mut y = area.y;

        // Top border
        if y < area.y + area.height {
            let sep = "\u{2500}".repeat(area.width as usize);
            buf.set_line(area.x, y, &Line::from(Span::styled(sep, dim)), area.width);
            y += 1;
        }

        // Title
        if y < area.y + area.height {
            buf.set_line(
                area.x,
                y,
                &Line::from(Span::styled(&self.title, title_style)),
                area.width,
            );
            y += 1;
        }

        // Input value
        if y < area.y + area.height {
            buf.set_line(
                area.x,
                y,
                &Line::from(Span::raw(&self.value)),
                area.width,
            );
            y += 1;
        }

        if self.mode == InputMode::Path {
            // Separator
            if y < area.y + area.height {
                let sep = "\u{2500}".repeat(area.width as usize);
                buf.set_line(area.x, y, &Line::from(Span::styled(sep, dim)), area.width);
                y += 1;
            }

            let max_visible = self.max_visible_entries();
            let end = (self.dir_scroll + max_visible).min(self.dir_entries.len());

            for i in self.dir_scroll..end {
                if y >= area.y + area.height {
                    break;
                }
                let entry = &self.dir_entries[i];
                let suffix = if entry.is_dir { "/" } else { "" };
                let text = format!("  {}{}", entry.name, suffix);

                let style = if i == self.dir_cursor {
                    selected_style
                } else if entry.is_dir {
                    dir_style
                } else {
                    dim
                };

                buf.set_line(area.x, y, &Line::from(Span::styled(text, style)), area.width);
                y += 1;
            }

            if self.dir_entries.is_empty() && y < area.y + area.height {
                buf.set_line(
                    area.x,
                    y,
                    &Line::from(Span::styled("  (empty)", dim)),
                    area.width,
                );
                y += 1;
            }
        }

        // Help line at the bottom
        let help_y = area.y + area.height - 1;
        if help_y > y || y >= area.y + area.height {
            let help = if self.mode == InputMode::Path {
                " esc cancel  tab complete  \u{2191}\u{2193} select  enter open/confirm"
            } else {
                " esc cancel"
            };
            buf.set_line(
                area.x,
                help_y,
                &Line::from(Span::styled(help, dim)),
                area.width,
            );
        }
    }

    fn split_dir_prefix<'a>(path: &'a str) -> (&'a str, &'a str) {
        if path.ends_with('/') {
            (path, "")
        } else {
            match path.rfind('/') {
                Some(i) => (&path[..=i], &path[i + 1..]),
                None => (path, ""),
            }
        }
    }

    fn update_dir_listing(&mut self) {
        let path = self.expand_path(&self.value);
        let (dir, prefix) = Self::split_dir_prefix(&path);
        let prefix_lower = prefix.to_lowercase();

        self.dir_entries = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    !name.starts_with('.')
                        && (prefix_lower.is_empty()
                            || name.to_lowercase().starts_with(&prefix_lower))
                })
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let is_dir = e.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    DirEntry { name, is_dir }
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        // Sort: dirs first, then alphabetically
        self.dir_entries
            .sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));

        if self.dir_cursor >= self.dir_entries.len() {
            self.dir_cursor = self.dir_entries.len().saturating_sub(1);
        }
    }

    fn tab_complete(&mut self) {
        let path = self.expand_path(&self.value);
        let (dir, prefix) = Self::split_dir_prefix(&path);
        let prefix_lower = prefix.to_lowercase();

        let matches: Vec<String> = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                        && e.file_name()
                            .to_string_lossy()
                            .to_lowercase()
                            .starts_with(&prefix_lower)
                })
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect(),
            Err(_) => Vec::new(),
        };

        if matches.len() == 1 {
            self.value = format!("{}{}/", dir, matches[0]);
            self.cursor_pos = self.value.len();
        } else if matches.len() > 1 {
            // Find common prefix
            let mut common = matches[0].clone();
            for m in &matches[1..] {
                let shared: String = common
                    .chars()
                    .zip(m.chars())
                    .take_while(|(a, b)| a == b)
                    .map(|(a, _)| a)
                    .collect();
                common = shared;
            }
            if common.len() > prefix.len() {
                self.value = format!("{}{}", dir, common);
                self.cursor_pos = self.value.len();
            }
        }

        self.dir_cursor = 0;
        self.dir_scroll = 0;
        self.update_dir_listing();
    }

    fn expand_path(&self, path: &str) -> String {
        crate::app::expand_tilde(path)
    }

    fn max_visible_entries(&self) -> usize {
        // Reserve lines for: border, title, input, separator, help
        let available = self.height as usize;
        available.saturating_sub(5).max(3)
    }
}
