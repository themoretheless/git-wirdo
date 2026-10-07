use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::App;
use crate::model::{ViewMode, display_path};

#[derive(Default)]
pub struct Ui {
    lists: [ListState; 15],
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
            Line::from(if let Some(error) = &app.tracking.error {
                format!(
                    "Branch: {} | Tracking unavailable: {}",
                    safe_text(&app.state.branch),
                    safe_text(error)
                )
            } else {
                format!(
                    "Branch: {} | {} +{} -{}",
                    safe_text(&app.state.branch),
                    safe_text(app.tracking.upstream.as_deref().unwrap_or("no upstream")),
                    app.tracking.ahead,
                    app.tracking.behind
                )
            }),
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
            ViewMode::RecentRepositories => {
                "enter open | N initialize | C clone Git | D forget entry | O open path"
            }
            ViewMode::Hunks => {
                "s stage unstaged hunk | u unstage staged hunk | d staged/unstaged | tab files"
            }
            ViewMode::Tags => "N create | D delete local | W publish | X delete remote | r refresh",
            ViewMode::Remotes => {
                "N add | L fetch URL | H push URL | D remove | f fetch | W publish | U upstream | l pull strategy"
            }
            ViewMode::RemoteBranches => "enter track | f fetch | U upstream | W publish",
            ViewMode::Stashes => "S save | y apply | T pop | D drop | r refresh",
            ViewMode::Files => {
                "h history | Q blame | s stage | u unstage | i hunks | w restore index | D discard HEAD | c commit | b branch | f fetch | p pull | P push"
            }
            ViewMode::History => "g graph/patch | + older | Y cherry-pick | Z revert | F reset",
            ViewMode::Reflog => "+ older entries | N recover into new branch",
            ViewMode::Branches => "enter switch | b new | B rename | D delete | m merge | z rebase",
            ViewMode::GitHubRepositories => "enter clone | G auth status | O open local repository",
            ViewMode::Workspaces => "enter open | N create | D remove | O open repository",
            ViewMode::PrFiles => "j/k file | C line comment | PgUp/PgDn patch | r refresh",
            ViewMode::PullRequests => {
                "i files | U filter | + more | L edit | T draft/ready | M merge | A/R review | C comment | V diff"
            }
            ViewMode::Conflicts => "o ours | t theirs | a resolved | e continue | K skip | x abort",
        };
        let message_style = if app.message_is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
        };
        let footer = Paragraph::new(vec![
            Line::from(if let Some(prompt) = &app.prompt { format!("{}: {}_ (Enter next, Esc cancel)", prompt.labels[prompt.values.len()], safe_text(&prompt.text).replace('\n', "↵")) } else if app.searching { format!("Find: {}_ (Enter search, Esc cancel)", safe_text(&app.search)) } else { "tab view | I recent | j/k select | PgUp/PgDn diff | / find | n next | d base | v seen | r refresh | q quit".into() }),
            Line::from(actions),
            Line::from(Span::styled(
                format!("Action: {}{}", safe_text(&app.message).replace('\n', " | "), app.persistence_error.as_ref().map(|e| format!(" | Settings: {}", safe_text(e).replace('\n', " | "))).unwrap_or_default()),
                message_style,
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title("Help"));
        frame.render_widget(footer, layout[2]);
    }
}

fn list_items(app: &App) -> (Vec<ListItem<'static>>, bool) {
    let (lines, empty): (Vec<String>, &str) = match app.view {
        ViewMode::RecentRepositories => (
            app.navigation
                .repositories
                .iter()
                .map(|settings| {
                    settings
                        .root
                        .path()
                        .map(|p| display_path(&p))
                        .unwrap_or_else(|e| format!("Invalid saved path: {e}"))
                })
                .collect(),
            "No recent repositories",
        ),
        ViewMode::Hunks => (
            app.hunks
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    format!(
                        "{} hunk {}: {}",
                        if h.staged { "Staged" } else { "Unstaged" },
                        i + 1,
                        display_path(&h.file.path)
                    )
                })
                .collect(),
            "No text hunks",
        ),
        ViewMode::Tags => (
            app.tags
                .iter()
                .map(|t| format!("{} [{}] {}", t.name, t.kind, t.subject))
                .collect(),
            "No tags",
        ),
        ViewMode::Remotes => (
            app.remotes.iter().map(|r| r.name.clone()).collect(),
            "No remotes",
        ),
        ViewMode::RemoteBranches => (app.remote_branches.clone(), "No remote branches"),
        ViewMode::Stashes => (
            app.stashes
                .iter()
                .map(|s| format!("{} {}", s.selector, s.subject))
                .collect(),
            "No stashes",
        ),
        ViewMode::GitHubRepositories => (
            app.github_repositories
                .iter()
                .map(|r| r.name_with_owner.clone())
                .collect(),
            "No repositories",
        ),
        ViewMode::Workspaces => (
            app.workspaces
                .iter()
                .map(|w| {
                    format!(
                        "{} {}{}",
                        w.branch,
                        display_path(&w.path),
                        if w.locked { " [locked]" } else { "" }
                    )
                })
                .collect(),
            "No workspaces",
        ),
        ViewMode::PrFiles => (
            app.pr_files
                .iter()
                .map(|file| format!("{} {}", file.status, file.filename.escape_debug()))
                .collect(),
            "No PR files loaded",
        ),
        ViewMode::PullRequests => (
            app.pull_requests
                .iter()
                .map(|pr| {
                    format!(
                        "#{} {}{}",
                        pr.number,
                        pr.title,
                        if pr.is_draft { " [draft]" } else { "" }
                    )
                })
                .collect(),
            "No matching pull requests",
        ),
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
        ViewMode::Reflog => (
            app.reflog
                .iter()
                .map(|e| format!("{} {} {}", e.selector, e.sha, e.subject))
                .collect(),
            "No reflog entries",
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
