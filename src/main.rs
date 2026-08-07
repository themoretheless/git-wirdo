use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

#[derive(Debug, Clone)]
struct FileEntry {
    path: String,
    status: String,
    staged: bool,
    unstaged: bool,
}

#[derive(Debug, Clone)]
struct BranchEntry {
    name: String,
    current: bool,
}

#[derive(Debug, Clone)]
struct CommitEntry {
    sha: String,
    short_sha: String,
    date: String,
    author: String,
    subject: String,
}

#[derive(Debug, Clone)]
struct MergeState {
    merge_in_progress: bool,
    rebase_in_progress: bool,
    conflicts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Files,
    History,
    Branches,
    Conflicts,
}

#[derive(Debug)]
struct RepoState {
    branch: String,
    summary: String,
    files: Vec<FileEntry>,
    branches: Vec<BranchEntry>,
    commits: Vec<CommitEntry>,
    merge_state: MergeState,
    message: String,
}

#[derive(Debug)]
struct App {
    repo_path: PathBuf,
    state: RepoState,
    view: ViewMode,
    file_selection: usize,
    history_selection: usize,
    branch_selection: usize,
    conflict_selection: usize,
    detail_text: String,
    running: bool,
}

#[derive(Debug, Parser)]
#[command(name = "git-wirdo", version, about = "A Rust-based Git workflow UI")]
struct Cli {
    #[arg(long, value_name = "PATH")]
    repo: Option<PathBuf>,
    #[arg(long)]
    headless: bool,
}

impl App {
    fn new(repo_path: PathBuf) -> Result<Self> {
        let mut app = Self {
            repo_path: repo_path.clone(),
            state: load_repo_state(&repo_path)?,
            view: ViewMode::Files,
            file_selection: 0,
            history_selection: 0,
            branch_selection: 0,
            conflict_selection: 0,
            detail_text: String::new(),
            running: true,
        };
        app.clamp_selections();
        app.refresh_detail();
        Ok(app)
    }

    fn refresh(&mut self) -> Result<()> {
        let message = self.state.message.clone();
        self.state = load_repo_state(&self.repo_path)?;
        self.state.message = message;
        self.clamp_selections();
        self.refresh_detail();
        Ok(())
    }

    fn clamp_selections(&mut self) {
        self.file_selection = self
            .file_selection
            .min(self.state.files.len().saturating_sub(1));
        self.history_selection = self
            .history_selection
            .min(self.state.commits.len().saturating_sub(1));
        self.branch_selection = self
            .branch_selection
            .min(self.state.branches.len().saturating_sub(1));
        self.conflict_selection = self
            .conflict_selection
            .min(self.state.merge_state.conflicts.len().saturating_sub(1));
    }

    fn refresh_detail(&mut self) {
        self.detail_text = match self.view {
            ViewMode::Files => self.selected_file_detail(),
            ViewMode::History => self.selected_commit_detail(),
            ViewMode::Branches => self.selected_branch_detail(),
            ViewMode::Conflicts => self.selected_conflict_detail(),
        };
    }

    fn move_selection(&mut self, delta: isize) {
        match self.view {
            ViewMode::Files => {
                if self.state.files.is_empty() {
                    return;
                }
                let len = self.state.files.len();
                self.file_selection =
                    (self.file_selection as isize + delta).clamp(0, len as isize - 1) as usize;
            }
            ViewMode::History => {
                if self.state.commits.is_empty() {
                    return;
                }
                let len = self.state.commits.len();
                self.history_selection =
                    (self.history_selection as isize + delta).clamp(0, len as isize - 1) as usize;
            }
            ViewMode::Branches => {
                if self.state.branches.is_empty() {
                    return;
                }
                let len = self.state.branches.len();
                self.branch_selection =
                    (self.branch_selection as isize + delta).clamp(0, len as isize - 1) as usize;
            }
            ViewMode::Conflicts => {
                if self.state.merge_state.conflicts.is_empty() {
                    return;
                }
                let len = self.state.merge_state.conflicts.len();
                self.conflict_selection =
                    (self.conflict_selection as isize + delta).clamp(0, len as isize - 1) as usize;
            }
        }
        self.refresh_detail();
    }

    fn cycle_view(&mut self) {
        self.view = match self.view {
            ViewMode::Files => ViewMode::History,
            ViewMode::History => ViewMode::Branches,
            ViewMode::Branches => ViewMode::Conflicts,
            ViewMode::Conflicts => ViewMode::Files,
        };
        self.refresh_detail();
    }

