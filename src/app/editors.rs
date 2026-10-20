//! Transient UI editors. Git mutations still run through guarded repository methods.
use super::{Action, App, HistoryPrompt, PromptKind};
use crate::{git::InteractiveRebase, model::ViewMode, patch::Hunk};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub(crate) struct LineEditor {
    pub hunk: Hunk,
    pub cursor: usize,
    pub selected: BTreeSet<usize>,
    pub anchor: Option<usize>,
    pub count: usize,
    changes: Vec<usize>,
    lines: Vec<String>,
}
impl LineEditor {
    pub fn new(hunk: Hunk) -> Self {
        let changes = hunk.changed_rows();
        let count = changes.len();
        let lines = String::from_utf8_lossy(&hunk.patch)
            .lines()
            .map(str::to_owned)
            .collect();
        Self {
            hunk,
            cursor: 1,
            selected: BTreeSet::new(),
            anchor: None,
            count,
            changes,
            lines,
        }
    }
    pub fn selection(&self) -> Vec<usize> {
        let mut selected = self.selected.clone();
        if let Some(anchor) = self.anchor {
            selected.extend(anchor.min(self.cursor)..=anchor.max(self.cursor));
        }
        selected.into_iter().collect()
    }
    pub fn rows(&self, height: usize) -> (Vec<String>, usize) {
        let cursor = self
            .changes
            .get(self.cursor.saturating_sub(1))
            .copied()
            .unwrap_or(0);
        let height = height.max(1);
        let start = cursor
            .saturating_sub(height / 2)
            .min(self.lines.len().saturating_sub(height));
        let end = (start + height).min(self.lines.len());
        let rows = self.lines[start..end]
            .iter()
            .enumerate()
            .map(
                |(offset, text)| match self.changes.binary_search(&(start + offset)) {
                    Ok(index) => {
                        let number = index + 1;
                        let selected = self.selected.contains(&number)
                            || self.anchor.is_some_and(|a| {
                                (a.min(self.cursor)..=a.max(self.cursor)).contains(&number)
                            });
                        format!("[{}] {number:>4} {text}", if selected { 'x' } else { ' ' })
                    }
                    Err(_) => format!("         {text}"),
                },
            )
            .collect();
        (rows, cursor.saturating_sub(start))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RebaseEditor {
    pub plan: InteractiveRebase,
    pub rows: Vec<String>,
    pub cursor: usize,
    pub preview: bool,
    pub preview_row: usize,
}
impl RebaseEditor {
    pub fn new(plan: InteractiveRebase) -> Self {
        let rows = plan.todo.lines().map(str::to_owned).collect();
        Self {
            plan,
            rows,
            cursor: 0,
            preview: false,
            preview_row: 0,
        }
    }
    pub fn display_rows(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                let mut parts = row.splitn(3, ' ');
                format!(
                    "{:<6} {:.12} {}",
                    parts.next().unwrap_or(""),
                    parts.next().unwrap_or(""),
                    parts.next().unwrap_or("").trim_start_matches("# ")
                )
            })
            .collect()
    }
    pub fn todo(&self) -> String {
        self.rows.join("\n")
    }
    pub fn result(&self) -> Vec<String> {
        let mut result = vec!["Proposed commits (new IDs are assigned by Git):".into()];
        let mut count = 0;
        for row in &self.rows {
            let mut parts = row.splitn(3, ' ');
            let action = parts.next().unwrap_or("");
            let sha = parts.next().unwrap_or("");
            let subject = parts.next().unwrap_or("").trim_start_matches("# ");
            if matches!(action, "pick" | "edit" | "reword") {
                count += 1;
                result.push(format!("{count}. {subject} [{:.12}]", sha));
            } else if action == "drop" {
                result.push(format!("   DROP: {subject} [{:.12}]", sha));
            } else {
                result.push(format!("   + {action}: {subject} [{:.12}]", sha));
            }
        }
        result.push(format!(
            "{} original commits -> {count} resulting commits",
            self.rows.len()
        ));
        result
    }
}

