use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn render_welcome(area: Rect, buf: &mut Buffer) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let green = Style::default().fg(Color::Rgb(0x98, 0xBE, 0x65));
    let pink = Style::default().fg(Color::Rgb(0xFF, 0x6C, 0x6B));
    let _yellow = Style::default().fg(Color::Rgb(0xEC, 0xBE, 0x7B));
    let magenta = Style::default().fg(Color::Rgb(0xC6, 0x78, 0xDD));
    let cyan = Style::default().fg(Color::Rgb(0x46, 0xD9, 0xFF));
    let frame_style = Style::default()
        .fg(Color::Rgb(0xEC, 0xBE, 0x7B))
        .add_modifier(Modifier::BOLD);
    let red = Style::default()
        .fg(Color::Rgb(0xFF, 0x6C, 0x6B))
        .add_modifier(Modifier::BOLD);

    let fw: usize = 35; // frame inner width

    // Each scene line is a plain colored string, padded to exactly fw chars.
    // This ensures the right frame border aligns.
    let scene_lines: Vec<String> = vec![
        String::new(),
        format!("                {}", "_\\"),
        format!("               {}", "(_)"),
        format!("           {}  {}  {}", "@", "_|_", "@"),
        format!("          {} {} {}", "@@@", " / ", "@@@"),
        format!("           {}  {}  {}", "@", " | ", "@"),
        format!("      ,       |          *"),
        format!("     /\\       |      *   |    ,"),
        "    /  \\   .  |    .   \\|/   / \\".to_string(),
        "   / .  \\  |\\ |    |\\   |  / . \\".to_string(),
        "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~".to_string(),
    ];

    // Scene lines with proper coloring via styled spans
    let scene_styled: Vec<Vec<Span>> = vec![
        vec![Span::raw("")],
        vec![
            Span::raw("                "),
            Span::styled("_", magenta),
            Span::styled("\\", pink),
        ],
        vec![
            Span::raw("               "),
            Span::styled("(_)", pink),
        ],
        vec![
            Span::raw("           "),
            Span::styled("@", pink),
            Span::raw("  "),
            Span::styled("_|_", magenta),
            Span::raw("  "),
            Span::styled("@", pink),
        ],
        vec![
            Span::raw("          "),
            Span::styled("@@@", pink),
            Span::styled(" / ", green),
            Span::styled("@@@", pink),
        ],
        vec![
            Span::raw("           "),
            Span::styled("@", pink),
            Span::styled("  |  ", green),
            Span::styled("@", pink),
        ],
        vec![
            Span::styled("      ,       |", green),
            Span::raw("          "),
            Span::styled("*", Style::default().fg(Color::Rgb(0xEC, 0xBE, 0x7B))),
        ],
        vec![
            Span::styled("     ", green),
            Span::styled("/\\", cyan),
            Span::styled("       |      ", green),
            Span::styled("*", Style::default().fg(Color::Rgb(0xEC, 0xBE, 0x7B))),
            Span::styled("   |    ", green),
            Span::styled(",", cyan),
        ],
        vec![Span::styled(
            "    /  \\   .  |    .   \\|/   / \\",
            green,
        )],
        vec![Span::styled(
            "   / .  \\  |\\ |    |\\   |  / . \\",
            green,
        )],
        vec![Span::styled(
            "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
            green,
        )],
    ];

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("               a r t a", red)));
    lines.push(Line::from(Span::styled(
        " agent runtime terminal application",
        dim,
    )));
    lines.push(Line::from(""));

    // Top frame border
    lines.push(Line::from(Span::styled(
        format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(fw)),
        frame_style,
    )));

    // Scene lines inside the frame — pad each to fw width
    for (i, styled_spans) in scene_styled.iter().enumerate() {
        let content_len = scene_lines[i].chars().count();
        let padding = fw.saturating_sub(content_len);

        let mut spans = vec![Span::styled("\u{2551}", frame_style)];
        spans.extend(styled_spans.clone());
        if padding > 0 {
            spans.push(Span::raw(" ".repeat(padding)));
        }
        spans.push(Span::styled("\u{2551}", frame_style));
        lines.push(Line::from(spans));
    }

    // Bottom frame border
    lines.push(Line::from(Span::styled(
        format!("\u{255a}{}\u{255d}", "\u{2550}".repeat(fw)),
        frame_style,
    )));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "    Select a project and press 'n'",
        dim,
    )));
    lines.push(Line::from(Span::styled(
        "    or press 'a' to add a project",
        dim,
    )));

    // Center vertically
    let total_lines = lines.len();
    let pad_top = (area.height as usize).saturating_sub(total_lines) / 2;

    // Center horizontally
    let frame_width = fw + 2;
    let h_pad = (area.width as usize).saturating_sub(frame_width) / 2;

    for (i, line) in lines.iter().enumerate() {
        let y = area.y + pad_top as u16 + i as u16;
        if y >= area.y + area.height {
            break;
        }
        buf.set_line(
            area.x + h_pad as u16,
            y,
            line,
            area.width.saturating_sub(h_pad as u16),
        );
    }
}
