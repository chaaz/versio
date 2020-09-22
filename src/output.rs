//! The way we output things to the user.

use crate::analyze::Analysis;
use crate::config::{Project, ProjectId, Size};
use crate::errors::{Result, ResultExt};
use crate::github::Changes;
use crate::mono::{Mono, Plan};
use crate::state::StateRead;
use serde_json::json;
use std::path::PathBuf;

pub struct Output {}

impl Default for Output {
  fn default() -> Output { Output::new() }
}

impl Output {
  pub fn new() -> Output { Output {} }
  pub fn check(&self) -> CheckOutput { CheckOutput::new() }
  pub fn projects(&self, wide: bool, vers_only: bool) -> ProjOutput { ProjOutput::new(wide, vers_only) }
  pub fn info(&self, show_name: bool, show_root: bool) -> ProjOutput { ProjOutput::info(show_name, show_root) }
  pub fn diff(&self) -> DiffOutput { DiffOutput::new() }
  pub fn files(&self) -> FilesOutput { FilesOutput::new() }
  pub fn changes(&self) -> ChangesOutput { ChangesOutput::new() }
  pub fn plan(&self) -> PlanOutput { PlanOutput::new() }
  pub fn release(&self) -> ReleaseOutput { ReleaseOutput::new() }
  pub fn resume(&self) -> ResumeOutput { ResumeOutput::new() }
}

pub struct CheckOutput {}

impl Default for CheckOutput {
  fn default() -> CheckOutput { CheckOutput::new() }
}

impl CheckOutput {
  pub fn new() -> CheckOutput { CheckOutput {} }
  pub fn write_done(&mut self) -> Result<()> { Ok(()) }

  pub fn commit(&mut self) -> Result<()> {
    println!("Check complete.");
    Ok(())
  }
}

pub struct ResumeOutput {}

impl Default for ResumeOutput {
  fn default() -> ResumeOutput { ResumeOutput::new() }
}

impl ResumeOutput {
  pub fn new() -> ResumeOutput { ResumeOutput {} }
  pub fn write_done(&mut self) -> Result<()> { Ok(()) }

  pub fn commit(&mut self) -> Result<()> {
    println!("Release complete.");
    Ok(())
  }
}

pub struct ProjOutput {
  wide: bool,
  vers_only: bool,
  proj_lines: Vec<ProjLine>,
  info_only: bool,
  show_name: bool,
  show_root: bool
}

impl ProjOutput {
  pub fn new(wide: bool, vers_only: bool) -> ProjOutput {
    ProjOutput { show_name: false, show_root: false, info_only: false, wide, vers_only, proj_lines: Vec::new() }
  }

  pub fn info(show_name: bool, show_root: bool) -> ProjOutput {
    ProjOutput { info_only: true, show_name, show_root, wide: false, vers_only: false, proj_lines: Vec::new() }
  }

  pub fn write_projects<I: Iterator<Item = Result<ProjLine>>>(&mut self, lines: I) -> Result<()> {
    self.proj_lines = lines.collect::<Result<_>>()?;
    Ok(())
  }

  pub fn write_project(&mut self, line: ProjLine) -> Result<()> {
    self.proj_lines = vec![line];
    Ok(())
  }

  pub fn commit(&mut self) -> Result<()> {
    let name_width = self.proj_lines.iter().map(|l| l.name.len()).max().unwrap_or(0);
    if self.info_only {
      let val = json!(self
        .proj_lines
        .iter()
        .map(|line| {
          let root = line.root.as_deref().unwrap_or(".");
          if self.show_name {
            if self.show_root {
              json!({"name": line.name, "root": root})
            } else {
              json!({"name": line.name})
            }
          } else if self.show_root {
            json!({ "root": root })
          } else {
            json!({})
          }
        })
        .collect::<Vec<_>>());
      println!("{}", serde_json::to_string(&val)?);
    } else {
      for line in &self.proj_lines {
        if self.vers_only {
          println!("{}", line.version);
        } else if self.wide {
          println!("{:>6}. {:width$} : {}", line.id, line.name, line.version, width = name_width);
        } else {
          println!("{:width$} : {}", line.name, line.version, width = name_width);
        }
      }
    }
    Ok(())
  }
}

