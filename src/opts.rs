//! The command-line options for the executable.

use crate::config::{configure_plan, Mono, NewTags, ShowFormat, Size};
use crate::error::Result;
use clap::{crate_version, App, AppSettings, Arg, ArgGroup, ArgMatches, SubCommand};

pub fn execute() -> Result<()> {
  let m = App::new("versio")
    .setting(AppSettings::UnifiedHelpMessage)
    .author("Charlie Ozinga, charlie@cloud-elements.com")
    .version(concat!(crate_version!(), " (", env!("GIT_SHORT_HASH"), " ", env!("DATE_DASH"), ")"))
    .about("Manage version numbers")
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
  let mono = Mono::here()?;

  match m.subcommand() {
    ("check", _) => check(&mono),
    ("show", Some(m)) => {
      if m.is_present("nofetch") {
        // TODO
      }
      let fmt = ShowFormat::new(m.is_present("wide"), false);
      if m.is_present("prev") {
        mono.previous_config().show(fmt)
      } else {
        mono.current_config().show(fmt)
      }
    }
    ("get", Some(m)) => {
      if m.is_present("nofetch") {
        // TODO
      }
      let fmt = ShowFormat::new(m.is_present("wide"), m.is_present("versiononly"));
      if m.is_present("prev") {
        if m.is_present("id") {
          mono.previous_config().show_id(m.value_of("id").unwrap().parse()?, fmt)
        } else {
          mono.previous_config().show_names(m.value_of("name").unwrap(), fmt)
        }
      } else if m.is_present("id") {
        mono.current_config().show_id(m.value_of("id").unwrap().parse()?, fmt)
      } else {
        mono.current_config().show_names(m.value_of("name").unwrap(), fmt)
      }
    }
    ("diff", Some(m)) => {
      if m.is_present("nofetch") {
        // TODO
      }
      diff(&mono)
    }
    ("set", Some(m)) => {
      if m.is_present("id") {
        mono.set_by_id(m.value_of("id").unwrap().parse()?, m.value_of("value").unwrap(), &mut NewTags::new())
      } else {
        mono.set_by_name(m.value_of("name").unwrap(), m.value_of("value").unwrap(), &mut NewTags::new())
      }
    }
    ("files", Some(m)) => {
      if m.is_present("nofetch") {
        // TODO
      }
      for result in mono.keyed_files()? {
        let (key, path) = result?;
        println!("{} : {}", key, path);
      }
      Ok(())
    }
    ("plan", Some(m)) => {
      if m.is_present("nofetch") {
        // TODO
      }
      plan(&mono)
    }
    ("run", Some(m)) => {
      if m.is_present("nofetch") {
        // TODO
      }
      run(&mono, m.is_present("all"), m.is_present("dry"))
    }
    ("log", Some(m)) => {
      if m.is_present("nofetch") {
        // TODO
      }
      log(&mono)
    }
    ("changes", Some(m)) => {
      if m.is_present("nofetch") {
        // TODO
      }
      changes(&mono)
    }
    ("", _) => empty_cmd(),
    (c, _) => unknown_cmd(c)
  }
}

fn diff(mono: &Mono) -> Result<()> {
  let analysis = mono.diff()?;

  if !analysis.older().is_empty() {
    println!("Removed projects:");
    for mark in analysis.older() {
      println!("  {} : {}", mark.name(), mark.mark().value());
    }
  }

  if !analysis.newer().is_empty() {
    println!("New projects:");
    for mark in analysis.newer() {
      println!("  {} : {}", mark.name(), mark.mark().value());
    }
  }

  if analysis.changes().iter().any(|c| c.value().is_some()) {
    println!("Changed versions:");
    for change in analysis.changes().iter().filter(|c| c.value().is_some()) {
      print!("  {}", change.new_mark().name());

      if let Some((o, _)) = change.name().as_ref() {
        print!(" (was \"{}\")", o);
      }
      if let Some((o, n)) = change.value().as_ref() {
        print!(" : {} -> {}", o, n);
      } else {
        print!(" : {}", change.new_mark().mark().value());
      }
      println!();
    }
  }

  if analysis.changes().iter().any(|c| c.value().is_none()) {
    println!("Unchanged versions:");
    for change in analysis.changes().iter().filter(|c| c.value().is_none()) {
      print!("  {}", change.new_mark().name());

      if let Some((o, _)) = change.name().as_ref() {
        print!(" (was \"{}\")", o);
      }
      print!(" : {}", change.new_mark().mark().value());
      println!();
    }
  }

  Ok(())
}

pub fn plan(mono: &Mono) -> Result<()> {
  let plan = configure_plan(mono)?;
  let curt_cfg = mono.current_config();

  if plan.incrs().is_empty() {
    println!("(No projects)");
  } else {
    for (id, (size, _, change_log)) in plan.incrs() {
      let curt_proj = curt_cfg.get_project(*id).unwrap();
      println!("{} : {}", curt_proj.name(), size);
      for dep in curt_proj.depends() {
        let size = plan.incrs().get(dep).unwrap().0;
        let dep_proj = curt_cfg.get_project(*dep).unwrap();
        println!("  Depends on {} : {}", dep_proj.name(), size);
      }
      for (pr, size) in change_log.entries() {
        if !pr.commits().iter().any(|c| c.included()) {
          continue;
        }
        if pr.number() == 0 {
          // "PR zero" is the top-level set of commits.
          println!("  Other commits : {}", size);
        } else {
          println!("  PR {} : {}", pr.number(), size);
        }
        for c /* (oid, msg, size, appl, dup) */ in pr.commits().iter().filter(|c| c.included()) {
          let symbol = if c.duplicate() {
            "."
          } else if c.applies() {
            "*"
          } else {
            " "
          };
          println!("    {} commit {} ({}) : {}", symbol, &c.oid()[.. 7], c.size(), c.message());
        }
      }
    }
  }

  Ok(())
}

