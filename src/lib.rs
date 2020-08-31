//! Versio is a version management utility.

#![recursion_limit = "1024"]

#[macro_use]
pub mod errors;
pub mod commands;
pub mod init;
pub(crate) mod scan;
pub mod vcs;

mod analyze;
mod config;
mod either;
mod git;
mod github;
mod mark;
mod mono;
mod output;
mod state;
