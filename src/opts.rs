//! The command-line options for the executable.

use crate::error::Result;
use crate::mono::Mono;
use crate::output::{Output, ProjLine};
use crate::vcs::{VcsLevel, VcsRange};
use clap::{crate_version, App, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};

pub fn execute() -> Result<()> {
  let m = App::new("versio")
    .setting(AppSettings::UnifiedHelpMessage)
    .author("Charlie Ozinga, charlie@cloud-elements.com")
    .version(concat!(crate_version!(), " (", env!("GIT_SHORT_HASH"), " ", env!("DATE_DASH"), ")"))
    .about("Manage version numbers")
    .arg(
      Arg::with_name("vcslevel")
        .short("l")
        .long("vcs-level")
        .takes_value(true)
        .value_name("level")
        .possible_values(&["auto", "max", "none", "local", "remote", "smart"])
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
        .display_order(1)
        .help("The maximum VCS level")
    )
    .group(ArgGroup::with_name("level").args(&["vcslevel", "vcslevelmin"]).required(false))
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
            .help("Whether to show prev versions")
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
  println!("parsing matches");
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

pub fn log(pref_vcs: Option<VcsRange>) -> Result<()> {
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

pub fn plan(pref_vcs: Option<VcsRange>) -> Result<()> {
  let mono = build(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.plan();

  output.write_plan(mono.build_plan()?)?;
  output.commit(&mono)
}

pub fn run(pref_vcs: Option<VcsRange>, _all: bool, _dry: bool) -> Result<()> {
  let mut mono = build(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.run();

  let plan = mono.build_plan()?;

  if plan.incrs().is_empty() {
    output.write_empty()?;
    return output.commit();
  }

  for (&id, (_size, change_log)) in plan.incrs() {
    // let proj = mono.get_project(id)?;
    // let _name = proj.name();

    // let curt_vers = curt_cfg.get_mark_value(id).unwrap()?;
    // let prev_vers = prev_cfg.get_mark_value(id).transpose()?;

    if let Some(wrote) = mono.write_change_log(id, change_log)? {
      output.write_logged(wrote)?;
    }

    // if let Some(prev_vers) = prev_vers {
    //   let target = size.apply(&prev_vers)?;
    //   if Size::less_than(&curt_vers, &target)? {
    //     if !dry {
    //       curt_cfg.set_by_id(id, &target, last_commit.as_ref(), &mut new_tags, wrote_change_log)?;
    //     } else {
    //       new_tags.flag_commit();
    //     }
    //     if prev_vers == curt_vers {
    //       println!("  {} : {} -> {}", curt_name, prev_vers, &target);
    //     } else {
    //       println!("  {} : {} -> {} instead of {}", curt_name, prev_vers, &target, curt_vers);
    //     }
    //   } else {
    //     if !dry {
    //       curt_cfg.forward_by_id(id, &curt_vers, last_commit.as_ref(), &mut new_tags, wrote_change_log)?;
    //     } else if wrote_change_log {
    //       new_tags.flag_commit();
    //     }
    //     if all {
    //       if prev_vers == curt_vers {
    //         println!("  {} : no change to {}", curt_name, curt_vers);
    //       } else if curt_vers == target {
    //         println!("  {} : no change: already {} -> {}", curt_name, prev_vers, &target);
    //       } else {
    //         println!("  {} : no change: {} -> {} exceeds {}", curt_name, prev_vers, curt_vers, &target);
    //       }
    //     }
    //   }
    // } else {
    //   if !dry {
    //     curt_cfg.forward_by_id(id, &curt_vers, last_commit.as_ref(), &mut new_tags, wrote_change_log)?;
    //   } else if wrote_change_log {
    //     new_tags.flag_commit();
    //   }

    //   if all {
    //     println!("  {} : no change: {} is new", curt_name, curt_vers);
    //   }
    // }
  }

  if !_dry {
    // TODO: suppress command-level commit
  }

  // if new_tags.should_commit() {
  //   if dry {
  //     println!("Dry run: no actual commits.");
  //   } else if prev.repo()?.make_changes(new_tags.tags_for_new_commit())? {
  //     if prev.has_remote() {
  //       println!("Changes committed and pushed.");
  //     } else {
  //       println!("Changes committed.");
  //     }
  //   } else {
  //     return versio_err!("No file changes found somehow.");
  //   }
  // } else {
  //   // TODO: still tag / push ?
  //   println!("No planned increments: not committing.");
  // }

  // if prev.repo()?.forward_tags(new_tags.changed_tags())? {
  //   if dry {
  //     println!("Dry run: no actual tag forwarding.");
  //   } else if prev.has_remote() {
  //     println!("Tags forwarded and pushed.");
  //   } else {
  //     println!("Tags forwarded.");
  //   }
  // }

  // if dry {
  //   println!("Dry run: no actual prevtag update.");
  // } else {
  //   prev.repo()?.forward_prev_tag(curt_cfg.prev_tag())?;
  //   if prev.has_remote() {
  //     println!("Prevtag forwarded and pushed.");
  //   } else {
  //     println!("Prevtag forwarded.");
  //   }
  // }

  output.write_done()?;

  mono.commit()?;
  output.commit()
}

fn unknown_cmd(c: &str) -> Result<()> { versio_err!("Unknown command: \"{}\" (try \"help\").", c) }
fn empty_cmd() -> Result<()> { versio_err!("No command (try \"help\").") }

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
  VcsRange::negotiate_and_combine(&pref_vcs, &reqd_vcs)
}
