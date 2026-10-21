//! Everything git-wirdo does, independent of how it is shown.
//!
//! `app` and `session` hold the interactive state machine (views, selections, prompts,
//! background work) and take toolkit-neutral [`keys::KeyEvent`]s; the remaining modules
//! run Git and GitHub and persist settings. A front end only translates input into key
//! events and draws what `App` exposes.
pub mod app;
pub mod commands;
pub mod git;
pub mod github;
pub mod input;
pub mod keys;
pub mod model;
pub mod patch;
pub mod pr_review;
pub mod process;
pub mod session;
pub mod settings;

#[cfg(test)]
extern crate self as git_wirdo;
#[cfg(test)]
#[path = "../../../tests/support/mod.rs"]
mod test_support;
