//! The terminal client. Repository, GitHub and settings logic lives in `git-wirdo-core`;
//! its modules are re-exported so `git_wirdo::git` and friends keep working.
pub use git_wirdo_core::{git, github, model, patch, pr_review, process, settings};

pub mod app;
pub mod commands;
pub mod input;
pub mod session;
pub mod ui;

#[cfg(test)]
extern crate self as git_wirdo;
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;
