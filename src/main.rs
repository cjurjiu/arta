mod app;
mod input_panel;
mod keys;
mod sidebar;
mod terminal_pane;
mod tmux;
mod welcome;
mod workspace;

use std::io;
use std::time::Duration;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    self, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--version" || a == "-v") {
        println!("arta {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Panic hook: restore terminal before printing panic message
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        default_hook(info);
    }));

    // Terminal setup
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = app::App::new();

    // Main loop
    loop {
        // Render
        terminal.draw(|frame| app.render(frame))?;

        // Poll for events (~60fps)
        if crossterm::event::poll(Duration::from_millis(16))? {
            let event = crossterm::event::read()?;
            app.handle_event(event);
        }

        // Check for bell/death notifications
        app.check_pane_events();

        if app.should_quit() {
            break;
        }
    }

    // Teardown
    terminal::disable_raw_mode()?;
    crossterm::execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    Ok(())
}
