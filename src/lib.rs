//! Versio is a version management utility.

#![recursion_limit = "1024"]

#[macro_use]
mod errors;
mod analyze;
mod config;
mod either;
mod git;
mod github;
mod mark;
mod mono;
pub mod opts;
mod output;
mod scan;
mod state;
mod vcs;
