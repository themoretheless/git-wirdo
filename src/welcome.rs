//! Repository selection before a working tree is available.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use git_wirdo::{
    git::Repository,
    process::{Control, controlled},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

#[derive(Clone, Copy)]
enum Operation {
    Open,
    Initialize,
    Clone,
}
struct Form {
    operation: Operation,
    values: Vec<String>,
    text: String,
}
impl Form {
    fn labels(&self) -> &[&str] {
        match self.operation {
            Operation::Open => &["Repository path"],
            Operation::Initialize => &["New repository path", "Initial branch"],
            Operation::Clone => &[
                "Git URL or local source path",
                "Clone destination (empty or new directory)",
            ],
        }
    }
}
struct Worker {
    started: Instant,
    control: Arc<Control>,
    thread: Option<JoinHandle<Result<Repository>>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.control.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn launch(base: PathBuf, operation: Operation, values: Vec<String>) -> Worker {
    launch_task(move || match operation {
        Operation::Open => Repository::open(&git_wirdo::git::resolve_against(
            &base,
            Path::new(&values[0]),
        )),
        Operation::Initialize => Repository::initialize(&base, Path::new(&values[0]), &values[1]),
        Operation::Clone => {
            Repository::clone_into(&base, values[0].as_ref(), Path::new(&values[1]))
        }
    })
}
fn launch_task(run: impl FnOnce() -> Result<Repository> + Send + 'static) -> Worker {
    let control = Arc::new(Control::default());
    let task = control.clone();
    let generation = task.generation();
    let thread = std::thread::spawn(move || controlled(task, generation, run));
    Worker {
        started: Instant::now(),
        control,
        thread: Some(thread),
    }
}

pub fn run(base: PathBuf, state_path: Option<&Path>) -> Result<Option<Repository>> {
    super::terminal::require_tty()?;
    let (recent, mut message) = match state_path.map(git_wirdo::settings::load).transpose() {
        Ok(navigation) => (
            navigation.map(|n| n.repositories).unwrap_or_default(),
            "Ready".into(),
        ),
        Err(error) => (
            Vec::new(),
            format!("Cannot read recent repositories: {error:#}"),
        ),
    };
    super::terminal::install_panic_hook();
    let _guard = super::terminal::TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut selection = 0usize;
    let mut form: Option<Form> = None;
    let mut worker: Option<Worker> = None;
    let mut quitting = false;
    loop {
        if worker
            .as_ref()
            .is_some_and(|w| w.thread.as_ref().unwrap().is_finished())
        {
            let mut finished = worker.take().unwrap();
            let result = finished
                .thread
                .take()
                .unwrap()
                .join()
                .map_err(|_| anyhow::anyhow!("Repository operation panicked"))?;
            if quitting {
                return Ok(None);
            }
            match result {
                Ok(repository) => return Ok(Some(repository)),
                Err(error) => message = format!("{error:#}"),
            }
        }
        terminal.draw(|frame| {
            let [header, roots, status] = Layout::vertical([
                Constraint::Length(4), Constraint::Min(1), Constraint::Length(8),
            ]).areas(frame.area());
            let heading = format!("Git Wirdo — choose a repository\nDirectory: {}\nO open path | N initialize | C clone Git | Enter open recent | q quit",
                git_wirdo::model::display_path(&base));
            frame.render_widget(Paragraph::new(heading).wrap(Wrap { trim: false }), header);
            let items = recent.iter().map(|settings| {
                ListItem::new(settings.root.path().map(|path| git_wirdo::model::display_path(&path))
                    .unwrap_or_else(|error| format!("Invalid saved path: {error}")))
            }).collect::<Vec<_>>();
            let mut list_state = ListState::default().with_selected((!recent.is_empty()).then_some(selection));
            frame.render_stateful_widget(List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Recent repositories"))
                .highlight_symbol("> ").highlight_style(Style::default().fg(Color::Cyan)), roots, &mut list_state);
            let text = if let Some(worker) = &worker {
                format!("Action: Working: repository operation ({:.1}s)\n{}\nEsc cancel | q cancel and quit",
                    worker.started.elapsed().as_secs_f32(), worker.control.progress())
            } else if let Some(form) = &form {
                let tail = form.text.chars().rev().take(200).collect::<String>().chars().rev().collect::<String>();
                format!("{}: {}{}\nEnter next/submit | Esc cancel | Ctrl-U clear\nAction: {message}",
                    form.labels()[form.values.len()], if tail.len() < form.text.len() { "…" } else { "" }, tail)
            } else { format!("Action: {message}") };
            frame.render_widget(Paragraph::new(text).block(Block::default().borders(Borders::ALL)).wrap(Wrap { trim: false }), status);
        })?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Paste(text) if worker.is_none() => {
                if let Some(form) = &mut form {
                    if text.contains(['\n', '\r', '\0']) || form.text.len() + text.len() > 65536 {
                        message = "Paths must be one line and at most 64 KiB".into();
                    } else {
                        form.text.push_str(&text);
                    }
                }
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    if let Some(worker) = &worker {
                        worker.control.cancel();
                        quitting = true;
                        continue;
                    }
                    return Ok(None);
                }
                if let Some(worker) = &worker {
                    match key.code {
                        KeyCode::Esc => {
                            worker.control.cancel();
                            message = "Cancelling".into();
                        }
                        KeyCode::Char('q') => {
                            worker.control.cancel();
                            quitting = true;
                        }
                        _ => {}
                    }
                    continue;
                }
                if let Some(current) = &mut form {
                    match key.code {
                        KeyCode::Esc => {
                            form = None;
                            message = "Cancelled".into();
                        }
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            current.text.clear()
                        }
                        KeyCode::Backspace => {
                            current.text.pop();
                        }
                        KeyCode::Char(c)
                            if !key
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                        {
                            if current.text.len() + c.len_utf8() <= 65536 {
                                current.text.push(c);
                            }
                        }
                        KeyCode::Enter if current.text.is_empty() => {
                            message = "Value is required".into()
                        }
                        KeyCode::Enter => {
                            current.values.push(std::mem::take(&mut current.text));
                            if current.values.len() == current.labels().len() {
                                let submitted = form.take().unwrap();
                                worker = Some(launch(
                                    base.clone(),
                                    submitted.operation,
                                    submitted.values,
                                ));
                            } else if matches!(current.operation, Operation::Initialize) {
                                current.text = "main".into();
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                    KeyCode::Char('O' | 'o') => {
                        form = Some(Form {
                            operation: Operation::Open,
                            values: vec![],
                            text: String::new(),
                        })
                    }
                    KeyCode::Char('N') => {
                        form = Some(Form {
                            operation: Operation::Initialize,
                            values: vec![],
                            text: String::new(),
                        })
                    }
                    KeyCode::Char('C') => {
                        form = Some(Form {
                            operation: Operation::Clone,
                            values: vec![],
                            text: String::new(),
                        })
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        selection = (selection + 1).min(recent.len().saturating_sub(1))
                    }
                    KeyCode::Up | KeyCode::Char('k') => selection = selection.saturating_sub(1),
                    KeyCode::Enter => {
                        if let Some(settings) = recent.get(selection) {
                            match settings.root.path() {
                                Ok(path) => {
                                    worker = Some(launch_task(move || Repository::open(&path)))
                                }
                                Err(error) => message = format!("{error:#}"),
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
