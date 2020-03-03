//! The command-line options for the sorcery executable.

use crate::config::{load_config, Config};
use crate::error::Result;
use crate::git::pull_ff_only;
use clap::{crate_version, App, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};
use git2::Repository;

pub fn execute() -> Result<()> {
  let m = App::new("versio")
    .setting(AppSettings::UnifiedHelpMessage)
    .author("Charlie Ozinga, charlie@cloud-elements.com")
    .version(concat!(crate_version!(), " (", env!("GIT_SHORT_HASH"), ")"))
    .about("Manage version numbers.")
    .subcommand(
      SubCommand::with_name("pull")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Pull the repo.")
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("show")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Show all versions.")
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("get")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Show one or more versions.")
        .arg(
          Arg::with_name("versiononly")
            .short("v")
            .long("version-only")
            .takes_value(false)
            .display_order(1)
            .help("Only print the version number")
        )
        .arg(
          Arg::with_name("name")
            .short("n")
            .long("name")
            .takes_value(true)
            .value_name("name")
            .display_order(1)
            .help("The name to get")
        )
        .arg(
          Arg::with_name("id")
            .short("i")
            .long("id")
            .takes_value(true)
            .value_name("id")
            .display_order(1)
            .help("The id to get")
        )
        .group(ArgGroup::with_name("ident").args(&["id", "name"]).required(true))
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("set")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Set a version.")
        .arg(
          Arg::with_name("name")
            .short("n")
            .long("name")
            .takes_value(true)
            .value_name("name")
            .display_order(1)
            .help("The name to set")
        )
        .arg(
          Arg::with_name("id")
            .short("i")
            .long("id")
            .takes_value(true)
            .value_name("id")
            .display_order(1)
            .help("The id to get")
        )
        .group(ArgGroup::with_name("ident").args(&["id", "name"]).required(true))
        .arg(
          Arg::with_name("value")
            .short("v")
            .long("value")
            .takes_value(true)
            .value_name("value")
            .display_order(2)
            .required(true)
            .help("The value to set to")
        )
        .display_order(1)
    )
    .get_matches();

  parse_matches(m)
}

fn parse_matches(m: ArgMatches) -> Result<()> {
  match m.subcommand() {
    ("show", _) => show(),
    ("get", Some(m)) => {
      if m.is_present("id") {
        get_id(m.value_of("id").unwrap(), m.is_present("versiononly"))
      } else {
        get_name(m.value_of("name").unwrap(), m.is_present("versiononly"))
      }
    }
    ("set", Some(m)) => {
      if m.is_present("id") {
        set_by_id(m.value_of("id").unwrap(), m.value_of("value").unwrap())
      } else {
        set_by_name(m.value_of("name").unwrap(), m.value_of("value").unwrap())
      }
    }
    ("pull", _) => pull_ff_only(None, None),
    ("", _) => empty_cmd(),
    (c, _) => unknown_cmd(c)
  }
}

fn get_config() -> Result<Config> {
  let repo = Repository::open(".")?;
  let workdir = repo.workdir().ok_or_else(|| versio_error!("No working directory."))?;

  let config = load_config(workdir)?;
  Ok(config)
}

fn show() -> Result<()> { get_config()?.show() }

fn get_name(name: &str, vonly: bool) -> Result<()> { get_config()?.get_name(name, vonly) }

fn get_id(id: &str, vonly: bool) -> Result<()> { get_config()?.get_id(id.parse()?, vonly) }

fn set_by_name(name: &str, val: &str) -> Result<()> { get_config()?.set_by_name(name, val) }

fn set_by_id(id: &str, val: &str) -> Result<()> { get_config()?.set_by_id(id.parse()?, val) }

fn unknown_cmd(c: &str) -> Result<()> { versio_err!("Unknown command: \"{}\" (try \"help\").", c) }

fn empty_cmd() -> Result<()> { versio_err!("No command (try \"help\").") }
