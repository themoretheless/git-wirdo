//! Terminal rendering of an `App` snapshot: header, list, detail pane, footer, palette.
use git_wirdo_core::model::printable_text as safe_text;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{App, PromptKind};
use crate::model::{ViewMode, display_path};

const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 16;

#[derive(Default)]
pub struct Ui {
    lists: [ListState; ViewMode::ALL.len()],
    detail: DetailCache,
    line_editor: ListState,
    rebase_editor: ListState,
    rebase_preview: ListState,
}

/// The detail text last rendered and its escaped form; large patches are escaped once.
#[derive(Default)]
struct DetailCache {
    source: String,
    rendered: String,
}

impl DetailCache {
    fn rendered(&mut self, text: &str) -> &str {
        if self.source != text {
            self.source = text.to_owned();
            self.rendered = safe_text(text);
        }
        &self.rendered
    }
}

impl Ui {
    pub fn draw(&mut self, frame: &mut Frame<'_>, app: &App) {
        if frame.area().width < MIN_WIDTH || frame.area().height < MIN_HEIGHT {
            frame.render_widget(
                Paragraph::new("Terminal too small. Resize to at least 60x16, or use --headless.")
                    .wrap(Wrap { trim: false }),
                frame.area(),
            );
            return;
        }
        let [header, body, footer] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Min(3),
                Constraint::Length(5),
            ])
            .areas(frame.area());
        let [list, detail] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .areas(body);

        render_header(frame, header, app);
        if app.line_editor().is_none() && app.rebase_editor().is_none() {
            self.render_list(frame, list, app);
            render_detail(frame, detail, app, &mut self.detail);
        }
        render_footer(frame, footer, app);
        if let Some(editor) = app.line_editor() {
            let (rows, cursor) = editor.rows(usize::from(body.height.saturating_sub(2)));
            frame.render_widget(Clear, body);
            let items = rows
                .iter()
                .map(|row| ListItem::new(safe_text(row)))
                .collect::<Vec<_>>();
            let widget = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(format!(
                    "Select lines | {} selected | {}",
                    editor.selection().len(),
                    display_path(&editor.hunk.file.path)
                )))
                .highlight_style(Style::default().fg(Color::Yellow))
                .highlight_symbol("> ");
            *self.line_editor.offset_mut() = 0;
            self.line_editor.select(Some(cursor));
            frame.render_stateful_widget(widget, body, &mut self.line_editor);
        }
        if let Some(editor) = app.rebase_editor() {
            frame.render_widget(Clear, body);
            let [todo, preview] = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(body);
            let display_rows = editor.display_rows();
            let items = display_rows
                .iter()
                .map(|row| ListItem::new(safe_text(row)))
                .collect::<Vec<_>>();
            let widget = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Interactive rebase | oldest first"),
                )
                .highlight_style(Style::default().fg(Color::Yellow))
                .highlight_symbol("> ");
            self.rebase_editor.select(Some(editor.cursor));
            frame.render_stateful_widget(widget, todo, &mut self.rebase_editor);
            if editor.preview {
                let rows = editor.result();
                self.rebase_preview
                    .select(Some(editor.preview_row.min(rows.len().saturating_sub(1))));
                let items = rows
                    .iter()
                    .map(|row| ListItem::new(safe_text(row)))
                    .collect::<Vec<_>>();
                let widget = List::new(items)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Rebase preview | PgUp/PgDn"),
                    )
                    .highlight_style(Style::default().fg(Color::Cyan));
                frame.render_stateful_widget(widget, preview, &mut self.rebase_preview);
            } else {
                let text = "j/k select commit\nJ/K move commit\np pick, s squash, f fixup\nr reword, e edit, d drop\nv preview resulting groups\nPgUp/PgDn scroll preview\nEnter review and confirm\nEsc cancel\n\nSquash combines messages.\nFixup discards its message.\nOriginal HEAD is saved for recovery.";
                frame.render_widget(
                    Paragraph::new(text).wrap(Wrap { trim: false }).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Rebase preview"),
                    ),
                    preview,
                );
            }
        }
        if let Some(prompt) = &app.prompt()
            && matches!(prompt.kind, PromptKind::CommandPalette)
        {
            render_palette(frame, app, &prompt.text);
        }
    }

    fn render_list(&mut self, frame: &mut Frame<'_>, area: Rect, app: &App) {
        let (items, has_selection) = list_items(app);
        let list =
            List::new(items)
                .block(Block::default().borders(Borders::ALL).title(
                    if !app.name_filter().is_empty() {
                        format!(
                            "Selection | filter: {} | {} matches",
                            safe_text(app.name_filter()),
                            app.visible_indices().len()
                        )
                    } else {
                        "Selection".into()
                    },
                ))
                .highlight_symbol("> ");
        let state = &mut self.lists[app.view().index()];
        let selection = if !app.name_filter().is_empty() {
            app.visible_indices()
                .iter()
                .position(|i| *i == app.selection())
        } else {
            has_selection.then_some(app.selection())
        };
        state.select(selection);
        frame.render_stateful_widget(list, area, state);
    }
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let tracking = if let Some(error) = &app.tracking().error {
        format!(
            "Branch: {} | Tracking unavailable: {}",
            safe_text(&app.state().branch),
            safe_text(error)
        )
    } else {
        format!(
            "Branch: {} | {} +{} -{}",
            safe_text(&app.state().branch),
            safe_text(app.tracking().upstream.as_deref().unwrap_or("no upstream")),
            app.tracking().ahead,
            app.tracking().behind
        )
    };
    let header = Paragraph::new(vec![
        Line::from(Span::styled("Git Wirdo", Style::default().fg(Color::Cyan))),
        Line::from(format!("Repo: {}", display_path(app.repository().root()))),
        Line::from(tracking),
        Line::from(format!("View: {:?}", app.view())),
        Line::from(app.state().merge_state.summary().replace('\n', " | ")),
    ])
    .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(header, area);
}