pub fn log(mono: &Mono) -> Result<()> {
  let plan = configure_plan(mono)?;

  if plan.incrs().is_empty() {
    println!("(No projects)");
    return Ok(());
  }

  let curt_cfg = mono.current_config();
  let curt = mono.current_source();

  println!("Executing plan:");
  for (id, (.., change_log)) in plan.incrs() {
    let proj = curt_cfg.get_project(*id).unwrap();

    if let Some(wrote) = proj.write_change_log(&change_log, curt)? {
      println!("Wrote {}", wrote);
    }
  }
  Ok(())
}

pub fn run(mono: &Mono, all: bool, dry: bool) -> Result<()> {
  if !dry {
    // TODO: mono.set_merge(true)?;

    // We're going to commit and push changes soon; let's make sure that we are up-to-date. But don't create a
    // merge commit: fail immediately if we can't pull with a fast-forward.
    mono.pull()?;
  }

  let plan = configure_plan(mono)?;

  if plan.incrs().is_empty() {
    println!("(No projects)");
    return Ok(());
  }

  let curt_cfg = mono.current_config();
  let prev_cfg = mono.previous_config();
  let curt = mono.current_source();

  println!("Executing plan:");
  let mut new_tags = NewTags::new();
  for (&id, (size, last_commit, change_log)) in plan.incrs() {
    let proj = curt_cfg.get_project(id).unwrap();
    let curt_name = proj.name();
    let curt_mark = curt_cfg.get_mark(id).unwrap()?;
    let curt_vers = curt_mark.value();
    let prev_mark = prev_cfg.get_mark(id).transpose()?;
    let prev_vers = prev_mark.as_ref().map(|m| m.value());

    proj.write_change_log(&change_log, curt)?;

    if let Some(prev_vers) = prev_vers {
      let target = size.apply(prev_vers)?;
      if Size::less_than(curt_vers, &target)? {
        if !dry {
          curt_cfg.set_by_id(id, &target, last_commit.as_ref(), &mut new_tags)?;
        } else {
          new_tags.flag_commit();
        }
        if prev_vers == curt_vers {
          println!("  {} : {} -> {}", curt_name, prev_vers, &target);
        } else {
          println!("  {} : {} -> {} instead of {}", curt_name, prev_vers, &target, curt_vers);
        }
      } else {
        if !dry {
          curt_cfg.forward_by_id(id, curt_vers, last_commit.as_ref(), &mut new_tags)?;
        }
        if all {
          if prev_vers == curt_vers {
            println!("  {} : no change to {}", curt_name, curt_vers);
          } else if curt_vers == target {
            println!("  {} : no change: already {} -> {}", curt_name, prev_vers, &target);
          } else {
            println!("  {} : no change: {} -> {} exceeds {}", curt_name, prev_vers, curt_vers, &target);
          }
        }
      }
    } else if all {
      println!("  {} : no change: {} is new", curt_name, curt_vers);
    }
  }

  let prev = mono.previous_source();

  if new_tags.should_commit() {
    if dry {
      println!("Dry run: no actual commits.");
    } else if prev.repo()?.make_changes(new_tags.tags_for_new_commit())? {
      if prev.has_remote() {
        println!("Changes committed and pushed.");
      } else {
        println!("Changes committed.");
      }
    } else {
      return versio_err!("No file changes found somehow.");
    }
  } else {
    // TODO: still tag / push ?
    println!("No planned increments: not committing.");
  }

  if prev.repo()?.forward_tags(new_tags.changed_tags())? {
    if dry {
      println!("Dry run: no actual tag forwarding.");
    } else if prev.has_remote() {
      println!("Tags forwarded and pushed.");
    } else {
      println!("Tags forwarded.");
    }
  }

  if dry {
    println!("Dry run: no actual prevtag update.");
  } else {
    prev.repo()?.forward_prev_tag()?;
    if prev.has_remote() {
      println!("Prevtag forwarded and pushed.");
    } else {
      println!("Prevtag forwarded.");
    }
  }

  println!("Run complete.");

  Ok(())
}

fn changes(mono: &Mono) -> Result<()> {
  let prev = mono.previous_source();
  let changes = prev.changes()?;

  println!("\ngroups:");
  for g in changes.groups().values() {
    let head_oid = g.head_oid().as_ref().map(|o| o.to_string()).unwrap_or_else(|| "<not found>".to_string());
    println!("  {}: {} ({} -> {})", g.number(), g.head_ref(), g.base_oid(), head_oid);
    println!("    commits:");
    for cmt in g.commits() {
      println!("      {}", cmt.id());
    }
    println!("    excludes:");
    for cmt in g.excludes() {
      println!("      {}", cmt);
    }
  }

  println!("\ncommits:");
  for oid in changes.commits() {
    println!("  {}", oid);
  }

  Ok(())
}

fn check(mono: &Mono) -> Result<()> {
  if !mono.is_configured()? {
    return versio_err!("No versio config file found.");
  }
  mono.current_config().check()
}

fn unknown_cmd(c: &str) -> Result<()> { versio_err!("Unknown command: \"{}\" (try \"help\").", c) }
fn empty_cmd() -> Result<()> { versio_err!("No command (try \"help\").") }