pub struct ProjLine {
  pub id: ProjectId,
  pub name: String,
  pub version: String,
  pub root: Option<String>
}

impl ProjLine {
  pub fn from<S: StateRead>(p: &Project, read: &S) -> Result<ProjLine> {
    let id = p.id();
    let name = p.name().to_string();
    let version = p.get_value(read)?;
    let root = p.root().cloned();
    Ok(ProjLine { id: id.clone(), name, version, root })
  }
}

pub struct DiffOutput {
  analysis: Option<Analysis>
}

impl Default for DiffOutput {
  fn default() -> DiffOutput { DiffOutput::new() }
}

impl DiffOutput {
  pub fn new() -> DiffOutput { DiffOutput { analysis: None } }

  pub fn write_analysis(&mut self, analysis: Analysis) -> Result<()> {
    self.analysis = Some(analysis);
    Ok(())
  }

  pub fn commit(&mut self) -> Result<()> {
    if let Some(analysis) = &self.analysis {
      println_analysis(analysis)?;
    }
    Ok(())
  }
}

fn println_analysis(analysis: &Analysis) -> Result<()> {
  if !analysis.older().is_empty() {
    println!("Removed projects:");
    for mark in analysis.older() {
      println!("  {} : {}", mark.name(), mark.mark());
    }
  }

  if !analysis.newer().is_empty() {
    println!("New projects:");
    for mark in analysis.newer() {
      println!("  {} : {}", mark.name(), mark.mark());
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
        print!(" : {}", change.new_mark().mark());
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
      print!(" : {}", change.new_mark().mark());
      println!();
    }
  }

  Ok(())
}

pub struct FilesOutput {
  files: Vec<(String, String)>
}

impl Default for FilesOutput {
  fn default() -> FilesOutput { FilesOutput::new() }
}

impl FilesOutput {
  pub fn new() -> FilesOutput { FilesOutput { files: Vec::new() } }

  pub fn write_files(&mut self, files: impl Iterator<Item = Result<(String, String)>>) -> Result<()> {
    self.files = files.collect::<std::result::Result<_, _>>()?;
    Ok(())
  }

  pub fn commit(&mut self) -> Result<()> {
    for (key, path) in &self.files {
      println!("{} : {}", key, path);
    }
    Ok(())
  }
}

pub struct ChangesOutput {
  changes: Option<Changes>
}

impl Default for ChangesOutput {
  fn default() -> ChangesOutput { ChangesOutput::new() }
}

impl ChangesOutput {
  pub fn new() -> ChangesOutput { ChangesOutput { changes: None } }

  pub fn write_changes(&mut self, changes: Changes) -> Result<()> {
    self.changes = Some(changes);
    Ok(())
  }

  pub fn commit(&mut self) -> Result<()> {
    if let Some(changes) = &self.changes {
      println_changes(changes)
    } else {
      println!("No changes.");
      Ok(())
    }
  }
}