fn render_detail(frame: &mut Frame<'_>, area: Rect, app: &App, cache: &mut DetailCache) {
    let title = if app.view() == ViewMode::Files {
        let files = app.displayed_files();
        format!(
            "{} | Seen {}/{} | Stash selected {}",
            if app.browse_tracked() {
                "Tracked files (read-only)"
            } else if app.upstream_comparison() {
                "Upstream"
            } else {
                "Working changes"
            },
            files.iter().filter(|file| app.file_seen(file)).count(),
            files.len(),
            app.stash_files().len()
        )
    } else {
        "Details".into()
    };
    let detail = Paragraph::new(cache.rendered(app.detail_text()))
        .block(Block::default().borders(Borders::ALL).title(title))
        .scroll((app.detail_scroll(), app.detail_column()));
    frame.render_widget(detail, area);
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let input = if let Some(prompt) = &app.prompt() {
        format!(
            "{}: {}_ (Enter next, Esc cancel)",
            prompt.labels[prompt.values.len()],
            safe_text(&prompt.text).replace('\n', "↵")
        )
    } else if app.line_editor().is_some() {
        "Select lines: j/k move | PgUp/Dn, Home/End jump | Space toggle | v range | s stage | u unstage | Esc cancel".into()
    } else if app.rebase_editor().is_some() {
        "Rebase: j/k select | J/K reorder | p/s/f actions | r reword | e edit | d drop | v preview | PgUp/Dn preview | Enter confirm | Esc cancel".into()
    } else if app.searching() {
        format!(
            "Find: {}_ (Enter search, Esc cancel)",
            safe_text(app.search())
        )
    } else {
        " : commands | tab view | I recent | j/k select | PgUp/PgDn diff | / find | n next | d base | v seen | r refresh | q quit".into()
    };
    let settings = app
        .persistence_error()
        .map(|error| format!(" | Settings: {}", safe_text(error).replace('\n', " | ")))
        .unwrap_or_default();
    let message_style = if app.message_is_error() {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };
    let footer = Paragraph::new(vec![
        Line::from(input),
        Line::from(shortcuts(app.view())),
        Line::from(Span::styled(
            format!(
                "Action: {}{settings}",
                safe_text(app.message()).replace('\n', " | ")
            ),
            message_style,
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("Help"));
    frame.render_widget(footer, area);
}

fn render_palette(frame: &mut Frame<'_>, app: &App, query: &str) {
    let area = frame.area();
    let panel = Rect::new(
        area.x + 2,
        area.y + 2,
        area.width.saturating_sub(4),
        area.height.saturating_sub(4),
    );
    frame.render_widget(Clear, panel);
    let entries = crate::commands::filtered(app.view(), query);
    let items: Vec<ListItem> = if entries.is_empty() {
        vec![ListItem::new("No matching commands")]
    } else {
        entries
            .iter()
            .map(|entry| ListItem::new(entry.title))
            .collect()
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Commands: {}_ | ↑/↓ select, Enter execute, Esc cancel",
            safe_text(query)
        )))
        .highlight_style(Style::default().fg(Color::Yellow))
        .highlight_symbol("> ");
    let mut selection = ListState::default()
        .with_selected((!entries.is_empty()).then_some(app.palette_selection()));
    frame.render_stateful_widget(list, panel, &mut selection);
}

/// Key hints for the current view; global keys are on the footer's first line.
fn shortcuts(view: ViewMode) -> &'static str {
    match view {
        ViewMode::RecentRepositories => {
            "enter open | N initialize | C clone Git | D forget entry | O open path"
        }
        ViewMode::Hunks => {
            "Space select lines | = numbers | s stage hunk | u unstage hunk | d staged/unstaged | tab next view"
        }
        ViewMode::Tags => "N create | D delete local | W publish | X delete remote | r refresh",
        ViewMode::Remotes => {
            "N add | L fetch URL | H push URL | D remove | f fetch | W publish | U upstream | l pull strategy"
        }
        ViewMode::RemoteBranches => "enter track | f fetch | U upstream | W publish",
        ViewMode::Stashes => "S save | y apply | T pop | D drop | r refresh",
        ViewMode::Files => {
            "\\ filter | Space mark | $ stash selected | J tracked/changes | h history | Q blame | s stage | u unstage | i hunks | w restore index | D discard HEAD | c commit | b branch | f fetch | p pull | P push"
        }
        ViewMode::History => {
            "~ interactive rebase | ! export patch | g graph/patch | + older | Y cherry-pick | Z revert | F reset"
        }
        ViewMode::Reflog => "+ older entries | N recover into new branch",
        ViewMode::Branches => {
            "\\ filter | enter switch | b new | B rename | D delete | m merge | z rebase"
        }
        ViewMode::GitHubRepositories => {
            "enter clone | N create repository | G auth status | O open local repository"
        }
        ViewMode::Worktrees => "enter open | N create | D remove | O open repository",
        ViewMode::PrFiles => "j/k file | C line comment | PgUp/PgDn patch | r refresh",
        ViewMode::PullRequests => {
            "i files | U filter | + more | L edit | T draft/ready | M merge | A/R review | C comment | V diff"
        }
        ViewMode::Conflicts => "o ours | t theirs | a resolved | e continue | K skip | x abort",
    }
}