impl App {
    pub fn name_filter(&self) -> &str {
        match self.view {
            ViewMode::Files => &self.file_filter,
            ViewMode::Branches => &self.branch_filter,
            _ => "",
        }
    }
    pub(crate) fn update_name_filter(&mut self, view: ViewMode, text: &str) {
        match view {
            ViewMode::Files => self.file_filter = text.to_lowercase(),
            ViewMode::Branches => self.branch_filter = text.to_lowercase(),
            _ => return,
        }
        let visible = self.visible_indices();
        if !visible.contains(&self.selection())
            && let Some(&index) = visible.first()
        {
            match view {
                ViewMode::Files => self.file_selection = index,
                ViewMode::Branches => self.branch_selection = index,
                _ => {}
            }
        }
        self.detail_scroll = 0;
        self.detail_column = 0;
        // Typing never loads Git details on the terminal thread, nor shows details
        // belonging to the now-hidden selection. Enter/Esc loads the chosen object.
        self.detail_text = "Name filter preview; Enter applies, Esc closes, Ctrl-U clears".into();
        self.notify(format!(
            "Filter: {} ({} matches)",
            self.name_filter(),
            visible.len()
        ));
    }
    pub(super) fn sync_filter_prompt(&mut self) {
        if let Some(prompt) = &self.prompt
            && let PromptKind::ListFilter(view) = prompt.kind
        {
            let text = prompt.text.clone();
            self.update_name_filter(view, &text);
        }
    }
    pub(crate) fn editor_key_is_local(&self, key: KeyEvent) -> bool {
        if self.line_editor.is_some() {
            return !matches!(key.code, KeyCode::Char('s' | 'u'));
        }
        self.rebase_editor.is_some()
    }
    /// Return true when a modal editor consumed the key.
    pub(super) fn handle_editor_key(&mut self, key: KeyEvent) -> bool {
        if self.line_editor.is_none() && self.rebase_editor.is_none() {
            return false;
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            self.running = false;
            return true;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return true;
        }
        if let Some(editor) = &mut self.line_editor {
            match key.code {
                KeyCode::Esc => {
                    self.line_editor = None;
                }
                KeyCode::Char('q') => self.running = false,
                KeyCode::Down | KeyCode::Char('j') => {
                    editor.cursor = (editor.cursor + 1).min(editor.count)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    editor.cursor = editor.cursor.saturating_sub(1).max(1)
                }
                KeyCode::PageDown => editor.cursor = (editor.cursor + 20).min(editor.count),
                KeyCode::PageUp => editor.cursor = editor.cursor.saturating_sub(20).max(1),
                KeyCode::Home => editor.cursor = 1,
                KeyCode::End => editor.cursor = editor.count,
                KeyCode::Char('v') => {
                    if editor.anchor.is_some() {
                        editor.selected = editor.selection().into_iter().collect();
                        editor.anchor = None;
                    } else {
                        editor.anchor = Some(editor.cursor);
                    }
                }
                KeyCode::Char(' ') => {
                    if editor.anchor.is_some() {
                        editor.selected = editor.selection().into_iter().collect();
                        editor.anchor = None;
                    } else if !editor.selected.remove(&editor.cursor) {
                        editor.selected.insert(editor.cursor);
                    }
                }
                KeyCode::Char('s') => self.handle(Action::Stage),
                KeyCode::Char('u') => self.handle(Action::Unstage),
                _ => {}
            }
            return true;
        }
        let editor = self.rebase_editor.as_mut().unwrap();
        match key.code {
            KeyCode::Esc => self.rebase_editor = None,
            KeyCode::Char('q') => self.running = false,
            KeyCode::Down | KeyCode::Char('j') => {
                editor.cursor = (editor.cursor + 1).min(editor.rows.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => editor.cursor = editor.cursor.saturating_sub(1),
            KeyCode::Char('J') if editor.cursor + 1 < editor.rows.len() => {
                editor.rows.swap(editor.cursor, editor.cursor + 1);
                editor.cursor += 1;
            }
            KeyCode::Char('K') if editor.cursor > 0 => {
                editor.rows.swap(editor.cursor, editor.cursor - 1);
                editor.cursor -= 1;
            }
            KeyCode::Char('r') => {
                let sha = editor.rows[editor.cursor]
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let message = editor.plan.message(&sha).unwrap_or("").to_owned();
                self.start_prefilled_prompt(
                    HistoryPrompt::Reword(sha),
                    vec!["Replacement commit message"],
                    vec![message],
                );
            }
            KeyCode::Char(c @ ('p' | 's' | 'f' | 'e' | 'd')) => {
                let action = match c {
                    's' => "squash",
                    'f' => "fixup",
                    'e' => "edit",
                    'd' => "drop",
                    _ => "pick",
                };
                let row = &mut editor.rows[editor.cursor];
                if let Some((_, rest)) = row.split_once(' ') {
                    *row = format!("{action} {rest}");
                }
            }
            KeyCode::PageDown => {
                editor.preview = true;
                editor.preview_row = (editor.preview_row + 10).min(editor.rows.len() + 1);
            }
            KeyCode::PageUp => {
                editor.preview = true;
                editor.preview_row = editor.preview_row.saturating_sub(10);
            }
            KeyCode::Home => editor.cursor = 0,
            KeyCode::End => editor.cursor = editor.rows.len().saturating_sub(1),
            KeyCode::Char('v') => editor.preview = !editor.preview,
            KeyCode::Enter => {
                if !editor
                    .rows
                    .iter()
                    .find(|r| !r.starts_with("drop "))
                    .is_some_and(|r| {
                        r.starts_with("pick ") || r.starts_with("edit ") || r.starts_with("reword ")
                    })
                {
                    self.notify("First retained commit must use pick, edit or reword");
                } else {
                    let kind =
                        HistoryPrompt::ConfirmInteractive(editor.plan.clone(), editor.todo());
                    editor.preview = true;
                    self.start_prompt(kind, vec!["Type rebase to apply the preview"]);
                }
            }
            _ => {}
        }
        true
    }
}
