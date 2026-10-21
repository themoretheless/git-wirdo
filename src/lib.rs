//! The terminal client. All behaviour lives in `git-wirdo-core`; its modules are
//! re-exported so `git_wirdo::app`, `git_wirdo::git` and friends keep working.
pub use git_wirdo_core::{
    app, commands, git, github, input, keys, model, patch, pr_review, process, session, settings,
};

pub mod ui;

#[cfg(test)]
extern crate self as git_wirdo;
#[cfg(test)]
mod app_render_tests;
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;
