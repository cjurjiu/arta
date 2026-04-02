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
