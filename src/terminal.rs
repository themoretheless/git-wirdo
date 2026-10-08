use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use anyhow::{Result, ensure};
use crossterm::cursor::Show;
use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use git_wirdo::app::App;
use git_wirdo::git::Repository;
use git_wirdo::session::Session;
use git_wirdo::ui::Ui;

pub(crate) struct TerminalGuard;

impl TerminalGuard {
    pub(crate) fn enter() -> Result<Self> {
        enable_raw_mode()?;
        // Construct the guard before the next fallible step so partial setup is also cleaned up.
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
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
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
}

/// Raw mode on a pipe would hang scripts, so interactive screens refuse to start without a TTY.
pub(crate) fn require_tty() -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "Interactive mode requires a terminal; use --headless for scripts"
    );
    Ok(())
}

pub fn run(repository: Repository, state_path: Option<PathBuf>) -> Result<()> {
    // Validate/load before touching the terminal; invalid repositories must not break the shell.
    let mut session = Session::new(App::new(repository)?);
    require_tty()?;
    let mut store = None;
    if let Some(path) = state_path {
        match git_wirdo::settings::Store::open(path.clone()) {
            Ok((loaded, navigation)) => {
                session.app.restore_navigation(navigation);
                store = Some(loaded);
                // Saved remote views are refreshed through the cancellable worker.
                session.handle_key(crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('r'),
                    crossterm::event::KeyModifiers::NONE,
                ));
            }
            Err(error) => {
                session.app.persistence_error =
                    Some(format!("Saving disabled: {error:#} ({})", path.display()))
            }
        }
    }
    install_panic_hook();
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut ui = Ui::default();
    let mut last_save = Instant::now() - Duration::from_secs(1);
    loop {
        session.tick();
        if !session.busy()
            && (last_save.elapsed() >= Duration::from_secs(1) || !session.app.running)
        {
            session.app.capture_navigation();
            if let Some(store) = &mut store {
                match store.persist(&session.app.navigation) {
                    Ok(navigation) => {
                        session.app.navigation = navigation;
                        session.app.persistence_error = None;
                    }
                    Err(error) => session.app.persistence_error = Some(format!("{error:#}")),
                }
            }
            last_save = Instant::now();
        }
        if !session.app.running {
            break;
        }
        terminal.draw(|frame| ui.draw(frame, &session.app))?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => session.handle_key(key),
                Event::Paste(text) => session.handle_paste(&text),
                _ => {}
            }
        }
    }
    drop(terminal);
    drop(_guard);
    if let Some(error) = &session.app.persistence_error {
        eprintln!("UI settings: {error}");
    }
    Ok(())
}

pub(crate) fn install_panic_hook() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Panic hooks run before unwinding, so restore before the diagnostic is printed.
        restore_terminal();
        previous_hook(info);
    }));
}
