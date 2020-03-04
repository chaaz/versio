//! The command-line options for the sorcery executable.

use crate::config::Config;
use crate::error::Result;
use crate::git::pull_ff_only;
use crate::{CurrentSource, PrevSource, Source};
use clap::{crate_version, App, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};
use git2::Repository;

pub fn execute() -> Result<()> {
  let m = App::new("versio")
    .setting(AppSettings::UnifiedHelpMessage)
    .author("Charlie Ozinga, charlie@cloud-elements.com")
    .version(concat!(crate_version!(), " (", env!("GIT_SHORT_HASH"), ")"))
    .about("Manage version numbers")
    .subcommand(
      SubCommand::with_name("pull").setting(AppSettings::UnifiedHelpMessage).about("Pull the repo").display_order(1)
    )
    .subcommand(
      SubCommand::with_name("show")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Show all versions")
        .arg(
          Arg::with_name("prev")
            .short("p")
            .long("prev")
            .takes_value(false)
            .display_order(1)
            .help("Whether to show prev versions")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("get")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Show one or more versions")
        .arg(
          Arg::with_name("prev")
            .short("p")
            .long("prev")
            .takes_value(false)
            .display_order(1)
            .help("Whether to show prev versions")
        )
        .arg(
          Arg::with_name("versiononly")
            .short("v")
            .long("version-only")
            .takes_value(false)
            .display_order(1)
            .help("Only show the version number")
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
        .about("Set a version")
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
            .help("The id to set")
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
  let prev = PrevSource::open(".")?;
  let curt = CurrentSource::open(".")?;

  match m.subcommand() {
    ("show", Some(m)) => {
      if m.is_present("prev") {
        show(prev)
      } else {
        show(curt)
      }
    }
    ("get", Some(m)) => {
      if m.is_present("prev") {
        if m.is_present("id") {
          get_id(prev, m.value_of("id").unwrap(), m.is_present("versiononly"))
        } else {
          get_name(prev, m.value_of("name").unwrap(), m.is_present("versiononly"))
        }
      } else if m.is_present("id") {
        get_id(curt, m.value_of("id").unwrap(), m.is_present("versiononly"))
      } else {
        get_name(curt, m.value_of("name").unwrap(), m.is_present("versiononly"))
      }
    }
    ("set", Some(m)) => {
      if m.is_present("id") {
        set_by_id(m.value_of("id").unwrap(), m.value_of("value").unwrap())
      } else {
        set_by_name(m.value_of("name").unwrap(), m.value_of("value").unwrap())
      }
    }
    ("pull", _) => pull_ff_only(&Repository::open(".")?, None, None),
    ("", _) => empty_cmd(),
    (c, _) => unknown_cmd(c)
  }
}

fn show<S: Source>(source: S) -> Result<()> { Config::from_source(source)?.show() }

fn current_config() -> Result<Config<CurrentSource>> { Config::from_source(CurrentSource::open(".")?) }

fn get_name<S: Source>(src: S, name: &str, vonly: bool) -> Result<()> {
  Config::from_source(src)?.get_name(name, vonly)
}

fn get_id<S: Source>(src: S, id: &str, vonly: bool) -> Result<()> {
  Config::from_source(src)?.get_id(id.parse()?, vonly)
}

fn set_by_name(name: &str, val: &str) -> Result<()> { current_config()?.set_by_name(name, val) }

fn set_by_id(id: &str, val: &str) -> Result<()> { current_config()?.set_by_id(id.parse()?, val) }

fn unknown_cmd(c: &str) -> Result<()> { versio_err!("Unknown command: \"{}\" (try \"help\").", c) }

fn empty_cmd() -> Result<()> { versio_err!("No command (try \"help\").") }