    fn selected_file_detail(&self) -> String {
        let Some(file) = self.state.files.get(self.file_selection) else {
            return "No files selected".to_string();
        };
        let staged_output =
            run_git_optional(&self.repo_path, &["diff", "--cached", "--", &file.path]);
        let unstaged_output = run_git_optional(&self.repo_path, &["diff", "--", &file.path]);
        let mut chunks = Vec::new();
        if !staged_output.is_empty() {
            chunks.push("[STAGED]".to_string());
            chunks.push(staged_output);
        }
        if !unstaged_output.is_empty() {
            if !chunks.is_empty() {
                chunks.push(String::new());
            }
            chunks.push("[UNSTAGED]".to_string());
            chunks.push(unstaged_output);
        }
        if chunks.is_empty() {
            "No diff for this file".to_string()
        } else {
            chunks.join("\n")
        }
    }

    fn selected_commit_detail(&self) -> String {
        let Some(commit) = self.state.commits.get(self.history_selection) else {
            return "No commit selected".to_string();
        };
        run_git_optional(
            &self.repo_path,
            &["show", "--stat", "--decorate", "--oneline", &commit.sha],
        )
    }

    fn selected_branch_detail(&self) -> String {
        let Some(branch) = self.state.branches.get(self.branch_selection) else {
            return "No branch selected".to_string();
        };
        if branch.current {
            format!(
                "Current branch: {}\n\nPress Enter to refresh the current branch view.",
                branch.name
            )
        } else {
            format!(
                "Branch: {}\n\nPress Enter to checkout this branch.",
                branch.name
            )
        }
    }

    fn selected_conflict_detail(&self) -> String {
        let header = merge_state_summary(&self.state.merge_state);
        let Some(path) = self
            .state
            .merge_state
            .conflicts
            .get(self.conflict_selection)
        else {
            return format!(
                "{header}\n\nNo conflicted file selected.\n\nKeys: o take ours | t take theirs | a add | e continue | k skip | x abort"
            );
        };
        let diff = run_git_optional(&self.repo_path, &["diff", "--", path]);
        if diff.is_empty() {
            format!(
                "{header}\n\nConflict: {path}\n\nNo diff preview.\n\nKeys: o take ours | t take theirs | a add | e continue | k skip | x abort"
            )
        } else {
            format!(
                "{header}\n\nConflict: {path}\n\n{diff}\n\nKeys: o take ours | t take theirs | a add | e continue | k skip | x abort"
            )
        }
    }

    fn stage_selected(&mut self) -> Result<()> {
        if let Some(file) = self.state.files.get(self.file_selection) {
            run_git(&self.repo_path, &["add", "--", &file.path])?;
            self.state.message = format!("Staged {}", file.path);
            self.refresh()?;
        }
        Ok(())
    }

    fn unstage_selected(&mut self) -> Result<()> {
        if let Some(file) = self.state.files.get(self.file_selection) {
            run_git(&self.repo_path, &["reset", "HEAD", "--", &file.path])?;
            self.state.message = format!("Unstaged {}", file.path);
            self.refresh()?;
        }
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        run_git(&self.repo_path, &["commit", "-m", "wirdo update"])?;
        self.state.message = "Committed changes".to_string();
        self.refresh()?;
        Ok(())
    }

    fn branch(&mut self) -> Result<()> {
        let branch_name = "feature/wirdo";
        let checkout = run_git(&self.repo_path, &["checkout", branch_name]);
        if checkout.is_err() {
            run_git(&self.repo_path, &["checkout", "-b", branch_name])?;
        }
        self.state.message = format!("Switched to {branch_name}");
        self.refresh()?;
        Ok(())
    }

    fn checkout_selected_branch(&mut self) -> Result<()> {
        let Some(branch) = self.state.branches.get(self.branch_selection) else {
            return Ok(());
        };
        if branch.current {
            self.state.message = format!("Already on {}", branch.name);
            self.refresh()?;
            return Ok(());
        }
        run_git(&self.repo_path, &["checkout", &branch.name])?;
        self.state.message = format!("Checked out {}", branch.name);
        self.refresh()?;
        Ok(())
    }

