//! The command-line options for the executable.

use clap::{crate_version, App, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};
use versio::commands::*;
use versio::err;
use versio::errors::Result;
use versio::vcs::{VcsLevel, VcsRange};

pub fn execute() -> Result<()> {
  let m = App::new("versio")
    .setting(AppSettings::UnifiedHelpMessage)
    .author("Charlie Ozinga, ozchaz@gmail.com")
    .version(concat!(crate_version!(), " (", env!("GIT_SHORT_HASH"), " ", env!("DATE_DASH"), ")"))
    .about("Manage version numbers")
    .arg(
      Arg::with_name("vcslevel")
        .short("l")
        .long("vcs-level")
        .takes_value(true)
        .value_name("level")
        .possible_values(&["auto", "max", "none", "local", "remote", "smart"])
        .conflicts_with_all(&["vcslevelmin", "vcslevelmax"])
        .display_order(1)
        .help("The VCS level")
    )
    .arg(
      Arg::with_name("vcslevelmin")
        .short("m")
        .long("vcs-level-min")
        .takes_value(true)
        .value_name("min")
        .possible_values(&["none", "local", "remote", "smart"])
        .requires("vcslevelmax")
        .display_order(1)
        .help("The minimum VCS level")
    )
    .arg(
      Arg::with_name("vcslevelmax")
        .short("x")
        .long("vcs-level-max")
        .takes_value(true)
        .value_name("max")
        .possible_values(&["none", "local", "remote", "smart"])
        .requires("vcslevelmin")
        .display_order(1)
        .help("The maximum VCS level")
    )
    .subcommand(
      SubCommand::with_name("check")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Check current config")
        .display_order(1)
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
            .help("Whether to show prev versions.")
        )
        .arg(
          Arg::with_name("wide")
            .short("w")
            .long("wide")
            .takes_value(false)
            .display_order(1)
            .help("Wide output shows IDs")
        )
        .arg(
          Arg::with_name("nofetch").short("F").long("no-fetch").takes_value(false).display_order(1).help("Don't fetch")
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
          Arg::with_name("nofetch").short("F").long("no-fetch").takes_value(false).display_order(1).help("Don't fetch")
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
          Arg::with_name("wide")
            .short("w")
            .long("wide")
            .takes_value(false)
            .display_order(1)
            .help("Wide output shows IDs")
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
    .subcommand(
      SubCommand::with_name("diff")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("See changes from previous")
        .arg(
          Arg::with_name("nofetch").short("F").long("no-fetch").takes_value(false).display_order(1).help("Don't fetch")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("files")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Stream changed files")
        .arg(
          Arg::with_name("nofetch").short("F").long("no-fetch").takes_value(false).display_order(1).help("Don't fetch")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("plan")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Find versions that need to change")
        .arg(
          Arg::with_name("nofetch").short("F").long("no-fetch").takes_value(false).display_order(1).help("Don't fetch")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("log")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Write plans to change logs")
        .arg(
          Arg::with_name("nofetch").short("F").long("no-fetch").takes_value(false).display_order(1).help("Don't fetch")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("run")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Change and commit version numbers")
        .arg(
          Arg::with_name("nofetch").short("F").long("no-fetch").takes_value(false).display_order(1).help("Don't fetch")
        )
        .arg(
          Arg::with_name("all")
            .short("a")
            .long("show-all")
            .takes_value(false)
            .display_order(1)
            .help("Also show unchnaged versions")
        )
        .arg(
          Arg::with_name("dry")
            .short("d")
            .long("dry-run")
            .takes_value(false)
            .display_order(1)
            .help("Don't write new versions")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("changes")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Print true changes")
        .arg(
          Arg::with_name("nofetch").short("F").long("no-fetch").takes_value(false).display_order(1).help("Don't fetch")
        )
        .display_order(1)
    )
    .get_matches();

  parse_matches(m)
}

fn parse_matches(m: ArgMatches) -> Result<()> {
  let pref_vcs = parse_vcs(&m)?;

  match m.subcommand() {
    ("check", _) => check(pref_vcs)?,
    ("get", Some(m)) => get(
      pref_vcs,
      m.is_present("wide"),
      m.is_present("versiononly"),
      m.is_present("prev"),
      m.value_of("id"),
      m.value_of("name")
    )?,
    ("show", Some(m)) => show(pref_vcs, m.is_present("wide"))?,
    ("set", Some(m)) => set(pref_vcs, m.value_of("id"), m.value_of("name"), m.value_of("value").unwrap())?,
    ("diff", Some(_)) => diff(pref_vcs)?,
    ("files", Some(_)) => files(pref_vcs)?,
    ("log", Some(_)) => log(pref_vcs)?,
    ("changes", Some(_)) => changes(pref_vcs)?,
    ("plan", Some(_)) => plan(pref_vcs)?,
    ("run", Some(m)) => run(pref_vcs, m.is_present("all"), m.is_present("dry"))?,
    ("", _) => empty_cmd()?,
    (c, _) => unknown_cmd(c)?
  }

  Ok(())
}

fn unknown_cmd(c: &str) -> Result<()> { err!("Unknown command: \"{}\" (try \"help\").", c) }
fn empty_cmd() -> Result<()> { err!("No command (try \"help\").") }

fn parse_vcs(m: &ArgMatches) -> Result<Option<VcsRange>> {
  if let Some(vcs_level) = m.value_of("vcslevel") {
    match vcs_level {
      "auto" => Ok(None),
      "max" => Ok(Some(VcsRange::full())),
      other => {
        let other: VcsLevel = other.parse()?;
        Ok(Some(VcsRange::exact(other)))
      }
    }
  } else if let Some(vcs_min) = m.value_of("vcslevelmin") {
    let vcs_max = m.value_of("vcslevelmax").unwrap();
    Ok(Some(VcsRange::new(vcs_min.parse()?, vcs_max.parse()?)))
  } else {
    Ok(None)
  }
}
