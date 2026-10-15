//! Repository, GitHub and persistence logic shared by every git-wirdo front end.
//!
//! Nothing here depends on a terminal or GUI toolkit: front ends drive these modules and
//! render the results however they like.
pub mod git;
pub mod github;
pub mod model;
pub mod patch;
pub mod pr_review;
pub mod process;
pub mod settings;