fn println_changes(changes: &Changes) -> Result<()> {
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

pub struct PlanOutput {
  plan: Option<Plan>
}

impl Default for PlanOutput {
  fn default() -> PlanOutput { PlanOutput::new() }
}

impl PlanOutput {
  pub fn new() -> PlanOutput { PlanOutput { plan: None } }

  pub fn write_plan(&mut self, plan: Plan) -> Result<()> {
    self.plan = Some(plan);
    Ok(())
  }

  pub fn commit(&mut self, mono: &Mono) -> Result<()> {
    if let Some(plan) = &self.plan {
      println_plan(plan, mono)
    } else {
      println!("No plan.");
      Ok(())
    }
  }
}

fn println_plan(plan: &Plan, mono: &Mono) -> Result<()> {
  println_plan_incrs(plan, mono)?;
  println_plan_ineff(plan)?;
  Ok(())
}

fn println_plan_incrs(plan: &Plan, mono: &Mono) -> Result<()> {
  if plan.incrs().is_empty() {
    println!("(No projects)");
    return Ok(());
  }

  for (id, (size, changelog)) in plan.incrs() {
    let curt_proj = mono.get_project(id).unwrap();
    println!("{} : {}", curt_proj.name(), size);
    for dep in curt_proj.depends() {
      let dep_size = plan.incrs().get(dep).unwrap().0;
      let dep_proj = mono.get_project(dep).unwrap();
      println!("  Depends on {} : {}", dep_proj.name(), dep_size);
    }

    let curt_config = mono.config();
    let prev_config = curt_config.slice_to_prev(mono.repo())?;
    let prev_vers = prev_config.get_value(id).chain_err(|| format!("Unable to find prev {} value.", id))?;
    let curt_vers = curt_config
      .get_value(id)
      .chain_err(|| format!("Unable to find project {} value.", id))?
      .unwrap_or_else(|| panic!("No such project {}.", id));

    if let Some(prev_vers) = prev_vers {
      if size != &Size::Empty {
        let target = size.apply(&prev_vers)?;
        if Size::less_than(&curt_vers, &target)? {
          if curt_proj.verify_restrictions(&target).is_err() {
            println!("  ! Illegal size change for restricted project {}.", curt_proj.id());
          }
        } else if curt_proj.verify_restrictions(&curt_vers).is_err() {
          println!("  ! Illegal size change for restricted project {}.", curt_proj.id());
        }
      }
    }

    for (pr, size) in changelog.entries() {
      if !pr.commits().iter().any(|c| c.included()) {
        continue;
      }
      if pr.number() == 0 {
        // "PR zero" is the top-level set of commits.
        println!("  Other commits : {}", size);
      } else {
        println!("  PR {} : {}", pr.number(), size);
      }
      for c in pr.commits().iter().filter(|c| c.included()) {
        let symbol = if c.duplicate() {
          "."
        } else if c.applies() {
          "*"
        } else {
          " "
        };
        println!("    {} commit {} ({}) : {}", symbol, &c.oid()[.. 7], c.size(), c.message().trim());
      }
    }
  }

  Ok(())
}

fn println_plan_ineff(plan: &Plan) -> Result<()> {
  for pr in plan.ineffective() {
    if !pr.commits().iter().any(|c| c.included()) {
      continue;
    }
    if pr.number() == 0 {
      println!("  Unapplied commits");
    } else {
      println!("  Unapplied PR {}", pr.number());
    }
    for c in pr.commits().iter().filter(|c| c.included()) {
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

  Ok(())
}

pub struct ReleaseOutput {
  result: ReleaseResult
}

impl Default for ReleaseOutput {
  fn default() -> ReleaseOutput { ReleaseOutput::new() }
}

impl ReleaseOutput {
  pub fn new() -> ReleaseOutput { ReleaseOutput { result: ReleaseResult::Empty } }

  pub fn write_empty(&mut self) -> Result<()> {
    self.result = ReleaseResult::Empty;
    Ok(())
  }

  pub fn write_logged(&mut self, path: PathBuf) -> Result<()> { self.result.append_logged(path) }
  pub fn write_done(&mut self) -> Result<()> { self.result.append_done() }
  pub fn write_commit(&mut self) -> Result<()> { self.result.append_commit() }
  pub fn write_pause(&mut self) -> Result<()> { self.result.append_pause() }
  pub fn write_dry(&mut self) -> Result<()> { self.result.append_dry() }

  pub fn write_changed(&mut self, name: String, prev: String, curt: String, targ: String) -> Result<()> {
    self.result.append_changed(name, prev, curt, targ)
  }

  pub fn write_forward(&mut self, all: bool, name: String, prev: String, curt: String, targ: String) -> Result<()> {
    self.result.append_forward(all, name, prev, curt, targ)
  }

  pub fn write_no_change(&mut self, all: bool, name: String, prev: Option<String>, curt: String) -> Result<()> {
    self.result.append_no_change(all, name, prev, curt)
  }

  pub fn write_new(&mut self, all: bool, name: String, curt: String) -> Result<()> {
    self.result.append_new(all, name, curt)
  }

  pub fn commit(&mut self) -> Result<()> { self.result.commit() }
}

enum ReleaseResult {
  Empty,
  Wrote(WroteReleases)
}

impl ReleaseResult {
  fn append_logged(&mut self, path: PathBuf) -> Result<()> { self.append(ReleaseEvent::Logged(path)) }
  fn append_done(&mut self) -> Result<()> { self.append(ReleaseEvent::Done) }
  fn append_commit(&mut self) -> Result<()> { self.append(ReleaseEvent::Commit) }
  fn append_pause(&mut self) -> Result<()> { self.append(ReleaseEvent::Pause) }
  fn append_dry(&mut self) -> Result<()> { self.append(ReleaseEvent::Dry) }

  fn append_changed(&mut self, name: String, prev: String, curt: String, targ: String) -> Result<()> {
    self.append(ReleaseEvent::Changed(name, prev, curt, targ))
  }

  fn append_forward(&mut self, all: bool, name: String, prev: String, curt: String, targ: String) -> Result<()> {
    self.append(ReleaseEvent::Forward(all, name, prev, curt, targ))
  }

  fn append_no_change(&mut self, all: bool, name: String, prev: Option<String>, curt: String) -> Result<()> {
    self.append(ReleaseEvent::NoChange(all, name, prev, curt))
  }

  fn append_new(&mut self, all: bool, name: String, curt: String) -> Result<()> {
    self.append(ReleaseEvent::New(all, name, curt))
  }

  fn append(&mut self, ev: ReleaseEvent) -> Result<()> {
    match self {
      ReleaseResult::Empty => {
        let mut releases = WroteReleases::new();
        releases.push(ev);
        *self = ReleaseResult::Wrote(releases);
      }
      ReleaseResult::Wrote(releases) => {
        releases.push(ev);
      }
    }

    Ok(())
  }

  fn commit(&mut self) -> Result<()> {
    match self {
      ReleaseResult::Empty => {
        println!("No release: no projects.");
        Ok(())
      }
      ReleaseResult::Wrote(w) => w.commit()
    }
  }
}

struct WroteReleases {
  events: Vec<ReleaseEvent>
}

impl WroteReleases {
  pub fn new() -> WroteReleases { WroteReleases { events: Vec::new() } }
  pub fn push(&mut self, path: ReleaseEvent) { self.events.push(path); }

  pub fn commit(&mut self) -> Result<()> {
    for ev in &mut self.events {
      ev.commit()?;
    }
    Ok(())
  }
}

enum ReleaseEvent {
  Logged(PathBuf),
  Changed(String, String, String, String),
  Forward(bool, String, String, String, String),
  NoChange(bool, String, Option<String>, String),
  New(bool, String, String),
  Commit,
  Pause,
  Dry,
  Done
}

impl ReleaseEvent {
  fn commit(&mut self) -> Result<()> {
    match self {
      ReleaseEvent::Logged(p) => println!("Wrote changelog at {}.", p.to_string_lossy()),
      ReleaseEvent::Done => println!("Release complete."),
      ReleaseEvent::Commit => println!("Changes committed."),
      ReleaseEvent::Pause => println!("Paused for commit: use --resume to continue."),
      ReleaseEvent::Dry => println!("Dry run: no actual changes."),
      ReleaseEvent::Changed(name, prev, curt, targ) => {
        if prev == curt {
          println!("  {} : {} -> {}", name, prev, targ);
        } else {
          println!("  {} : {} -> {} instead of {}", name, prev, targ, curt);
        }
      }
      ReleaseEvent::NoChange(all, name, prev, curt) => {
        if *all {
          if let Some(prev) = prev {
            if prev == curt {
              println!("  {} : untouched at {}", name, curt);
            } else {
              println!("  {} : untouched: {} -> {}", name, prev, curt);
            }
          } else {
            println!("  {} : untouched non-existent at {}", name, curt);
          }
        }
      }
      ReleaseEvent::Forward(all, name, prev, curt, targ) => {
        if *all {
          if prev == curt {
            println!("  {} : no change to {}", name, curt);
          } else if curt == targ {
            println!("  {} : no change: already {} -> {}", name, prev, curt);
          } else {
            println!("  {} : no change: {} -> {} exceeds {}", name, prev, curt, targ);
          }
        }
      }
      ReleaseEvent::New(all, name, curt) => {
        if *all {
          println!("  {} : no change: {} is new", name, curt);
        }
      }
    }
    Ok(())
  }
}
