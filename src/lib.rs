pub mod app;
pub mod git;
pub mod github;
pub mod input;
pub mod model;
pub mod patch;
pub mod pr_review;
pub mod process;
pub mod session;
pub mod settings;
pub mod ui;

#[cfg(test)]
extern crate self as git_wirdo;
#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;
