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
    /// Print repository status without starting an interactive terminal UI.
    #[arg(long)]
    headless: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = match cli.repo {
        Some(path) => path,
        None => env::current_dir().context("Cannot determine the current directory")?,
    };
    let repository = Repository::open(&path)?;
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
