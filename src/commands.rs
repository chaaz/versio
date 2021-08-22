//! The command-line options for the executable.

use crate::config::{Config, ConfigFile, ProjectId, Size};
use crate::errors::{Result, ResultExt};
use crate::git::Repo;
use crate::mono::Mono;
use crate::output::{Output, ProjLine};
use crate::state::{CommitState, StateRead};
use crate::vcs::{VcsLevel, VcsRange, VcsState};
use error_chain::bail;
use std::collections::HashMap;
use std::fs::{remove_file, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

pub fn early_info() -> Result<EarlyInfo> {
  let vcs = VcsRange::detect()?.max();
  let root = Repo::find_working_dir(".", vcs, true)?;
  let file = ConfigFile::from_dir(&root)?;
  let project_count = file.projects().len();
  let orig_dir = std::env::current_dir()?;
  assert_ok!(orig_dir.is_absolute(), "Couldn't find current working directory.");

  Ok(EarlyInfo::new(project_count, root, orig_dir))
}

/// Environment information gathered even before we set the CLI options.
pub struct EarlyInfo {
  project_count: usize,
  working_dir: PathBuf,
  orig_dir: PathBuf
}

impl EarlyInfo {
  pub fn new(project_count: usize, working_dir: PathBuf, orig_dir: PathBuf) -> EarlyInfo {
    EarlyInfo { project_count, working_dir, orig_dir }
  }

  pub fn project_count(&self) -> usize { self.project_count }
  pub fn working_dir(&self) -> &Path { &self.working_dir }
  pub fn orig_dir(&self) -> &Path { &self.orig_dir }
}

pub fn check(pref_vcs: Option<VcsRange>, ignore_current: bool) -> Result<()> {
  let mono = with_opts(pref_vcs, VcsLevel::None, VcsLevel::Local, VcsLevel::None, VcsLevel::Smart, ignore_current)?;
  let output = Output::new();
  let mut output = output.check();

  mono.check()?;
  output.write_done()?;

  output.commit()
}

pub fn get(
  pref_vcs: Option<VcsRange>, wide: bool, versonly: bool, prev: bool, id: Option<&str>, name: Option<&str>,
  ignore_current: bool
) -> Result<()> {
  let mono = with_opts(pref_vcs, VcsLevel::None, VcsLevel::Local, VcsLevel::None, VcsLevel::Smart, ignore_current)?;

  if prev {
    get_using_cfg(&mono.config().slice_to_prev(mono.repo())?, wide, versonly, id, name)
  } else {
    get_using_cfg(mono.config(), wide, versonly, id, name)
  }
}

fn get_using_cfg<R: StateRead>(
  cfg: &Config<R>, wide: bool, versonly: bool, id: Option<&str>, name: Option<&str>
) -> Result<()> {
  let output = Output::new();
  let mut output = output.projects(wide, versonly);

  let ensure = || bad!("No such project.");

  let reader = cfg.state_read();
  if let Some(id) = id {
    let id = id.parse()?;
    output.write_project(ProjLine::from(cfg.get_project(&id).ok_or_else(&ensure)?, reader)?)?;
  } else if let Some(name) = name {
    let id = cfg.find_unique(name)?;
    output.write_project(ProjLine::from(cfg.get_project(id).ok_or_else(&ensure)?, reader)?)?;
  } else {
    if cfg.projects().len() != 1 {
      bail!("No solo project.");
    }
    let id = cfg.projects().get(0).unwrap().id();
    output.write_project(ProjLine::from(cfg.get_project(id).ok_or_else(&ensure)?, reader)?)?;
  }

  output.commit()
}

pub fn show(pref_vcs: Option<VcsRange>, wide: bool, prev: bool, ignore_current: bool) -> Result<()> {
  let mono = with_opts(pref_vcs, VcsLevel::None, VcsLevel::Local, VcsLevel::None, VcsLevel::Smart, ignore_current)?;

  if prev {
    show_using_cfg(&mono.config().slice_to_prev(mono.repo())?, wide)
  } else {
    show_using_cfg(mono.config(), wide)
  }
}

fn show_using_cfg<R: StateRead>(cfg: &Config<R>, wide: bool) -> Result<()> {
  let output = Output::new();
  let mut output = output.projects(wide, false);
  let reader = cfg.state_read();
  output.write_projects(cfg.projects().iter().map(|p| ProjLine::from(p, reader)))?;
  output.commit()
}

pub fn set(pref_vcs: Option<VcsRange>, id: Option<&str>, name: Option<&str>, value: &str) -> Result<()> {
  let mut mono = build(pref_vcs, VcsLevel::None, VcsLevel::None, VcsLevel::None, VcsLevel::Smart)?;

  if let Some(id) = id {
    mono.set_by_id(&id.parse()?, value)?;
  } else if let Some(name) = name {
    mono.set_by_name(name, value)?;
  } else {
    mono.set_by_only(value)?;
  }

  mono.commit(false, false)
}

pub fn diff(pref_vcs: Option<VcsRange>, ignore_current: bool) -> Result<()> {
  let mono = with_opts(pref_vcs, VcsLevel::None, VcsLevel::Local, VcsLevel::Local, VcsLevel::Smart, ignore_current)?;
  let output = Output::new();
  let mut output = output.diff();

  let analysis = mono.diff()?;

  output.write_analysis(analysis)?;
  output.commit()
}

pub fn files(pref_vcs: Option<VcsRange>, ignore_current: bool) -> Result<()> {
  let mono = with_opts(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart, ignore_current)?;
  let output = Output::new();
  let mut output = output.files();

  output.write_files(mono.keyed_files()?)?;
  output.commit()
}

pub fn changes(pref_vcs: Option<VcsRange>, ignore_current: bool) -> Result<()> {
  let mono = with_opts(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart, ignore_current)?;
  let output = Output::new();
  let mut output = output.changes();

  output.write_changes(mono.changes()?)?;
  output.commit();
  Ok(())
}

pub fn plan(_early_info: &EarlyInfo, pref_vcs: Option<VcsRange>, ignore_current: bool) -> Result<()> {
  let mono = with_opts(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart, ignore_current)?;
  let output = Output::new();
  let mut output = output.plan();

  output.write_plan(mono.build_plan()?)?;
  output.commit(&mono)
}

pub fn info(
  pref_vcs: Option<VcsRange>, ids: Vec<ProjectId>, names: Vec<&str>, labels: Vec<&str>, show: InfoShow,
  ignore_current: bool
) -> Result<()> {
  let mono = with_opts(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::None, VcsLevel::Smart, ignore_current)?;
  let output = Output::new();
  let all = show.all();
  let mut output = output.info(show);

  let cfg = mono.config();
  let reader = cfg.state_read();

  if all {
    output.write_projects(cfg.projects().iter().map(|p| ProjLine::from(p, reader)))?;
  } else {
    output.write_projects(
      cfg
        .projects()
        .iter()
        .filter(|p| {
          ids.contains(p.id())
            || names.contains(&p.name())
            || p.labels().iter().any(|l| labels.iter().any(|ll| ll == l))
        })
        .map(|p| ProjLine::from(p, reader))
    )?;
  }

  output.commit()?;
  Ok(())
}

pub struct InfoShow {
  pick_all: bool,
  show_id: bool,
  show_root: bool,
  show_name: bool,
  show_tag_prefix: bool,
  show_full_version: bool,
  show_version: bool
}

impl Default for InfoShow {
  fn default() -> InfoShow { InfoShow::new() }
}

impl InfoShow {
  pub fn new() -> InfoShow {
    InfoShow {
      pick_all: false,
      show_id: false,
      show_root: false,
      show_name: false,
      show_version: false,
      show_tag_prefix: false,
      show_full_version: false
    }
  }

  pub fn all(&self) -> bool { self.pick_all }
  pub fn id(&self) -> bool { self.show_id }
  pub fn name(&self) -> bool { self.show_name }
  pub fn root(&self) -> bool { self.show_root }
  pub fn tag_prefix(&self) -> bool { self.show_tag_prefix }
  pub fn full_version(&self) -> bool { self.show_full_version }
  pub fn version(&self) -> bool { self.show_version }

  pub fn pick_all(mut self, v: bool) -> InfoShow {
    self.pick_all = v;
    self
  }

  pub fn show_id(mut self, v: bool) -> InfoShow {
    self.show_id = v;
    self
  }

  pub fn show_name(mut self, v: bool) -> InfoShow {
    self.show_name = v;
    self
  }

  pub fn show_root(mut self, v: bool) -> InfoShow {
    self.show_root = v;
    self
  }

  pub fn show_tag_prefix(mut self, v: bool) -> InfoShow {
    self.show_tag_prefix = v;
    self
  }

  pub fn show_full_version(mut self, v: bool) -> InfoShow {
    self.show_full_version = v;
    self
  }

  pub fn show_version(mut self, v: bool) -> InfoShow {
    self.show_version = v;
    self
  }
}

pub async fn release(pref_vcs: Option<VcsRange>, all: bool, dry: bool, pause: bool) -> Result<()> {
  let mut mono = build(pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart)?;
  let output = Output::new();
  let mut output = output.release();
  let plan = mono.build_plan()?;

  if let Err((should, is)) = mono.check_branch() {
    bail!("Branch name \"{}\"\" doesn't match \"{}\".", is, should);
  }

  if plan.incrs().is_empty() {
    output.write_empty()?;
    output.commit();
    return Ok(());
  }

  let mut final_sizes = HashMap::new();
  for (id, (size, changelog)) in plan.incrs() {
    let proj = mono.get_project(id)?;
    let name = proj.name().to_string();
    let curt_config = mono.config();
    let prev_config = curt_config.slice_to_prev(mono.repo())?;
    let curt_vers = curt_config
      .get_value(id)
      .chain_err(|| format!("Unable to find project {} value.", id))?
      .unwrap_or_else(|| panic!("No such project {}.", id));
    let prev_vers = prev_config.get_value(id).chain_err(|| format!("Unable to find prev {} value.", id))?;

    let new_vers = if size == &Size::Empty {
      output.write_no_change(all, name.clone(), prev_vers.clone(), curt_vers.clone());
      curt_vers
    } else if let Some(prev_vers) = prev_vers {
      let target = size.apply(&prev_vers)?;
      if Size::less_than(&curt_vers, &target)? {
        proj.verify_restrictions(&target)?;
        mono.set_by_id(id, &target)?;
        output.write_changed(name.clone(), prev_vers.clone(), curt_vers.clone(), target.clone());
      } else {
        proj.verify_restrictions(&curt_vers)?;
        mono.forward_by_id(id, &curt_vers)?;
        output.write_forward(all, name.clone(), prev_vers.clone(), curt_vers.clone(), target.clone());
      }
      target
    } else {
      proj.verify_restrictions(&curt_vers)?;
      mono.forward_by_id(id, &curt_vers)?;
      output.write_new(all, name.clone(), curt_vers.clone());
      curt_vers
    };

    if let Some(wrote) = mono.write_changelog(id, changelog, &new_vers).await? {
      output.write_logged(wrote);
    }

    final_sizes.insert(id.clone(), new_vers);
  }

  mono.write_chains(plan.chain_writes(), &final_sizes)?;

  if !dry {
    mono.commit(true, pause)?;
    if pause {
      output.write_pause();
    } else {
      output.write_commit();
      output.write_done();
    }
  } else {
    output.write_dry();
  }

  output.commit();
  Ok(())
}

pub fn resume(user_pref_vcs: Option<VcsRange>) -> Result<()> {
  let vcs = combine_vcs(user_pref_vcs, VcsLevel::None, VcsLevel::Smart, VcsLevel::Local, VcsLevel::Smart)?;
  let repo = Repo::open(".", VcsState::new(vcs.max(), false))?;
  let output = Output::new();
  let mut output = output.resume();

  let mut commit: CommitState = {
    let file = File::open(".versio-paused")?;
    let reader = BufReader::new(file);
    let commit: CommitState = serde_json::from_reader(reader)?;

    // We must remove the pausefile before resuming, or else it will be committed.
    remove_file(".versio-paused")?;
    commit
  };
  commit.resume(&repo)?;

  output.write_done()?;
  output.commit()?;

  Ok(())
}

pub fn abort() -> Result<()> {
  remove_file(".versio-paused")?;
  println!("Release aborted. You may need to rollback your VCS \n(i.e `git checkout -- .`)");
  Ok(())
}

pub fn sanity_check() -> Result<()> {
  if Path::new(".versio-paused").exists() {
    bail!("versio is paused: use `release --resume` or `--abort`.")
  } else {
    Ok(())
  }
}

fn with_opts(
  user_pref_vcs: Option<VcsRange>, my_pref_lo: VcsLevel, my_pref_hi: VcsLevel, my_reqd_lo: VcsLevel,
  my_reqd_hi: VcsLevel, ignore_current: bool
) -> Result<Mono> {
  let vcs = combine_vcs(user_pref_vcs, my_pref_lo, my_pref_hi, my_reqd_lo, my_reqd_hi)?;
  Mono::here(VcsState::new(vcs.max(), ignore_current))
}

fn build(
  user_pref_vcs: Option<VcsRange>, my_pref_lo: VcsLevel, my_pref_hi: VcsLevel, my_reqd_lo: VcsLevel,
  my_reqd_hi: VcsLevel
) -> Result<Mono> {
  with_opts(user_pref_vcs, my_pref_lo, my_pref_hi, my_reqd_lo, my_reqd_hi, false)
}

fn combine_vcs(
  user_pref_vcs: Option<VcsRange>, my_pref_lo: VcsLevel, my_pref_hi: VcsLevel, my_reqd_lo: VcsLevel,
  my_reqd_hi: VcsLevel
) -> Result<VcsRange> {
  let pref_vcs = user_pref_vcs.unwrap_or_else(move || VcsRange::new(my_pref_lo, my_pref_hi));
  let reqd_vcs = VcsRange::new(my_reqd_lo, my_reqd_hi);
  VcsRange::detect_and_combine(&pref_vcs, &reqd_vcs)
}
