//! The command-line options for the executable.

use clap::{crate_version, App, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};
use versio::commands::*;
use versio::err;
use versio::errors::Result;
use versio::init::init;
use versio::vcs::{VcsLevel, VcsRange};

pub async fn execute(info: &EarlyInfo) -> Result<()> {
  let id_required = info.project_count() != 1;

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
    .arg(
      Arg::with_name("ignorecurrent")
        .short("c")
        .long("no-current")
        .takes_value(false)
        .display_order(1)
        .help("Accept local repo changes")
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
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("get")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Get one or more versions")
        .arg(
          Arg::with_name("prev")
            .short("p")
            .long("prev")
            .takes_value(false)
            .display_order(1)
            .requires("ident")
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
        .group(ArgGroup::with_name("ident").args(&["id", "name"]).required(id_required))
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
        .group(ArgGroup::with_name("ident").args(&["id", "name"]).required(id_required))
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
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("files")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Stream changed files")
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("plan")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Find versions that need to change")
        .arg({
          let arg = Arg::with_name("template")
            .short("t")
            .long("template")
            .takes_value(true)
            .value_name("url")
            .display_order(1)
            .help("The changelog template to format with.");
          if id_required {
            arg.requires("id")
          } else {
            arg
          }
        })
        .arg(
          Arg::with_name("id")
            .short("i")
            .long("id")
            .takes_value(true)
            .value_name("id")
            .display_order(1)
            .help("Plan only a single project.")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("release")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Change and commit version numbers")
        .arg(
          Arg::with_name("all")
            .short("a")
            .long("show-all")
            .takes_value(false)
            .display_order(1)
            .help("Also show unchanged versions")
        )
        .arg(
          Arg::with_name("pause")
            .short("p")
            .long("pause")
            .takes_value(true)
            .value_name("stage")
            .possible_values(&["commit"])
            .display_order(1)
            .help("Pause the release")
        )
        .arg(Arg::with_name("resume").long("resume").takes_value(false).display_order(1).help("Resume after pausing"))
        .arg(
          Arg::with_name("abort")
            .long("abort")
            .takes_value(false)
            .conflicts_with("resume")
            .display_order(1)
            .help("Abort after pausing")
        )
        .arg(
          Arg::with_name("dry")
            .short("d")
            .long("dry-run")
            .takes_value(false)
            .conflicts_with_all(&["pause", "resume", "abort"])
            .display_order(1)
            .help("Don't write new versions")
        )
        .arg(
          Arg::with_name("changelogonly")
            .short("c")
            .long("changelog-only")
            .takes_value(false)
            .conflicts_with_all(&["pause", "resume", "abort", "dry"])
            .display_order(1)
            .help("Don't do anything except write changelogs")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("changes")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Print true changes")
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("init")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Search for projects and write a config")
        .arg(
          Arg::with_name("maxdepth")
            .short("d")
            .long("max-depth")
            .takes_value(true)
            .value_name("depth")
            .display_order(1)
            .help("Max descent to search")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("info")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Print info about projects")
        .arg(
          Arg::with_name("id")
            .short("i")
            .long("id")
            .takes_value(true)
            .value_name("id")
            .multiple(true)
            .number_of_values(1)
            .min_values(1)
            .display_order(1)
            .help("Info on project ID")
        )
        .arg(
          Arg::with_name("name")
            .short("n")
            .long("name")
            .takes_value(true)
            .value_name("name")
            .multiple(true)
            .number_of_values(1)
            .min_values(1)
            .display_order(1)
            .help("Info on project name")
        )
        .arg(
          Arg::with_name("label")
            .short("l")
            .long("label")
            .takes_value(true)
            .value_name("label")
            .multiple(true)
            .number_of_values(1)
            .min_values(1)
            .display_order(1)
            .help("Info on projects with a label")
        )
        .arg(
          Arg::with_name("all").short("a").long("all").takes_value(false).display_order(1).help("Info on all projects")
        )
        .arg(
          Arg::with_name("showall")
            .short("A")
            .long("show-all")
            .takes_value(false)
            .display_order(1)
            .help("Show all fields")
        )
        .arg(
          Arg::with_name("showroot")
            .short("R")
            .long("show-root")
            .takes_value(false)
            .display_order(1)
            .help("Show the project(s) root")
        )
        .arg(
          Arg::with_name("showname")
            .short("N")
            .long("show-name")
            .takes_value(false)
            .display_order(1)
            .help("Show the project(s) name")
        )
        .arg(
          Arg::with_name("showid")
            .short("I")
            .long("show-id")
            .takes_value(false)
            .display_order(1)
            .help("Show the project(s) ID")
        )
        .arg(
          Arg::with_name("showfull")
            .short("F")
            .long("show-full-version")
            .takes_value(false)
            .display_order(1)
            .help("Show the project(s) full version with the tag prefix")
        )
        .arg(
          Arg::with_name("showversion")
            .short("V")
            .long("show-version")
            .takes_value(false)
            .display_order(1)
            .help("Show the project(s) version")
        )
        .arg(
          Arg::with_name("showtagprefix")
            .short("T")
            .long("show-tag-prefix")
            .takes_value(false)
            .display_order(1)
            .help("Show the project(s) tag prefix")
        )
        .display_order(1)
    )
    .subcommand(
      SubCommand::with_name("template")
        .setting(AppSettings::UnifiedHelpMessage)
        .about("Output a changelog template")
        .arg(
          Arg::with_name("template")
            .short("t")
            .long("template")
            .takes_value(true)
            .value_name("url")
            .display_order(1)
            .required(true)
            .help("The changelog template to format with.")
        )
        .display_order(1)
    )
    .get_matches();

  parse_matches(m, info).await
}

async fn parse_matches(m: ArgMatches<'_>, early_info: &EarlyInfo) -> Result<()> {
  match m.subcommand() {
    ("release", Some(m)) if m.is_present("abort") => (),
    ("release", Some(m)) if m.is_present("resume") => (),
    _ => sanity_check()?
  }

  let pref_vcs = parse_vcs(&m)?;
  let ignore_current = m.is_present("ignorecurrent");

  match m.subcommand() {
    ("check", _) => check(pref_vcs, ignore_current)?,
    ("get", Some(m)) => get(
      pref_vcs,
      m.is_present("wide"),
      m.is_present("versiononly"),
      m.is_present("prev"),
      m.value_of("id"),
      m.value_of("name"),
      ignore_current
    )?,
    ("show", Some(m)) => show(pref_vcs, m.is_present("wide"), m.is_present("prev"), ignore_current)?,
    ("set", Some(m)) => set(pref_vcs, m.value_of("id"), m.value_of("name"), m.value_of("value").unwrap())?,
    ("diff", Some(_)) => diff(pref_vcs, ignore_current)?,
    ("files", Some(_)) => files(pref_vcs, ignore_current)?,
    ("changes", Some(_)) => changes(pref_vcs, ignore_current)?,
    ("plan", Some(m)) => plan(early_info, pref_vcs, m.value_of("id"), m.value_of("template"), ignore_current).await?,
    ("release", Some(m)) if m.is_present("abort") => abort()?,
    ("release", Some(m)) if m.is_present("resume") => resume(pref_vcs)?,
    ("release", Some(m)) => {
      let dry = if m.is_present("dry") {
        Engagement::Dry
      } else if m.is_present("changelogonly") {
        Engagement::Changelog
      } else {
        Engagement::Full
      };

      release(pref_vcs, m.is_present("all"), &dry, m.is_present("pause")).await?
    }
    ("init", Some(m)) => init(m.value_of("maxdepth").map(|d| d.parse().unwrap()).unwrap_or(5))?,
    ("info", Some(m)) => {
      let names = m.values_of("name").map(|v| v.collect::<Vec<_>>()).unwrap_or_default();
      let labels = m.values_of("label").map(|v| v.collect::<Vec<_>>()).unwrap_or_default();
      let ids = m
        .values_of("id")
        .map(|v| v.map(|i| i.parse()).collect::<std::result::Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();

      let show = InfoShow::new()
        .pick_all(m.is_present("all"))
        .show_name(m.is_present("showname") || m.is_present("showall"))
        .show_root(m.is_present("showroot") || m.is_present("showall"))
        .show_id(m.is_present("showid") || m.is_present("showall"))
        .show_full_version(m.is_present("showfull") || m.is_present("showall"))
        .show_version(m.is_present("showversion") || m.is_present("showall"))
        .show_tag_prefix(m.is_present("showtagprefix") || m.is_present("showall"));

      info(pref_vcs, ids, names, labels, show, ignore_current)?
    }
    ("template", Some(m)) => template(early_info, m.value_of("template").unwrap()).await?,
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