    fn take_conflict_side(&mut self, side: &str) -> Result<()> {
        let Some(path) = self
            .state
            .merge_state
            .conflicts
            .get(self.conflict_selection)
            .cloned()
        else {
            return Ok(());
        };
        run_git(&self.repo_path, &["checkout", side, "--", &path])?;
        run_git(&self.repo_path, &["add", "--", &path])?;
        self.state.message = format!("Resolved {path} with {side}");
        self.refresh()?;
        Ok(())
    }

    fn stage_selected_conflict(&mut self) -> Result<()> {
        let Some(path) = self
            .state
            .merge_state
            .conflicts
            .get(self.conflict_selection)
            .cloned()
        else {
            return Ok(());
        };
        run_git(&self.repo_path, &["add", "--", &path])?;
        self.state.message = format!("Marked {path} as resolved");
        self.refresh()?;
        Ok(())
    }

    fn continue_rebase_or_merge(&mut self) -> Result<()> {
        if self.state.merge_state.rebase_in_progress {
            run_git(&self.repo_path, &["rebase", "--continue"])?;
            self.state.message = "Continued rebase".to_string();
        } else if self.state.merge_state.merge_in_progress {
            run_git(&self.repo_path, &["commit", "--no-edit"])?;
            self.state.message = "Completed merge".to_string();
        } else {
            self.state.message = "No merge or rebase in progress".to_string();
        }
        self.refresh()?;
        Ok(())
    }

    fn skip_rebase(&mut self) -> Result<()> {
        if self.state.merge_state.rebase_in_progress {
            run_git(&self.repo_path, &["rebase", "--skip"])?;
            self.state.message = "Skipped current rebase commit".to_string();
            self.refresh()?;
        } else {
            self.state.message = "No rebase in progress".to_string();
            self.refresh_detail();
        }
        Ok(())
    }

    fn abort_operation(&mut self) -> Result<()> {
        if self.state.merge_state.rebase_in_progress {
            run_git(&self.repo_path, &["rebase", "--abort"])?;
            self.state.message = "Aborted rebase".to_string();
        } else if self.state.merge_state.merge_in_progress {
            run_git(&self.repo_path, &["merge", "--abort"])?;
            self.state.message = "Aborted merge".to_string();
        } else {
            self.state.message = "No merge or rebase in progress".to_string();
        }
        self.refresh()?;
        Ok(())
    }

    fn fetch(&mut self) -> Result<()> {
        run_git(&self.repo_path, &["fetch", "--all"])?;
        self.state.message = "Fetched remotes".to_string();
        self.refresh()?;
        Ok(())
    }

    fn pull(&mut self) -> Result<()> {
        run_git(&self.repo_path, &["pull", "--ff-only"])?;
        self.state.message = "Pulled changes".to_string();
        self.refresh()?;
        Ok(())
    }

    fn push(&mut self) -> Result<()> {
        run_git(&self.repo_path, &["push"])?;
        self.state.message = "Pushed changes".to_string();
        self.refresh()?;
        Ok(())
    }

