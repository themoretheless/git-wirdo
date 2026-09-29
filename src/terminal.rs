use std::io::{self, IsTerminal};
use std::time::Duration;

use anyhow::{Result, ensure};
use crossterm::cursor::Show;
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use git_wirdo::app::App;
use git_wirdo::git::Repository;
use git_wirdo::input::action_for_key;
use git_wirdo::ui::Ui;

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        // Construct the guard before the next fallible step so partial setup is also cleaned up.
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
}

pub fn run(repository: Repository) -> Result<()> {
    // Validate/load before touching the terminal; invalid repositories must not break the shell.
    let mut app = App::new(repository)?;
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "Interactive mode requires a terminal; use --headless for scripts"
    );
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Panic hooks run before unwinding, so restore before the diagnostic is printed.
        restore_terminal();
        previous_hook(info);
    }));
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut ui = Ui::default();
    while app.running {
        terminal.draw(|frame| ui.draw(frame, &app))?;
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && let Some(action) = action_for_key(key)
        {
            app.handle(action);
        }
    }
    Ok(())
}
