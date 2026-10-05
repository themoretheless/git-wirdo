mod terminal;

use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use git_wirdo::git::Repository;
use git_wirdo::model::display_path;

#[derive(Debug, Parser)]
#[command(name = "git-wirdo", version, about = "A Rust-based Git workflow UI")]
struct Cli {
    /// A working tree or any directory inside it (defaults to the current directory).
    #[arg(long, value_name = "PATH")]
    repo: Option<PathBuf>,
    /// Authenticate GitHub CLI interactively before opening the TUI.
    #[arg(long)]
    github_login: bool,
    /// List local workspaces without starting the TUI.
    #[arg(long, group = "inspection")]
    list_workspaces: bool,
    /// List saved stashes without starting the TUI.
    #[arg(long, group = "inspection")]
    list_stashes: bool,
    /// List local tags without starting the TUI.
    #[arg(long, group = "inspection")]
    list_tags: bool,
    /// List open GitHub pull requests without starting the TUI.
    #[arg(long, group = "inspection")]
    list_prs: bool,
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
    let path = match cli.repo {
        Some(path) => path,
        None => env::current_dir().context("Cannot determine the current directory")?,
    };
    let repository = Repository::open(&path)?;
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
    if cli.list_prs {
        for pr in git_wirdo::github::list(repository.root())? {
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
        println!("{}", git_wirdo::github::detail(repository.root(), number)?);
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
    terminal::run(repository)
}
