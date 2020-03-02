//! The command-line options for the sorcery executable.

use crate::config::read_config;
use crate::error::Result;
use clap::{crate_version, App, AppSettings, ArgMatches, SubCommand};
use git2::Repository;

pub fn execute() -> Result<()> {
  let m = App::new("versio")
    .setting(AppSettings::UnifiedHelpMessage)
    .author("Charlie Ozinga, charlie@cloud-elements.com")
    .version(concat!(crate_version!(), " (", env!("GIT_SHORT_HASH"), ")"))
    .about("Manage version numbers.")
    .subcommand(
      SubCommand::with_name("show")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Show all keys and values")
        .display_order(1)
    )
    .get_matches();

  parse_matches(m)
}

fn parse_matches(m: ArgMatches) -> Result<()> {
  match m.subcommand() {
    ("show", _) => show(),
    ("", _) => empty_cmd(),
    (c, _) => unknown_cmd(c)
  }
}

pub fn show() -> Result<()> {
  let repo = Repository::open(".")?;
  let workdir = repo.workdir().ok_or_else(|| versio_error!("No working directory."))?;

  let config = read_config(workdir.join(".versio.yaml"))?;
  config.show()?;

  Ok(())
}

pub fn unknown_cmd(c: &str) -> Result<()> { versio_err!("Unknown command: \"{}\" (try \"help\").", c) }

pub fn empty_cmd() -> Result<()> { versio_err!("No command (try \"help\").") }
