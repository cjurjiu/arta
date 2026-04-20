use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Convert crossterm modifier flags to xterm modifier parameter.
/// xterm uses: 2=Shift, 3=Alt, 4=Shift+Alt, 5=Ctrl, 6=Ctrl+Shift, 7=Ctrl+Alt, 8=Ctrl+Shift+Alt
fn modifier_param(modifiers: KeyModifiers) -> u8 {
    let mut m: u8 = 1;
    if modifiers.contains(KeyModifiers::SHIFT) {
        m += 1;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        m += 2;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        m += 4;
    }
    m
}

pub fn key_event_to_bytes(event: &KeyEvent) -> Option<Vec<u8>> {
    // Ctrl+letter → control character
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = event.code {
            if c.is_ascii_lowercase() {
                return Some(vec![c as u8 - b'a' + 1]);
            }
            if c.is_ascii_uppercase() {
                return Some(vec![c.to_ascii_lowercase() as u8 - b'a' + 1]);
            }
        }
    }

    // Alt+char → ESC + char (readline meta prefix, e.g. Alt+b, Alt+f for word nav)
    if event.modifiers.contains(KeyModifiers::ALT)
        && !event.modifiers.contains(KeyModifiers::CONTROL)
    {
        if let KeyCode::Char(c) = event.code {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            let mut out = Vec::with_capacity(1 + s.len());
            out.push(0x1b);
            out.extend_from_slice(s.as_bytes());
            return Some(out);
        }
    }

    match event.code {
        // Printable characters
        KeyCode::Char(c) if event.modifiers.is_empty() || event.modifiers == KeyModifiers::SHIFT => {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            Some(s.as_bytes().to_vec())
        }

        // Special keys
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::BackTab => Some(b"\x1b[Z".to_vec()),
        KeyCode::Esc => Some(vec![0x1b]),

        // Modified arrow keys (Ctrl, Alt, Ctrl+Shift, etc.)
        KeyCode::Up | KeyCode::Down | KeyCode::Right | KeyCode::Left
            if event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let dir = match event.code {
                KeyCode::Up => 'A',
                KeyCode::Down => 'B',
                KeyCode::Right => 'C',
                _ => 'D',
            };
            let m = modifier_param(event.modifiers);
            Some(format!("\x1b[1;{}{}", m, dir).into_bytes())
        }

        // Arrow keys (plain)
        KeyCode::Up => Some(b"\x1b[A".to_vec()),
        KeyCode::Down => Some(b"\x1b[B".to_vec()),
        KeyCode::Right => Some(b"\x1b[C".to_vec()),
        KeyCode::Left => Some(b"\x1b[D".to_vec()),

        // Modified navigation keys
        KeyCode::Home if event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            let m = modifier_param(event.modifiers);
            Some(format!("\x1b[1;{}H", m).into_bytes())
        }
        KeyCode::End if event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            let m = modifier_param(event.modifiers);
            Some(format!("\x1b[1;{}F", m).into_bytes())
        }

        // Navigation (plain)
        KeyCode::Home => Some(b"\x1b[H".to_vec()),
        KeyCode::End => Some(b"\x1b[F".to_vec()),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        KeyCode::PageUp => Some(b"\x1b[5~".to_vec()),
        KeyCode::PageDown => Some(b"\x1b[6~".to_vec()),

        // Function keys
        KeyCode::F(1) => Some(b"\x1bOP".to_vec()),
        KeyCode::F(2) => Some(b"\x1bOQ".to_vec()),
        KeyCode::F(3) => Some(b"\x1bOR".to_vec()),
        KeyCode::F(4) => Some(b"\x1bOS".to_vec()),
        KeyCode::F(5) => Some(b"\x1b[15~".to_vec()),
        KeyCode::F(6) => Some(b"\x1b[17~".to_vec()),
        KeyCode::F(7) => Some(b"\x1b[18~".to_vec()),
        KeyCode::F(8) => Some(b"\x1b[19~".to_vec()),
        KeyCode::F(9) => Some(b"\x1b[20~".to_vec()),
        KeyCode::F(10) => Some(b"\x1b[21~".to_vec()),
        KeyCode::F(11) => Some(b"\x1b[23~".to_vec()),
        KeyCode::F(12) => Some(b"\x1b[24~".to_vec()),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn test_printable() {
        assert_eq!(key_event_to_bytes(&key(KeyCode::Char('a'))), Some(vec![b'a']));
        assert_eq!(key_event_to_bytes(&key(KeyCode::Char('Z'))), Some(vec![b'Z']));
    }

    #[test]
    fn test_ctrl() {
        assert_eq!(key_event_to_bytes(&ctrl_key('a')), Some(vec![1]));
        assert_eq!(key_event_to_bytes(&ctrl_key('c')), Some(vec![3]));
        assert_eq!(key_event_to_bytes(&ctrl_key('z')), Some(vec![26]));
    }

    #[test]
    fn test_special_keys() {
        assert_eq!(key_event_to_bytes(&key(KeyCode::Enter)), Some(vec![b'\r']));
        assert_eq!(key_event_to_bytes(&key(KeyCode::Backspace)), Some(vec![0x7f]));
        assert_eq!(key_event_to_bytes(&key(KeyCode::Tab)), Some(vec![b'\t']));
        assert_eq!(key_event_to_bytes(&key(KeyCode::Esc)), Some(vec![0x1b]));
    }

    #[test]
    fn test_arrows() {
        assert_eq!(key_event_to_bytes(&key(KeyCode::Up)), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_event_to_bytes(&key(KeyCode::Down)), Some(b"\x1b[B".to_vec()));
    }

    #[test]
    fn test_alt_char() {
        // Option+Left in iTerm2 sends ESC+b; crossterm reports it as Alt+b
        let alt_b = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT);
        assert_eq!(key_event_to_bytes(&alt_b), Some(vec![0x1b, b'b']));
        let alt_f = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT);
        assert_eq!(key_event_to_bytes(&alt_f), Some(vec![0x1b, b'f']));
    }

    #[test]
    fn test_function_keys() {
        assert_eq!(key_event_to_bytes(&key(KeyCode::F(1))), Some(b"\x1bOP".to_vec()));
        assert_eq!(key_event_to_bytes(&key(KeyCode::F(12))), Some(b"\x1b[24~".to_vec()));
    }
}