fn list_items(app: &App) -> (Vec<ListItem<'static>>, bool) {
    let (lines, empty): (Vec<String>, &str) = match app.view() {
        ViewMode::RecentRepositories => (
            app.navigation()
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
            app.hunks()
                .items
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
            app.tags()
                .items
                .iter()
                .map(|t| format!("{} [{}] {}", t.name, t.kind, t.subject))
                .collect(),
            "No tags",
        ),
        ViewMode::Remotes => (
            app.remotes().items.iter().map(|r| r.name.clone()).collect(),
            "No remotes",
        ),
        ViewMode::RemoteBranches => (app.remote_branches().items.clone(), "No remote branches"),
        ViewMode::Stashes => (
            app.stashes()
                .items
                .iter()
                .map(|s| format!("{} {}", s.selector, s.subject))
                .collect(),
            "No stashes",
        ),
        ViewMode::GitHubRepositories => (
            app.github_repositories()
                .items
                .iter()
                .map(|r| r.name_with_owner.clone())
                .collect(),
            "No repositories",
        ),
        ViewMode::Worktrees => (
            app.workspaces()
                .items
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
            "No worktrees",
        ),
        ViewMode::PrFiles => (
            app.pr_files()
                .items
                .iter()
                .map(|file| format!("{} {}", file.status, file.filename.escape_debug()))
                .collect(),
            "No PR files loaded",
        ),
        ViewMode::PullRequests => (
            app.pull_requests()
                .items
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
                        "{}[{marker}] [{}] {}{}",
                        if app.stash_files().contains(&file.path) {
                            "[x] "
                        } else {
                            ""
                        },
                        file.status,
                        file.label(),
                        if app.file_seen(file) { " [seen]" } else { "" }
                    )
                })
                .collect(),
            "No changes",
        ),
        ViewMode::Reflog => (
            app.reflog()
                .items
                .iter()
                .map(|e| {
                    let label = if e.selector.starts_with("refs/git-wirdo/recovery/") {
                        "recovery"
                    } else {
                        &e.selector
                    };
                    format!(
                        "{} {} {}",
                        label,
                        e.sha.get(..12).unwrap_or(&e.sha),
                        e.subject
                    )
                })
                .collect(),
            "No reflog entries",
        ),
        ViewMode::History => (
            app.state()
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
            app.state()
                .branches
                .iter()
                .map(|branch| format!("{} {}", if branch.current { "*" } else { " " }, branch.name))
                .collect(),
            "No branches",
        ),
        ViewMode::Conflicts => (
            app.state()
                .merge_state
                .conflicts
                .iter()
                .map(|path| display_path(path))
                .collect(),
            "No conflicted files",
        ),
    };
    let lines = if !app.name_filter().is_empty() {
        app.visible_indices()
            .into_iter()
            .filter_map(|i| lines.get(i).cloned())
            .collect()
    } else {
        lines
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

    #[test]
    fn every_view_has_shortcut_hints_and_a_list_state() {
        for view in ViewMode::ALL {
            assert!(!shortcuts(view).is_empty());
            assert!(view.index() < ViewMode::ALL.len());
        }
        assert_eq!(ViewMode::RecentRepositories.next(), ViewMode::Files);
    }
}
