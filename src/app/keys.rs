//! Raw key and paste events: prompt editing, search input, and dispatch to actions.
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{App, PromptKind};
use crate::model::ViewMode;

impl App {
    pub fn handle_paste(&mut self, text: &str) {
        self.palette_selection = 0;
        let target = if let Some(prompt) = &mut self.prompt {
            Some(&mut prompt.text)
        } else if self.searching {
            Some(&mut self.search)
        } else {
            None
        };
        if let Some(target) = target {
            if target.len().saturating_add(text.len()) > 64 * 1024 {
                self.message = "Input exceeds 64 KiB; clear or shorten the field".into();
                self.message_is_error = true;
                return;
            }
            target.push_str(text);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self
            .prompt
            .as_ref()
            .is_some_and(|p| matches!(p.kind, PromptKind::CommandPalette))
        {
            let count =
                crate::commands::filtered(self.view, &self.prompt.as_ref().unwrap().text).len();
            match key.code {
                KeyCode::Up => {
                    self.palette_selection = self.palette_selection.saturating_sub(1);
                    return;
                }
                KeyCode::Down => {
                    self.palette_selection = self
                        .palette_selection
                        .saturating_add(1)
                        .min(count.saturating_sub(1));
                    return;
                }
                KeyCode::Char(_) | KeyCode::Backspace => self.palette_selection = 0,
                _ => {}
            }
        }
        if self.prompt.is_some() {
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                self.running = false;
                return;
            }
            if key.code == KeyCode::Char('u') && key.modifiers == KeyModifiers::CONTROL {
                self.prompt.as_mut().unwrap().text.clear();
                return;
            }
            if key.code == KeyCode::Enter && key.modifiers == KeyModifiers::ALT {
                self.prompt.as_mut().unwrap().text.push('\n');
                return;
            }
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                return;
            }
            if key.code == KeyCode::Esc {
                self.prompt = None;
                return;
            }
            let prompt = self.prompt.as_mut().unwrap();
            match key.code {
                KeyCode::Char(c) => prompt.text.push(c),
                KeyCode::Backspace => {
                    prompt.text.pop();
                }
                KeyCode::Enter => {
                    prompt.values.push(std::mem::take(&mut prompt.text));
                    if prompt.values.len() < prompt.labels.len() {
                        prompt.text = prompt
                            .defaults
                            .get(prompt.values.len())
                            .cloned()
                            .unwrap_or_default();
                    }
                    if prompt.values.len() == prompt.labels.len() {
                        let prompt = self.prompt.take().unwrap();
                        if let Err(error) = self.submit_prompt(prompt) {
                            self.report_error(error);
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if self.searching
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            match key.code {
                KeyCode::Esc => self.searching = false,
                KeyCode::Enter => {
                    self.searching = false;
                    self.find_match(false);
                }
                KeyCode::Backspace => {
                    self.search.pop();
                }
                KeyCode::Char(c) => self.search.push(c),
                _ => {}
            }
        } else if let Some(action) = crate::input::action_for_key(key) {
            self.handle(action);
        }
    }

    pub(super) fn find_match(&mut self, next: bool) {
        if self.search.is_empty() {
            return;
        }
        if self.view == ViewMode::History {
            let len = self.state.commits.len();
            let start = if next {
                self.history_selection.saturating_add(1)
            } else {
                0
            };
            let found = (start..len).chain(0..start.min(len)).find(|index| {
                let c = &self.state.commits[*index];
                [&c.sha, &c.subject, &c.author, &c.date]
                    .iter()
                    .any(|value| value.contains(&self.search))
            });
            if let Some(index) = found {
                self.history_selection = index;
                self.refresh_detail();
            }
            self.message = if found.is_some() {
                format!("History match: {}", self.search)
            } else {
                format!(
                    "No loaded history match: {} (use + to load older commits)",
                    self.search
                )
            };
            return;
        }
        let start = if next {
            usize::from(self.detail_scroll) + 1
        } else {
            0
        };
        let lines: Vec<_> = self.detail_text.lines().collect();
        let found = (start..lines.len())
            .chain(0..start.min(lines.len()))
            .find(|index| lines[*index].contains(&self.search));
        self.detail_column = 0;
        if let Some(line) = found {
            self.detail_scroll = line.min(u16::MAX as usize) as u16;
        }
        self.message = if found.is_some() {
            format!("Find: {}", self.search)
        } else {
            format!("No match: {}", self.search)
        };
    }
}
