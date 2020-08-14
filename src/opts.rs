//! The command-line options for the executable.

use crate::errors::{Result, ResultExt};
use crate::config::Size;
use crate::mono::Mono;
use crate::output::{Output, ProjLine};
use crate::vcs::{VcsLevel, VcsRange};
use clap::{crate_version, App, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};
use error_chain::bail;

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

fn check(pref_vcs: Option<VcsRange>) -> Result<()> {
  let mono = build(pref_vcs, VcsLevel::None, VcsLevel::Local, VcsLevel::None, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.check();

  mono.check()?;
  output.write_done()?;

  output.commit()
}

fn get(
  pref_vcs: Option<VcsRange>, wide: bool, versonly: bool, _prev: bool, id: Option<&str>, name: Option<&str>
) -> Result<()> {
  let mono = build(pref_vcs, VcsLevel::None, VcsLevel::Local, VcsLevel::None, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.projects(wide, versonly);

  // TODO: prev

  let reader = mono.reader();
  if let Some(id) = id {
    let id = id.parse()?;
    output.write_project(ProjLine::from(mono.get_project(id)?, reader)?)?;
  } else {
    output.write_project(ProjLine::from(mono.get_named_project(name.unwrap())?, reader)?)?;
  }

  output.commit()
}

fn show(pref_vcs: Option<VcsRange>, wide: bool) -> Result<()> {
  let mono = build(pref_vcs, VcsLevel::None, VcsLevel::Local, VcsLevel::None, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.projects(wide, false);

  let reader = mono.reader();
  output.write_projects(mono.projects().iter().map(|p| ProjLine::from(p, reader)))?;
  output.commit()
}

fn set(pref_vcs: Option<VcsRange>, id: Option<&str>, name: Option<&str>, value: &str) -> Result<()> {
  let mut mono = build(pref_vcs, VcsLevel::None, VcsLevel::None, VcsLevel::None, VcsLevel::Smart)?;

  if let Some(id) = id {
    mono.set_by_id(id.parse()?, value)?;
  } else {
    mono.set_by_name(name.unwrap(), value)?;
  }

  mono.commit()
}

fn diff(pref_vcs: Option<VcsRange>) -> Result<()> {
  let mono = build(pref_vcs, VcsLevel::None, VcsLevel::Local, VcsLevel::Local, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.diff();

  let analysis = mono.diff()?;

  output.write_analysis(analysis)?;
  output.commit()
}

fn files(pref_vcs: Option<VcsRange>) -> Result<()> {
  let mono = build(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.files();

  output.write_files(mono.keyed_files()?)?;
  output.commit()
}

fn log(pref_vcs: Option<VcsRange>) -> Result<()> {
  let mut mono = build(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.log();

  let plan = mono.build_plan()?;

  if plan.incrs().is_empty() {
    output.write_empty()?;
    return output.commit();
  }

  for (&id, (.., change_log)) in plan.incrs() {
    if let Some(wrote) = mono.write_change_log(id, change_log)? {
      output.write_logged(wrote)?;
    }
  }

  mono.commit()?;
  output.commit()
}

fn changes(pref_vcs: Option<VcsRange>) -> Result<()> {
  let mono = build(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.changes();

  output.write_changes(mono.changes()?)?;
  output.commit()
}

fn plan(pref_vcs: Option<VcsRange>) -> Result<()> {
  let mono = build(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.plan();

  output.write_plan(mono.build_plan()?)?;
  output.commit(&mono)
}

fn run(pref_vcs: Option<VcsRange>, all: bool, dry: bool) -> Result<()> {
  let mut mono = build(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.run();

  let plan = mono.build_plan()?;

  if plan.incrs().is_empty() {
    output.write_empty()?;
    return output.commit();
  }

  for (&id, (size, change_log)) in plan.incrs() {
    if let Some(wrote) = mono.write_change_log(id, change_log)? {
      output.write_logged(wrote)?;
    }

    let proj = mono.get_project(id)?;
    let name = proj.name().to_string();
    let curt_config = mono.config();
    let prev_config = curt_config.slice_to_prev(mono.repo())?;
    let curt_vers = curt_config
      .get_value(id)
      .chain_err(|| format!("Unable to find project {} value.", id))?
      .unwrap_or_else(|| panic!("No such project {}.", id));
    let prev_vers = prev_config.get_value(id).chain_err(|| format!("Unable to find prev {} value.", id))?;

    if let Some(prev_vers) = prev_vers {
      // if a project has a specific major, rebuke major changes to a previous version.
      if proj.tag_major().is_some() && size >= &Size::Major {
        bail!("Illegal size change for restricted project \"{}\".", name);
      }

      let target = size.apply(&prev_vers)?;
      if Size::less_than(&curt_vers, &target)? {
        mono.set_by_id(id, &target)?;
        output.write_changed(name.clone(), prev_vers.clone(), curt_vers.clone(), target.clone())?;
      } else {
        mono.forward_by_id(id, &target)?;
        output.write_forward(all, name.clone(), prev_vers.clone(), curt_vers.clone(), target.clone())?;
      }
    } else {
      mono.forward_by_id(id, &curt_vers)?;
      output.write_new(all, name.clone(), curt_vers.clone())?;
    }
  }

  if !dry {
    mono.commit()?;
    output.write_commit()?;
  } else {
    output.write_dry()?;
  }

  output.write_done()?;
  output.commit()?;
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

fn build(
  user_pref_vcs: Option<VcsRange>, my_pref_lo: VcsLevel, my_pref_hi: VcsLevel, my_reqd_lo: VcsLevel,
  my_reqd_hi: VcsLevel
) -> Result<Mono> {
  let vcs = combine_vcs(user_pref_vcs, my_pref_lo, my_pref_hi, my_reqd_lo, my_reqd_hi)?;
  Mono::here(vcs.max())
}

fn combine_vcs(
  user_pref_vcs: Option<VcsRange>, my_pref_lo: VcsLevel, my_pref_hi: VcsLevel, my_reqd_lo: VcsLevel,
  my_reqd_hi: VcsLevel
) -> Result<VcsRange> {
  let pref_vcs = user_pref_vcs.unwrap_or_else(move || VcsRange::new(my_pref_lo, my_pref_hi));
  let reqd_vcs = VcsRange::new(my_reqd_lo, my_reqd_hi);
  VcsRange::detect_and_combine(&pref_vcs, &reqd_vcs)
}
