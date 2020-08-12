//! Versio is a version management utility.

#![recursion_limit = "1024"]

#[macro_use]
pub mod errors;
pub mod analyze;
pub mod config;
pub mod either;
pub mod git;
pub mod github;
pub mod mark;
pub mod mono;
pub mod opts;
pub mod output;
pub mod scan;
pub mod state;
pub mod vcs;
