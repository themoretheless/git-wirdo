mod terminal;
mod welcome;

use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use git_wirdo::git::Repository;
use git_wirdo::model::display_path;

#[derive(Debug, Parser)]
#[command(name = "git-wirdo", version, about = "A Rust-based Git workflow UI")]
struct Cli {
    /// Use a specific UI state file instead of the user configuration directory.
    #[arg(long, value_name = "PATH", conflicts_with = "no_state")]
    state_file: Option<PathBuf>,
    /// Disable reading/writing saved UI navigation and settings.
    #[arg(long, conflicts_with_all = ["resume", "list_recent"])]
    no_state: bool,
    /// Open the last saved working tree instead of the current directory.
    #[arg(long, conflicts_with = "repo")]
    resume: bool,
    /// List saved recent repository/worktree paths without starting the TUI.
    #[arg(long, group = "inspection")]
    list_recent: bool,
    /// A working tree or any directory inside it (defaults to the current directory).
    #[arg(long, value_name = "PATH")]
    repo: Option<PathBuf>,
    /// Initialize a new repository, retaining existing ordinary files.
    #[arg(long, value_name = "PATH", conflicts_with_all = ["repo", "resume", "clone", "inspection", "github_login"])]
    init: Option<PathBuf>,
    /// Initial branch used with --init.
    #[arg(long, default_value = "main", requires = "init")]
    initial_branch: String,
    /// Clone an arbitrary Git URL or local repository.
    #[arg(long, value_name = "SOURCE", requires = "destination", conflicts_with_all = ["repo", "resume", "init", "inspection", "github_login"])]
    clone: Option<std::ffi::OsString>,
    /// New or empty directory used with --clone.
    #[arg(long, value_name = "PATH", requires = "clone")]
    destination: Option<PathBuf>,
    /// Start with repository selection, even inside an existing checkout.
    #[arg(long, conflicts_with_all = ["repo", "resume", "init", "clone", "inspection", "github_login", "headless"])]
    start: bool,
    /// Authenticate GitHub CLI interactively before opening the TUI.
    #[arg(long)]
    github_login: bool,
    /// List local workspaces without starting the TUI.
    #[arg(long, group = "inspection")]
    list_workspaces: bool,
    /// List saved stashes without starting the TUI.
    #[arg(long, group = "inspection")]
    list_stashes: bool,
    /// List tracked paths from the index (including clean files).
    #[arg(long, group = "inspection")]
    list_files: bool,
    /// Import a mail patch/mbox as commits into a clean checkout.
    #[arg(long, value_name = "PATH", group = "inspection")]
    import_patch: Option<PathBuf>,
    /// Export one non-merge commit as a mail patch without overwriting a file.
    #[arg(
        long,
        value_name = "REVISION",
        group = "inspection",
        requires = "output"
    )]
    export_patch: Option<String>,
    /// New file destination for --export-patch.
    #[arg(long, value_name = "PATH", requires = "export_patch")]
    output: Option<PathBuf>,
    /// Show committed history for a literal path, following renames.
    #[arg(long, value_name = "PATH", group = "inspection")]
    file_history: Option<PathBuf>,
    /// Show line authorship at HEAD for a literal path (pending edits excluded).
    #[arg(long, value_name = "PATH", group = "inspection")]
    blame: Option<PathBuf>,
    /// Maximum file-history commits (1..=10000).
    #[arg(long, default_value = "100", requires = "file_history", value_parser = clap::value_parser!(u32).range(1..=10000))]
    file_limit: u32,
    /// List local tags without starting the TUI.
    #[arg(long, group = "inspection")]
    list_tags: bool,
    /// List HEAD reflog entries without starting the TUI.
    #[arg(long, group = "inspection")]
    list_reflog: bool,
    /// List open GitHub pull requests without starting the TUI.
    #[arg(long, group = "inspection")]
    list_prs: bool,
    /// State used with --list-prs.
    #[arg(long, default_value = "open", value_parser = ["open", "closed", "merged", "all"])]
    pr_state: String,
    /// GitHub search query used with --list-prs.
    #[arg(long, default_value = "")]
    pr_search: String,
    /// Maximum PR list size (increase to load more).
    #[arg(long, default_value_t = 100)]
    pr_limit: usize,
    /// Inspect a PR's files and line-numbered patches.
    #[arg(long, group = "inspection")]
    pr_files: Option<u64>,
    /// List repositories belonging to the authenticated GitHub user.
    #[arg(long, group = "inspection")]
    list_github_repos: bool,
    /// Show a GitHub PR, checks and reviews without starting the TUI.
    #[arg(long, group = "inspection")]
    pr: Option<u64>,
    /// Print repository status without starting an interactive terminal UI.
    #[arg(long)]
    headless: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.github_login {
        let status = std::process::Command::new("gh")
            .args(["auth", "login"])
            .status()
            .context("Install GitHub CLI (gh) first")?;
        anyhow::ensure!(status.success(), "GitHub login failed");
        return Ok(());
    }
    let settings_path = || {
        cli.state_file
            .clone()
            .map(Ok)
            .unwrap_or_else(git_wirdo::settings::default_path)
    };
    if cli.list_recent {
        for settings in git_wirdo::settings::load(&settings_path()?)?.repositories {
            println!("{}", display_path(&settings.root.path()?));
        }
        return Ok(());
    }
    let explicit_repo = cli.repo.is_some();
    let path = match &cli.repo {
        Some(path) => path.clone(),
        None if cli.resume => git_wirdo::settings::load(&settings_path()?)?
            .repositories
            .first()
            .context("No saved recent repository; use --repo PATH")?
            .root
            .path()?,
        None => env::current_dir().context("Cannot determine the current directory")?,
    };
    let start = || {
        welcome::run(
            path.clone(),
            if cli.no_state {
                None
            } else {
                Some(settings_path()?)
            }
            .as_deref(),
        )
    };
    let repository = if cli.start {
        match start()? {
            Some(repository) => repository,
            None => return Ok(()),
        }
    } else if let Some(destination) = cli.init {
        Repository::initialize(&path, &destination, &cli.initial_branch)?
    } else if let Some(source) = cli.clone {
        Repository::clone_into(
            &path,
            &source,
            cli.destination
                .as_deref()
                .expect("clap requires destination"),
        )?
    } else {
        match Repository::open(&path) {
            Ok(repository) => repository,
            Err(error) => {
                use std::io::IsTerminal;
                let inspection = cli.list_files
                    || cli.import_patch.is_some()
                    || cli.export_patch.is_some()
                    || cli.file_history.is_some()
                    || cli.blame.is_some()
                    || cli.list_workspaces
                    || cli.list_stashes
                    || cli.list_tags
                    || cli.list_reflog
                    || cli.list_prs
                    || cli.list_github_repos
                    || cli.pr.is_some()
                    || cli.pr_files.is_some();
                if !cli.resume
                    && !explicit_repo
                    && !cli.headless
                    && !inspection
                    && std::io::stdin().is_terminal()
                    && std::io::stdout().is_terminal()
                {
                    match start()? {
                        Some(repository) => repository,
                        None => return Ok(()),
                    }
                } else {
                    return Err(error);
                }
            }
        }
    };
    if let Some(path) = cli.import_patch {
        repository.import_mail_patch(&path)?;
        println!("Imported mail patch");
        return Ok(());
    }
    if cli.list_files {
        for file in repository.tracked_files()? {
            println!("{}", display_path(&file.path));
        }
        return Ok(());
    }
    if let Some(revision) = cli.export_patch {
        let path = repository.export_commit(
            &revision,
            cli.output.as_deref().context("Missing patch destination")?,
        )?;
        println!("Exported mail patch {}", display_path(&path));
        return Ok(());
    }
    if let Some(path) = cli.file_history {
        print!(
            "{}",
            repository.file_history(&path, cli.file_limit as usize)?
        );
        return Ok(());
    }
    if let Some(path) = cli.blame {
        print!("{}", repository.file_blame(&path)?);
        return Ok(());
    }
    if cli.list_reflog {
        for entry in repository.reflog(100)? {
            println!(
                "{} {} {}",
                entry.selector,
                entry.sha,
                entry.subject.escape_debug()
            );
        }
        return Ok(());
    }
    if cli.list_tags {
        for tag in repository.tags()? {
            println!(
                "{} {} {} {}",
                tag.name,
                tag.sha,
                tag.kind,
                tag.subject.escape_debug()
            );
        }
        return Ok(());
    }
    if cli.list_stashes {
        for stash in repository.stashes()? {
            println!(
                "{} {} {}",
                stash.selector,
                stash.sha,
                stash.subject.escape_debug()
            );
        }
        return Ok(());
    }
    if cli.list_workspaces {
        for workspace in repository.workspaces()? {
            println!(
                "{} {}{}",
                workspace.branch,
                display_path(&workspace.path),
                if workspace.locked { " [locked]" } else { "" }
            );
        }
        return Ok(());
    }
    if let Some(number) = cli.pr_files {
        let pr = git_wirdo::github::get_pr(repository.root(), number)?;
        for file in git_wirdo::pr_review::reviewed_files(repository.root(), &pr)? {
            println!("{}", file.detail(&pr)?);
        }
        return Ok(());
    }
    if cli.list_prs {
        let filter = git_wirdo::github::PrFilter {
            state: cli.pr_state,
            search: cli.pr_search,
            limit: cli.pr_limit,
        };
        for pr in git_wirdo::github::list_filtered(repository.root(), &filter)? {
            println!("#{} {} {}", pr.number, pr.title.escape_debug(), pr.url);
        }
        return Ok(());
    }
    if cli.list_github_repos {
        for repo in git_wirdo::github::repositories(repository.root())? {
            println!("{} {}", repo.name_with_owner, repo.url);
        }
        return Ok(());
    }
    if let Some(number) = cli.pr {
        println!(
            "{}\n{}",
            git_wirdo::github::detail(repository.root(), number)?,
            git_wirdo::pr_review::comments(repository.root(), number)?
        );
        return Ok(());
    }
    if cli.headless {
        let state = repository.load_state()?;
        println!("repo: {}", display_path(repository.root()));
        println!("branch: {}", state.branch.escape_debug());
        println!("summary: {}", state.summary());
        println!(
            "merge: {} rebase: {} conflicts: {}",
            state.merge_state.merge_in_progress,
            state.merge_state.rebase_in_progress,
            state.merge_state.conflicts.len()
        );
        println!(
            "cherry-pick: {} revert: {}",
            state.merge_state.cherry_pick_in_progress, state.merge_state.revert_in_progress
        );
        for file in &state.files {
            println!(
                "{} {} {} {}",
                file.status,
                file.staged,
                file.unstaged,
                file.label()
            );
        }
        return Ok(());
    }
    terminal::run(
        repository,
        if cli.no_state {
            None
        } else {
            Some(settings_path()?)
        },
    )
}
