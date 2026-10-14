//! One serialized background task owns mutations; the terminal renders a stable snapshot.
use crate::{
    app::{Action, App},
    input::action_for_key,
    process::{Control, controlled},
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Instant;

struct Job {
    scroll_revision: u64,
    worker: JoinHandle<(App, bool)>,
    control: Arc<Control>,
    closing: Arc<AtomicBool>,
    started: Instant,
    description: String,
}
pub struct Session {
    scroll_revision: u64,
    pub app: App,
    job: Option<Job>,
    recovery_required: bool,
}
impl Session {
    pub fn new(app: App) -> Self {
        Self {
            app,
            job: None,
            scroll_revision: 0,
            recovery_required: false,
        }
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn tick(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        if !job.worker.is_finished() {
            let progress = job.control.progress();
            self.app.message = format!(
                "{} ({:.1}s) {} — Esc cancels; q stops and quits",
                job.description,
                job.started.elapsed().as_secs_f32(),
                progress
            );
            self.app.message_is_error = false;
            return;
        }
        let job = self.job.take().unwrap();
        match job.worker.join() {
            Ok((mut updated, recovery_required)) => {
                if updated.detail_text == self.app.detail_text
                    && job.scroll_revision != self.scroll_revision
                {
                    updated.detail_scroll = self.app.detail_scroll;
                    updated.detail_column = self.app.detail_column;
                }
                if job.closing.load(Ordering::SeqCst) {
                    updated.running = false;
                }
                self.recovery_required =
                    recovery_required || (self.recovery_required && updated.message_is_error);
                self.app = updated;
            }
            Err(_) => {
                self.recovery_required = true;
                self.app.prompt = None;
                self.app.searching = false;
                self.app.message = "Background task panicked; refresh before another action".into();
                self.app.message_is_error = true;
                if job.closing.load(Ordering::SeqCst) {
                    self.app.running = false;
                }
            }
        }
    }
    pub fn handle_key(&mut self, key: KeyEvent) {
        self.tick();
        if !self.app.running {
            return;
        }
        if key.kind != KeyEventKind::Press {
            return;
        }
        if let Some(job) = &self.job {
            if action_for_key(key) == Some(Action::Quit) {
                job.closing.store(true, Ordering::SeqCst);
                job.control.cancel();
            } else if key.code == KeyCode::Esc {
                job.control.cancel();
            } else if let Some(
                action @ (Action::PageDown
                | Action::PageUp
                | Action::ScrollLeft
                | Action::ScrollRight),
            ) = action_for_key(key)
            {
                self.scroll_revision = self.scroll_revision.wrapping_add(1);
                // The displayed snapshot may still contain the submitted form.
                // Busy viewport keys must not be swallowed by modal input.
                self.app.handle(action);
            }
            return;
        }
        if self.recovery_required
            && !matches!(
                action_for_key(key),
                Some(
                    Action::Refresh
                        | Action::Quit
                        | Action::PageDown
                        | Action::PageUp
                        | Action::ScrollLeft
                        | Action::ScrollRight
                )
            )
        {
            self.app.message =
                "Background task failed; press r to refresh before another action".into();
            self.app.message_is_error = true;
            return;
        }
        if self.local_key(key) {
            self.app.handle_key(key);
            return;
        }
        if action_for_key(key).is_none() && self.app.prompt.is_none() && !self.app.searching {
            return;
        }
        let mut updated = self.app.clone();
        let control = Arc::new(Control::default());
        let closing = Arc::new(AtomicBool::new(false));
        let task_control = control.clone();
        let task_closing = closing.clone();
        let generation = control.generation();
        let description = if self.app.prompt.is_some() {
            "Applying confirmed action".into()
        } else {
            format!(
                "Working: {:?}",
                action_for_key(key).unwrap_or(Action::Search)
            )
        };
        if key.code == KeyCode::Enter
            && self
                .app
                .prompt
                .as_ref()
                .is_some_and(|p| matches!(p.kind, crate::app::PromptKind::CommandPalette))
        {
            self.app.prompt = None;
        }
        let worker = thread::spawn(move || {
            let mut recovery_required = false;
            controlled(task_control.clone(), generation, || updated.handle_key(key));
            if task_control.generation() != generation && !task_closing.load(Ordering::SeqCst) {
                // A cancelled Git command may already have changed index/files/operation state.
                // A fresh cancellation generation makes this refresh cancellable as well.
                let refreshed = controlled(task_control.clone(), task_control.generation(), || {
                    updated.refresh()
                });
                recovery_required = refreshed.is_err();
                if recovery_required {
                    updated.prompt = None;
                    updated.searching = false;
                }
                updated.message = match refreshed { Ok(()) => "Operation cancelled; repository state reloaded. Inspect changes before retrying.".into(), Err(error) => format!("Operation cancelled; refresh failed: {error:#}") };
                updated.message_is_error = true;
            }
            (updated, recovery_required)
        });
        self.job = Some(Job {
            scroll_revision: self.scroll_revision,
            worker,
            control,
            closing,
            started: Instant::now(),
            description,
        });
        self.tick();
    }
    pub fn handle_paste(&mut self, text: &str) {
        self.tick();
        if self.job.is_none() && self.app.running {
            self.app.handle_paste(text);
        }
    }
    /// Keys handled on the UI thread: modal text entry, viewport scrolling and quitting.
    /// Everything that may run Git or GitHub goes through the background worker.
    fn local_key(&self, key: KeyEvent) -> bool {
        let action = action_for_key(key);
        if action == Some(Action::Quit) {
            return true;
        }
        if let Some(prompt) = &self.app.prompt {
            return key.modifiers == crossterm::event::KeyModifiers::ALT
                || key.code != KeyCode::Enter
                || prompt.values.len() + 1 < prompt.labels.len();
        }
        if self.app.searching {
            return key.code != KeyCode::Enter;
        }
        action.is_some_and(|action| self.app.runs_locally(action))
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            job.closing.store(true, Ordering::SeqCst);
            job.control.cancel();
            let _ = job.worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panic_requires_successful_refresh_before_another_mutation() {
        let repo = crate::test_support::TestRepo::new();
        std::fs::write(repo.open().root().join("file"), "pending\n").unwrap();
        let mut session = Session::new(App::new(repo.open()).unwrap());
        let worker = thread::spawn(|| panic!("injected worker failure"));
        while !worker.is_finished() {
            thread::yield_now();
        }
        session.job = Some(Job {
            scroll_revision: 0,
            worker,
            control: Arc::new(Control::default()),
            closing: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            description: "injected failure".into(),
        });
        session.tick();
        session.handle_key(KeyEvent::new(
            KeyCode::Char('s'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(
            !session.busy(),
            "mutation must be blocked after a worker panic"
        );
        assert!(
            repo.open()
                .load_state()
                .unwrap()
                .files
                .iter()
                .all(|f| !f.staged)
        );
        session.handle_key(KeyEvent::new(
            KeyCode::Char('r'),
            crossterm::event::KeyModifiers::NONE,
        ));
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        while session.busy() {
            assert!(Instant::now() < deadline);
            session.tick();
            thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!session.app.message_is_error, "{}", session.app.message);
        session.handle_key(KeyEvent::new(
            KeyCode::Char('s'),
            crossterm::event::KeyModifiers::NONE,
        ));
        while session.busy() {
            assert!(Instant::now() < deadline);
            session.tick();
            thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(repo.open().load_state().unwrap().files[0].staged);
    }
}
