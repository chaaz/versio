//! The way we output things to the user.

use crate::analyze::Analysis;
use crate::config::{Project, ProjectId};
use crate::errors::Result;
use crate::github::Changes;
use crate::mono::{Mono, Plan};
use crate::state::StateRead;
use std::path::PathBuf;

pub struct Output {}

impl Default for Output {
  fn default() -> Output { Output::new() }
}

impl Output {
  pub fn new() -> Output { Output {} }
  pub fn check(&self) -> CheckOutput { CheckOutput::new() }
  pub fn projects(&self, wide: bool, vers_only: bool) -> ProjOutput { ProjOutput::new(wide, vers_only) }
  pub fn diff(&self) -> DiffOutput { DiffOutput::new() }
  pub fn files(&self) -> FilesOutput { FilesOutput::new() }
  pub fn log(&self) -> LogOutput { LogOutput::new() }
  pub fn changes(&self) -> ChangesOutput { ChangesOutput::new() }
  pub fn plan(&self) -> PlanOutput { PlanOutput::new() }
  pub fn run(&self) -> RunOutput { RunOutput::new() }
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

pub struct ProjOutput {
  wide: bool,
  vers_only: bool,
  proj_lines: Vec<ProjLine>
}

impl ProjOutput {
  pub fn new(wide: bool, vers_only: bool) -> ProjOutput { ProjOutput { wide, vers_only, proj_lines: Vec::new() } }

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
    for line in &self.proj_lines {
      if self.vers_only {
        println!("{}", line.version);
      } else if self.wide {
        println!("{:>4}. {:width$} : {}", line.id, line.name, line.version, width = name_width);
      } else {
        println!("{:width$} : {}", line.name, line.version, width = name_width);
      }
    }
    Ok(())
  }
}

pub struct ProjLine {
  pub id: ProjectId,
  pub name: String,
  pub version: String
}

impl ProjLine {
  pub fn new(id: ProjectId, name: String, version: String) -> ProjLine { ProjLine { id, name, version } }