    fn handle_key(&mut self, key: KeyCode) -> Result<bool> {
        match key {
            KeyCode::Char('q') => {
                self.running = false;
                Ok(true)
            }
            KeyCode::Char('r') => {
                self.refresh()?;
                Ok(false)
            }
            KeyCode::Tab => {
                self.cycle_view();
                Ok(false)
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                Ok(false)
            }
            KeyCode::Up => {
                self.move_selection(-1);
                Ok(false)
            }
            KeyCode::Enter => {
                if self.view == ViewMode::Branches {
                    self.checkout_selected_branch()?;
                }
                Ok(false)
            }
            KeyCode::Char('s') => {
                self.stage_selected()?;
                Ok(false)
            }
            KeyCode::Char('u') => {
                self.unstage_selected()?;
                Ok(false)
            }
            KeyCode::Char('c') => {
                self.commit()?;
                Ok(false)
            }
            KeyCode::Char('b') => {
                self.branch()?;
                Ok(false)
            }
            KeyCode::Char('f') => {
                self.fetch()?;
                Ok(false)
            }
            KeyCode::Char('p') => {
                self.pull()?;
                Ok(false)
            }
            KeyCode::Char('P') => {
                self.push()?;
                Ok(false)
            }
            KeyCode::Char('o') => {
                if self.view == ViewMode::Conflicts {
                    self.take_conflict_side("--ours")?;
                }
                Ok(false)
            }
            KeyCode::Char('t') => {
                if self.view == ViewMode::Conflicts {
                    self.take_conflict_side("--theirs")?;
                }
                Ok(false)
            }
            KeyCode::Char('a') => {
                if self.view == ViewMode::Conflicts {
                    self.stage_selected_conflict()?;
                }
                Ok(false)
            }
            KeyCode::Char('e') => {
                if self.view == ViewMode::Conflicts {
                    self.continue_rebase_or_merge()?;
                }
                Ok(false)
            }
            KeyCode::Char('k') => {
                if self.view == ViewMode::Conflicts {
                    self.skip_rebase()?;
                } else {
                    self.move_selection(-1);
                }
                Ok(false)
            }
            KeyCode::Char('x') => {
                if self.view == ViewMode::Conflicts {
                    self.abort_operation()?;
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git with args: {args:?}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        bail!(if stderr.is_empty() { stdout } else { stderr });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_git_optional(repo_path: &Path, args: &[&str]) -> String {
    match run_git(repo_path, args) {
        Ok(output) => output,
        Err(_) => String::new(),
    }
}

fn merge_state_summary(merge_state: &MergeState) -> String {
    let mut lines: Vec<String> = Vec::new();
    if merge_state.merge_in_progress {
        lines.push("Merge in progress".to_string());
    }
    if merge_state.rebase_in_progress {
        lines.push("Rebase in progress".to_string());
    }
    if lines.is_empty() {
        lines.push("No merge or rebase in progress".to_string());
    }
    if merge_state.conflicts.is_empty() {
        lines.push("Conflicts: none".to_string());
    } else {
        lines.push(format!("Conflicts: {}", merge_state.conflicts.len()));
    }
    lines.join("\n")
}

fn load_merge_state(repo_path: &Path) -> MergeState {
    let git_dir = repo_path.join(".git");
    let merge_in_progress = git_dir.join("MERGE_HEAD").exists();
    let rebase_in_progress =
        git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists();
    let conflicts_output = run_git_optional(repo_path, &["diff", "--name-only", "--diff-filter=U"]);
    let conflicts = conflicts_output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    MergeState {
        merge_in_progress,
        rebase_in_progress,
        conflicts,
    }
}

fn load_repo_state(repo_path: &Path) -> Result<RepoState> {
    let branch = run_git_optional(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let branch = if branch.is_empty() {
        "HEAD".to_string()
    } else {
        branch
    };
    let status_text = run_git(repo_path, &["status", "--short", "--branch"])?;
    let mut files = Vec::new();
    for line in status_text.lines() {
        if line.is_empty() || line.starts_with("##") {
            continue;
        }
        let status = line.get(0..2).unwrap_or("  ").to_string();
        let path = line.get(3..).unwrap_or(line).trim().to_string();
        let staged = !status.starts_with(' ') && status.chars().next() != Some('?');
        let unstaged = !status.ends_with(' ') && status.chars().nth(1) != Some('?');
        files.push(FileEntry {
            path,
            status,
            staged,
            unstaged,
        });
    }
    let branch_output = run_git_optional(repo_path, &["branch", "--list"]);
    let mut branches = Vec::new();
    for line in branch_output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let current = trimmed.starts_with('*');
        let name = trimmed.trim_start_matches('*').trim().to_string();
        if !name.is_empty() {
            branches.push(BranchEntry { name, current });
        }
    }
    let log_output = run_git_optional(
        repo_path,
        &[
            "log",
            "-n",
            "20",
            "--date=short",
            "--pretty=format:%H%x00%h%x00%ad%x00%an%x00%s",
        ],
    );
    let mut commits = Vec::new();
    for line in log_output.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\0').collect();
        if parts.len() >= 5 {
            commits.push(CommitEntry {
                sha: parts[0].to_string(),
                short_sha: parts[1].to_string(),
                date: parts[2].to_string(),
                author: parts[3].to_string(),
                subject: parts[4].to_string(),
            });
        }
    }
    let merge_state = load_merge_state(repo_path);
    let summary = if files.is_empty() {
        "clean".to_string()
    } else {
        status_text.to_string()
    };
    Ok(RepoState {
        branch,
        summary,
        files,
        branches,
        commits,
        merge_state,
        message: "Ready".to_string(),
    })
}

fn draw_ui<T>(terminal: &mut Terminal<T>, app: &App) -> Result<()>
where
    T: ratatui::backend::Backend,
    T::Error: std::error::Error + Send + Sync + 'static,
{
    terminal.draw(|frame| {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(8), Constraint::Length(3)])
            .split(frame.area());

        let header = Paragraph::new(vec![
            Line::from(Span::styled("Git Wirdo", Style::default().fg(Color::Cyan))),
            Line::from(format!("Repo: {}", app.repo_path.display())),
            Line::from(format!("Branch: {}", app.state.branch)),
            Line::from(format!("View: {:?}", app.view)),
            Line::from(merge_state_summary(&app.state.merge_state)),
        ])
        .block(Block::default().borders(Borders::ALL).title("Status"));
        frame.render_widget(header, layout[0]);

        let body_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(layout[1]);

        let left_lines = render_list(app);
        let left = Paragraph::new(left_lines)
            .block(Block::default().borders(Borders::ALL).title("Selection"))
            .wrap(Wrap { trim: true });
        frame.render_widget(left, body_layout[0]);

        let right = Paragraph::new(app.detail_text.clone())
            .block(Block::default().borders(Borders::ALL).title("Details"))
            .wrap(Wrap { trim: true });
        frame.render_widget(right, body_layout[1]);

        let footer = Paragraph::new(vec![
            Line::from("tab view | j/down next | k/up prev | enter checkout branch | s stage | u unstage | c commit | b branch | f fetch | p pull | P push"),
            Line::from("conflicts: o ours | t theirs | a add | e continue | k skip | x abort | q quit"),
            Line::from(format!("Action: {}", app.state.message)),
        ])
        .block(Block::default().borders(Borders::ALL).title("Help"));
        frame.render_widget(footer, layout[2]);
    })?;
    Ok(())
}

fn render_list(app: &App) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    match app.view {
        ViewMode::Files => {
            if app.state.files.is_empty() {
                lines.push(Line::from("No changes"));
            } else {
                for (index, file) in app.state.files.iter().enumerate() {
                    let prefix = if index == app.file_selection {
                        ">"
                    } else {
                        " "
                    };
                    let marker = match (file.staged, file.unstaged) {
                        (true, true) => "SU",
                        (true, false) => "S ",
                        (false, true) => " U",
                        (false, false) => "  ",
                    };
                    lines.push(Line::from(format!(
                        "{prefix} [{marker}] [{}] {}",
                        file.status, file.path
                    )));
                }
            }
        }
        ViewMode::History => {
            if app.state.commits.is_empty() {
                lines.push(Line::from("No commits"));
            } else {
                for (index, commit) in app.state.commits.iter().enumerate() {
                    let prefix = if index == app.history_selection {
                        ">"
                    } else {
                        " "
                    };
                    lines.push(Line::from(format!(
                        "{prefix} {} {} {} {}",
                        commit.short_sha, commit.date, commit.author, commit.subject
                    )));
                }
            }
        }
        ViewMode::Branches => {
            if app.state.branches.is_empty() {
                lines.push(Line::from("No branches"));
            } else {
                for (index, branch) in app.state.branches.iter().enumerate() {
                    let prefix = if index == app.branch_selection {
                        ">"
                    } else {
                        " "
                    };
                    let marker = if branch.current { "*" } else { " " };
                    lines.push(Line::from(format!("{prefix} {marker} {}", branch.name)));
                }
            }
        }
        ViewMode::Conflicts => {
            if app.state.merge_state.conflicts.is_empty() {
                lines.push(Line::from("No conflicted files"));
            } else {
                for (index, path) in app.state.merge_state.conflicts.iter().enumerate() {
                    let prefix = if index == app.conflict_selection {
                        ">"
                    } else {
                        " "
                    };
                    lines.push(Line::from(format!("{prefix} {path}")));
                }
            }
        }
    }
    lines
}

fn run_tui(repo_path: &Path) -> Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(repo_path.to_path_buf())?;
    loop {
        draw_ui(&mut terminal, &app)?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                let should_exit = app.handle_key(key.code)?;
                if !app.running || should_exit {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let repo_path = cli.repo.unwrap_or_else(|| env::current_dir().unwrap());
    if cli.headless {
        let state = load_repo_state(&repo_path)?;
        println!("repo: {}", repo_path.display());
        println!("branch: {}", state.branch);
        println!("summary: {}", state.summary);
        println!(
            "merge: {} rebase: {} conflicts: {}",
            state.merge_state.merge_in_progress,
            state.merge_state.rebase_in_progress,
            state.merge_state.conflicts.len()
        );
        for file in &state.files {
            println!(
                "{} {} {} {}",
                file.status, file.staged, file.unstaged, file.path
            );
        }
        return Ok(());
    }

    run_tui(&repo_path)
}
