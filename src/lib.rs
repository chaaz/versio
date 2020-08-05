//! Versio is a version management utility.

// #[macro_use]
// pub mod error;
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

// #[macro_use]
// extern crate error_chain;

#![recursion_limit = "1024"]

pub mod error {
  error_chain::error_chain! {}
}
