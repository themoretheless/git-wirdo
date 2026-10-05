use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::App;
use crate::model::{ViewMode, display_path};

#[derive(Default)]
pub struct Ui {
    lists: [ListState; 4],
}

impl Ui {
    pub fn draw(&mut self, frame: &mut Frame<'_>, app: &App) {
        if frame.area().width < 60 || frame.area().height < 16 {
            frame.render_widget(
                Paragraph::new("Terminal too small. Resize to at least 60x16, or use --headless.")
                    .wrap(Wrap { trim: false }),
                frame.area(),
            );
            return;
        }
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Min(3),
                Constraint::Length(5),
            ])
            .split(frame.area());

        let header = Paragraph::new(vec![
            Line::from(Span::styled("Git Wirdo", Style::default().fg(Color::Cyan))),
            Line::from(format!("Repo: {}", display_path(app.repository.root()))),
            Line::from(format!("Branch: {}", safe_text(&app.state.branch))),
            Line::from(format!("View: {:?}", app.view)),
            Line::from(app.state.merge_state.summary().replace('\n', " | ")),
        ])
        .block(Block::default().borders(Borders::ALL).title("Status"));
        frame.render_widget(header, layout[0]);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(layout[1]);
        let (items, has_selection) = list_items(app);
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Selection"))
            .highlight_symbol("> ");
        let state = &mut self.lists[app.view.index()];
        state.select(has_selection.then_some(app.selection()));
        frame.render_stateful_widget(list, body[0], state);

        let title = if app.view == ViewMode::Files {
            let files = app.displayed_files();
            format!(
                "{} | Seen {}/{}",
                if app.upstream_comparison {
                    "Upstream"
                } else {
                    "Working changes"
                },
                files.iter().filter(|file| app.file_seen(file)).count(),
                files.len()
            )
        } else {
            "Details".into()
        };
        let detail = Paragraph::new(safe_text(&app.detail_text))
            .block(Block::default().borders(Borders::ALL).title(title))
            .scroll((app.detail_scroll, app.detail_column));
        frame.render_widget(detail, body[1]);

        let actions = match app.view {
            ViewMode::Files => {
                "s stage | u unstage | c commit | b branch | f fetch | p pull | P push"
            }
            ViewMode::History => "c commit | b branch | f fetch | p pull (ff only) | P push",
            ViewMode::Branches => "enter switch | b branch | c commit | f fetch | p pull | P push",
            ViewMode::Conflicts => "o ours | t theirs | a resolved | e continue | K skip | x abort",
        };
        let message_style = if app.message_is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
        };
        let footer = Paragraph::new(vec![
            Line::from(if app.searching { format!("Find: {}_ (Enter search, Esc cancel)", safe_text(&app.search)) } else { "tab view | j/k select | PgUp/PgDn diff | / find | n next | d base | v seen | r refresh | q quit".into() }),
            Line::from(actions),
            Line::from(Span::styled(
                format!("Action: {}", safe_text(&app.message).replace('\n', " | ")),
                message_style,
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title("Help"));
        frame.render_widget(footer, layout[2]);
    }
}

fn list_items(app: &App) -> (Vec<ListItem<'static>>, bool) {
    let (lines, empty): (Vec<String>, &str) = match app.view {
        ViewMode::Files => (
            app.displayed_files()
                .iter()
                .map(|file| {
                    let marker = match (file.staged, file.unstaged) {
                        (true, true) => "SU",
                        (true, false) => "S ",
                        (false, true) => " U",
                        (false, false) => "  ",
                    };
                    format!(
                        "[{marker}] [{}] {}{}",
                        file.status,
                        file.label(),
                        if app.file_seen(file) { " [seen]" } else { "" }
                    )
                })
                .collect(),
            "No changes",
        ),
        ViewMode::History => (
            app.state
                .commits
                .iter()
                .map(|commit| {
                    format!(
                        "{} {} {} {}",
                        commit.short_sha, commit.date, commit.author, commit.subject
                    )
                })
                .collect(),
            "No commits",
        ),
        ViewMode::Branches => (
            app.state
                .branches
                .iter()
                .map(|branch| format!("{} {}", if branch.current { "*" } else { " " }, branch.name))
                .collect(),
            "No branches",
        ),
        ViewMode::Conflicts => (
            app.state
                .merge_state
                .conflicts
                .iter()
                .map(|path| display_path(path))
                .collect(),
            "No conflicted files",
        ),
    };
    if lines.is_empty() {
        (vec![ListItem::new(empty)], false)
    } else {
        (
            lines
                .iter()
                .map(|line| ListItem::new(safe_text(line)))
                .collect(),
            true,
        )
    }
}

/// Git hooks and file contents may include terminal control sequences. Render them as text.
fn safe_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_control() && !matches!(character, '\n' | '\t') {
            result.extend(character.escape_default());
        } else {
            result.push(character);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_terminal_controls_but_preserves_diff_lines() {
        assert_eq!(
            safe_text("+ hello\n\tworld\x1b[31m"),
            "+ hello\n\tworld\\u{1b}[31m"
        );
    }
}
