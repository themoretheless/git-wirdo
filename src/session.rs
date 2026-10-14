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
    worker: JoinHandle<App>,
    control: Arc<Control>,
    closing: Arc<AtomicBool>,
    started: Instant,
    description: String,
}
pub struct Session {
    scroll_revision: u64,
    pub app: App,
    job: Option<Job>,
}
impl Session {
    pub fn new(app: App) -> Self {
        Self {
            app,
            job: None,
            scroll_revision: 0,
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
            Ok(mut updated) => {
                if updated.detail_text == self.app.detail_text
                    && job.scroll_revision != self.scroll_revision
                {
                    updated.detail_scroll = self.app.detail_scroll;
                    updated.detail_column = self.app.detail_column;
                }
                if job.closing.load(Ordering::SeqCst) {
                    updated.running = false;
                }
                self.app = updated;
            }
            Err(_) => {
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
            controlled(task_control.clone(), generation, || updated.handle_key(key));
            if task_control.generation() != generation && !task_closing.load(Ordering::SeqCst) {
                // A cancelled Git command may already have changed index/files/operation state.
                // A fresh cancellation generation makes this refresh cancellable as well.
                let refreshed = controlled(task_control.clone(), task_control.generation(), || {
                    updated.refresh()
                });
                updated.message = match refreshed { Ok(()) => "Operation cancelled; repository state reloaded. Inspect changes before retrying.".into(), Err(error) => format!("Operation cancelled; refresh failed: {error:#}") };
                updated.message_is_error = true;
            }
            updated
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
    /// Keys that only edit a prompt, a search or UI state never leave the terminal thread;
    /// the final Enter of a prompt and every Git/GitHub action go to the worker.
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
        action.is_some_and(|action| action.runs_locally(self.app.view))
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