  pub fn from<S: StateRead>(p: &Project, read: &S) -> Result<ProjLine> {
    let id = p.id();
    let name = p.name().to_string();
    let version = p.get_value(read)?;
    Ok(ProjLine { id, name, version })
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

pub struct LogOutput {
  result: LogResult
}

impl Default for LogOutput {
  fn default() -> LogOutput { LogOutput::new() }
}

impl LogOutput {
  pub fn new() -> LogOutput { LogOutput { result: LogResult::Empty } }

  pub fn write_empty(&mut self) -> Result<()> {
    self.result = LogResult::Empty;
    Ok(())
  }

  pub fn write_logged(&mut self, path: PathBuf) -> Result<()> { self.result.append_logged(path) }

  pub fn commit(&mut self) -> Result<()> {
    match &mut self.result {
      LogResult::Empty => {
        println!("No projects.");
        Ok(())
      }
      LogResult::Wrote(wrote) => wrote.commit()
    }
  }
}

enum LogResult {
  Empty,
  Wrote(WroteLogs)
}

impl LogResult {
  fn append_logged(&mut self, path: PathBuf) -> Result<()> {
    match self {
      LogResult::Empty => {
        let mut logs = WroteLogs::new();
        logs.push(path);
        *self = LogResult::Wrote(logs);
      }
      LogResult::Wrote(logs) => logs.push(path)
    }

    Ok(())
  }
}

struct WroteLogs {
  logged: Vec<PathBuf>
}

impl WroteLogs {
  pub fn new() -> WroteLogs { WroteLogs { logged: Vec::new() } }
  pub fn push(&mut self, path: PathBuf) { self.logged.push(path); }

  pub fn commit(&mut self) -> Result<()> {
    for logged in &mut self.logged {
      println!("Wrote log {}", logged.to_string_lossy());
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
  if plan.incrs().is_empty() {
    println!("(No projects)");
  } else {
    for (id, (size, change_log)) in plan.incrs() {
      let curt_proj = mono.get_project(*id).unwrap();
      println!("{} : {}", curt_proj.name(), size);
      for dep in curt_proj.depends() {
        let size = plan.incrs().get(dep).unwrap().0;
        let dep_proj = mono.get_project(*dep).unwrap();
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
    }
  }

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

pub struct RunOutput {
  result: RunResult
}

impl Default for RunOutput {
  fn default() -> RunOutput { RunOutput::new() }
}

impl RunOutput {
  pub fn new() -> RunOutput { RunOutput { result: RunResult::Empty } }

  pub fn write_empty(&mut self) -> Result<()> {
    self.result = RunResult::Empty;
    Ok(())
  }

  pub fn write_logged(&mut self, path: PathBuf) -> Result<()> { self.result.append_logged(path) }
  pub fn write_done(&mut self) -> Result<()> { self.result.append_done() }
  pub fn write_commit(&mut self) -> Result<()> { self.result.append_commit() }
  pub fn write_dry(&mut self) -> Result<()> { self.result.append_dry() }

  pub fn write_changed(&mut self, name: String, prev: String, curt: String, targ: String) -> Result<()> {
    self.result.append_changed(name, prev, curt, targ)
  }

  pub fn write_forward(&mut self, all: bool, name: String, prev: String, curt: String, targ: String) -> Result<()> {
    self.result.append_forward(all, name, prev, curt, targ)
  }

  pub fn write_new(&mut self, all: bool, name: String, curt: String) -> Result<()> {
    self.result.append_new(all, name, curt)
  }

  pub fn commit(&mut self) -> Result<()> { self.result.commit() }
}

enum RunResult {
  Empty,
  Wrote(WroteRuns)
}

impl RunResult {
  fn append_logged(&mut self, path: PathBuf) -> Result<()> { self.append(RunEvent::Logged(path)) }
  fn append_done(&mut self) -> Result<()> { self.append(RunEvent::Done) }
  fn append_commit(&mut self) -> Result<()> { self.append(RunEvent::Commit) }
  fn append_dry(&mut self) -> Result<()> { self.append(RunEvent::Dry) }

  fn append_changed(&mut self, name: String, prev: String, curt: String, targ: String) -> Result<()> {
    self.append(RunEvent::Changed(name, prev, curt, targ))
  }

  fn append_forward(&mut self, all: bool, name: String, prev: String, curt: String, targ: String) -> Result<()> {
    self.append(RunEvent::Forward(all, name, prev, curt, targ))
  }

  fn append_new(&mut self, all: bool, name: String, curt: String) -> Result<()> {
    self.append(RunEvent::New(all, name, curt))
  }

  fn append(&mut self, ev: RunEvent) -> Result<()> {
    match self {
      RunResult::Empty => {
        let mut runs = WroteRuns::new();
        runs.push(ev);
        *self = RunResult::Wrote(runs);
      }
      RunResult::Wrote(runs) => {
        runs.push(ev);
      }
    }

    Ok(())
  }

  fn commit(&mut self) -> Result<()> {
    match self {
      RunResult::Empty => {
        println!("No run: no projects.");
        Ok(())
      }
      RunResult::Wrote(w) => w.commit()
    }
  }
}

struct WroteRuns {
  events: Vec<RunEvent>
}

impl WroteRuns {
  pub fn new() -> WroteRuns { WroteRuns { events: Vec::new() } }
  pub fn push(&mut self, path: RunEvent) { self.events.push(path); }

  pub fn commit(&mut self) -> Result<()> {
    for ev in &mut self.events {
      ev.commit()?;
    }
    Ok(())
  }
}

enum RunEvent {
  Logged(PathBuf),
  Changed(String, String, String, String),
  Forward(bool, String, String, String, String),
  New(bool, String, String),
  Commit,
  Dry,
  Done,
}

impl RunEvent {
  fn commit(&mut self) -> Result<()> {
    match self {
      RunEvent::Logged(p) => println!("Wrote changelog at {}.", p.to_string_lossy()),
      RunEvent::Done => println!("Run complete."),
      RunEvent::Commit => println!("Changes committed."),
      RunEvent::Dry => println!("Dry run: no actual changes."),
      RunEvent::Changed(name, prev, curt, targ) => {
        if prev == curt {
          println!("  {} : {} -> {}", name, prev, targ);
        } else {
          println!("  {} : {} -> {} instead of {}", name, prev, targ, curt);
        }
      }
      RunEvent::Forward(all, name, prev, curt, targ) => {
        if *all {
          if prev == curt {
            println!("  {} : no change to {}", name, curt);
          } else if curt == targ {
            println!("  {} : no change: already {} -> {}", name, prev, targ);
          } else {
            println!("  {} : no change: {} -> {} exceeds {}", name, prev, curt, targ);
          }
        }
      }
      RunEvent::New(all, name, curt) => {
        if *all {
          println!("  {} : no change: {} is new", name, curt);
        }
      }
    }
    Ok(())
  }
}
