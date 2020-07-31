use crate::analyze::{analyze, Analysis};
use crate::config::{Config, ConfigFile, ProjectId, Size};
use crate::either::{IterEither2 as E2, IterEither3 as E3};
use crate::error::Result;
use crate::git::{CommitData, FullPr, Repo, Slice};
use crate::github::{changes, line_commits, Changes};
use crate::state::{CurrentState, StateRead, StateWrite};
use chrono::{DateTime, FixedOffset};
use std::cmp::max;
use std::collections::{HashMap, HashSet, VecDeque};
use std::iter;
use std::path::Path;

pub struct Mono {
  current: Config<CurrentState>,
  next: StateWrite,
  last_commits: HashMap<ProjectId, String>,
  repo: Repo
}

impl Mono {
  pub fn here() -> Result<Mono> { Mono::open(".") }

  pub fn open<P: AsRef<Path>>(dir: P) -> Result<Mono> {
    let repo = Repo::open(dir.as_ref())?;
    let current = Config::from_state(CurrentState::open(dir.as_ref(), old_tags)?)?;
    let last_commits = find_last_commits(&current, &repo)?;
    let next = StateWrite::new();

    Ok(Mono { current, next, last_commits, repo })
  }

  // pub fn current_source(&self) -> &CurrentSource { self.current.source() }
  // pub fn current_config(&self) -> &Config<CurrentSource> { &self.current }
  // pub fn old_tags(&self) -> &OldTags { &self.old_tags }
  // pub fn repo(&self) -> &Repo { &self.repo }
  // pub fn pull(&self) -> Result<()> { self.repo().pull() }
  // pub fn is_configured(&self) -> Result<bool> { Config::has_config_file(self.current_source()) }

  pub fn set_by_id(&self, id: ProjectId, val: &str) -> Result<()> {
    let proj = self.current.get_project(id).ok_or_else(|| versio_error!("No such project {}", id))?;
    proj.set_value(&mut self.next, val)
  }

  pub fn forward_by_id(&self, id: ProjectId, val: &str) -> Result<()> {
    let proj = self.current.get_project(id).ok_or_else(|| versio_error!("No such project {}", id))?;
    proj.forward_tag(&mut self.next, val)
  }

  pub fn set_by_name(&self, name: &str, val: &str) -> Result<()> {
    let id = self.current.find_unique(name)?;
    self.set_by_id(id, val)
  }

  pub fn changes(&self) -> Result<Changes> {
    let base = self.current.prev_tag().to_string();
    let head = self.repo.branch_name().to_string();
    changes(&self.repo, head, base)
  }

  pub fn keyed_files<'a>(&'a self) -> Result<impl Iterator<Item = Result<(String, String)>> + 'a> {
    let changes = self.changes()?;
    let prs = changes.into_groups().into_iter().map(|(_, v)| v).filter(|pr| !pr.best_guess());

    let mut vec = Vec::new();
    for pr in prs {
      vec.push(pr_keyed_files(&self.repo, pr));
    }

    Ok(vec.into_iter().flatten())
  }

  pub fn diff(&self) -> Result<Analysis> {
    let prev_config = self.current.slice_to_prev(&self.repo)?;

    let curt_annotate = self.current.annotate()?;
    let prev_annotate = prev_config.annotate()?;

    Ok(analyze(prev_annotate, curt_annotate))
  }

  // TODO: HERE: rejigger for Mono instead of Config

  // pub fn check(&self) -> Result<()> {
  //   for project in &self.file.projects {
  //     project.check(&self.source)?;
  //   }
  //   Ok(())
  // }

  // pub fn get_mark_value(&self, id: ProjectId) -> Option<Result<String>> {
  //   self.get_project(id).map(|p| p.get_mark_value(&self.source))
  // }

  // pub fn show(&self, format: ShowFormat) -> Result<()> {
  //   let name_width = self.file.projects.iter().map(|p| p.name.len()).max().unwrap_or(0);

  //   for project in &self.file.projects {
  //     project.show(&self.source, name_width, &format)?;
  //   }
  //   Ok(())
  // }

  // pub fn show_id(&self, id: ProjectId, format: ShowFormat) -> Result<()> {
  //   let project = self.get_project(id).ok_or_else(|| versio_error!("No such project {}", id))?;
  //   project.show(&self.source, 0, &format)
  // }

  // pub fn show_names(&self, name: &str, format: ShowFormat) -> Result<()> {
  //   let filter = |p: &&Project| p.name.contains(name);
  //   let name_width = self.file.projects.iter().filter(filter).map(|p| p.name.len()).max().unwrap_or(0);

  //   for project in self.file.projects.iter().filter(filter) {
  //     project.show(&self.source, name_width, &format)?;
  //   }
  //   Ok(())
  // }

  pub fn configure_plan(&self) -> Result<Plan> {
    let prev_spec = self.current.prev_tag().to_string();
    let head = self.repo.branch_name().to_string();

    let mut plan = PlanBuilder::create(self.repo.slice(prev_spec.clone()), self.current.file())?;

    // Consider the grouped, unsquashed commits to determine project sizing and changelogs.
    for pr in changes(&self.repo, head, prev_spec)?.groups().values() {
      plan.start_pr(pr)?;
      for commit in pr.included_commits() {
        plan.start_commit(commit.clone())?;
        for file in commit.files() {
          plan.start_file(file)?;
          plan.finish_file()?;
        }
        plan.finish_commit()?;
      }
      plan.finish_pr()?;
    }

    plan.handle_last_commits(&self.last_commits)?;

    // Some projects might depend on other projects.
    plan.handle_deps()?;

    // Sort projects by earliest closed date, mark duplicate commits.
    plan.sort_and_dedup()?;

    Ok(plan.build())
  }
}

/// Find the last covering commit ID, if any, for each current project.
fn find_last_commits(current: &Config<CurrentState>, repo: &Repo) -> Result<HashMap<ProjectId, String>> {
  let prev_spec = current.prev_tag().to_string();
  let head = repo.branch_name().to_string();

  let mut last_commits = LastCommitBuilder::create(repo.slice(prev_spec.clone()), &current)?;

  // Consider the in-line commits to determine the last commit (if any) for each project.
  for commit in line_commits(repo, head, prev_spec)? {
    last_commits.start_line_commit(&commit)?;
    for file in commit.files() {
      last_commits.start_line_file(file)?;
      last_commits.finish_line_file()?;
    }
    last_commits.finish_line_commit()?;
  }

  last_commits.build()
}

fn pr_keyed_files<'a>(repo: &'a Repo, pr: FullPr) -> impl Iterator<Item = Result<(String, String)>> + 'a {
  let head_oid = match pr.head_oid() {
    Some(oid) => *oid,
    None => return E3::C(iter::empty())
  };

  let iter = repo.commits_between(pr.base_oid(), head_oid).map(move |cmts| {
    cmts
      .filter_map(move |cmt| match cmt {
        Ok(cmt) => {
          if pr.has_exclude(&cmt.id()) {
            None
          } else {
            match cmt.files() {
              Ok(files) => {
                let kind = cmt.kind();
                Some(E2::A(files.map(move |f| Ok((kind.clone(), f)))))
              }
              Err(e) => Some(E2::B(iter::once(Err(e))))
            }
          }
        }
        Err(e) => Some(E2::B(iter::once(Err(e))))
      })
      .flatten()
  });

  match iter {
    Ok(iter) => E3::A(iter),
    Err(e) => E3::B(iter::once(Err(e)))
  }
}

pub struct Plan {
  incrs: HashMap<ProjectId, (Size, Option<String>, ChangeLog)>, // proj ID, incr size, last_commit, change log
  ineffective: Vec<LoggedPr>                                    // PRs that didn't apply to any project
}

impl Plan {
  pub fn incrs(&self) -> &HashMap<ProjectId, (Size, Option<String>, ChangeLog)> { &self.incrs }
  pub fn ineffective(&self) -> &[LoggedPr] { &self.ineffective }
}

pub struct ChangeLog {
  entries: Vec<(LoggedPr, Size)>
}

impl ChangeLog {
  pub fn empty() -> ChangeLog { ChangeLog { entries: Vec::new() } }
  pub fn entries(&self) -> &[(LoggedPr, Size)] { &self.entries }
  pub fn add_entry(&mut self, pr: LoggedPr, size: Size) { self.entries.push((pr, size)); }
}

pub struct LoggedPr {
  number: u32,
  closed_at: DateTime<FixedOffset>,
  commits: Vec<LoggedCommit>
}

impl LoggedPr {
  pub fn empty(number: u32, closed_at: DateTime<FixedOffset>) -> LoggedPr {
    LoggedPr { number, commits: Vec::new(), closed_at }
  }

  pub fn capture(pr: &FullPr) -> LoggedPr {
    LoggedPr { number: pr.number(), closed_at: *pr.closed_at(), commits: Vec::new() }
  }

  pub fn number(&self) -> u32 { self.number }
  pub fn closed_at(&self) -> &DateTime<FixedOffset> { &self.closed_at }
  pub fn commits(&self) -> &[LoggedCommit] { &self.commits }
}

pub struct LoggedCommit {
  oid: String,
  message: String,
  size: Size,
  applies: bool,
  duplicate: bool
}

impl LoggedCommit {
  pub fn new(oid: String, message: String, size: Size) -> LoggedCommit {
    LoggedCommit { oid, message, size, applies: false, duplicate: false }
  }

  pub fn applies(&self) -> bool { self.applies }
  pub fn duplicate(&self) -> bool { self.duplicate }
  pub fn included(&self) -> bool { self.applies && !self.duplicate }
  pub fn oid(&self) -> &str { &self.oid }
  pub fn message(&self) -> &str { &self.message }
  pub fn size(&self) -> Size { self.size }
}

struct PlanBuilder<'s> {
  on_pr_sizes: HashMap<ProjectId, LoggedPr>,
  on_ineffective: Option<LoggedPr>,
  on_commit: Option<CommitData>,
  prev: (Slice<'s>, ConfigFile),
  current: &'s ConfigFile,
  incrs: HashMap<ProjectId, (Size, Option<String>, ChangeLog)>, // proj ID, incr size, last_commit, change log
  ineffective: Vec<LoggedPr>                                    // PRs that didn't apply to any project
}

impl<'s> PlanBuilder<'s> {
  fn create(prev: Slice<'s>, current: &'s ConfigFile) -> Result<PlanBuilder<'s>> {
    let prev_file = ConfigFile::from_slice(&prev)?;
    let builder = PlanBuilder {
      on_pr_sizes: HashMap::new(),
      on_ineffective: None,
      on_commit: None,
      prev: (prev, prev_file),
      current,
      incrs: HashMap::new(),
      ineffective: Vec::new()
    };
    Ok(builder)
  }

  pub fn start_pr(&mut self, pr: &FullPr) -> Result<()> {
    self.on_pr_sizes = self.current.projects().iter().map(|p| (p.id(), LoggedPr::capture(pr))).collect();
    self.on_ineffective = Some(LoggedPr::capture(pr));
    Ok(())
  }

  pub fn finish_pr(&mut self) -> Result<()> {
    let mut found = false;
    for (proj_id, logged_pr) in self.on_pr_sizes.drain() {
      let (size, _, change_log) = self.incrs.entry(proj_id).or_insert((Size::None, None, ChangeLog::empty()));
      let pr_size = logged_pr.commits.iter().filter(|c| c.applies).map(|c| c.size).max();
      if let Some(pr_size) = pr_size {
        found = true;
        *size = max(*size, pr_size);
        change_log.add_entry(logged_pr, pr_size);
      }
    }

    let ineffective = self.on_ineffective.take().unwrap();
    if !found {
      self.ineffective.push(ineffective);
    }

    Ok(())
  }

  pub fn start_commit(&mut self, commit: CommitData) -> Result<()> {
    let id = commit.id().to_string();
    let kind = commit.kind().to_string();
    let summary = commit.summary().to_string();
    self.on_commit = Some(commit);
    self.slice_to(id.clone())?;

    for (proj_id, logged_pr) in &mut self.on_pr_sizes {
      if let Some(cur_project) = self.current.get_project(*proj_id) {
        let size = cur_project.size(&self.current.sizes(), &kind)?;
        logged_pr.commits.push(LoggedCommit::new(id.clone(), summary.clone(), size));
      }
    }

    Ok(())
  }

  pub fn finish_commit(&mut self) -> Result<()> { Ok(()) }

  pub fn start_file(&mut self, path: &str) -> Result<()> {
    let commit = self.on_commit.as_ref().ok_or_else(|| versio_error!("Not on a commit"))?;
    let commit_id = commit.id();

    for prev_project in self.prev.1.projects() {
      if let Some(logged_pr) = self.on_pr_sizes.get_mut(&prev_project.id()) {
        if prev_project.does_cover(path)? {
          let LoggedCommit { applies, .. } = logged_pr.commits.iter_mut().find(|c| c.oid == commit_id).unwrap();
          *applies = true;
        }
      }
    }
    Ok(())
  }

  pub fn finish_file(&mut self) -> Result<()> { Ok(()) }

  pub fn handle_deps(&mut self) -> Result<()> {
    // Use a modified Kahn's algorithm to traverse deps in order.
    let mut queue: VecDeque<(ProjectId, Size)> = VecDeque::new();

    let mut dependents: HashMap<ProjectId, HashSet<ProjectId>> = HashMap::new();
    for project in self.current.projects() {
      for dep in project.depends() {
        dependents.entry(*dep).or_insert_with(HashSet::new).insert(project.id());
      }

      if project.depends().is_empty() {
        if let Some((size, ..)) = self.incrs.get(&project.id()) {
          queue.push_back((project.id(), *size));
        } else {
          queue.push_back((project.id(), Size::None))
        }
      }
    }

    while let Some((id, size)) = queue.pop_front() {
      let val = &mut self.incrs.entry(id).or_insert((Size::None, None, ChangeLog::empty())).0;
      *val = max(*val, size);

      let depds: Option<HashSet<ProjectId>> = dependents.get(&id).cloned();
      if let Some(depds) = depds {
        for depd in depds {
          dependents.get_mut(&id).unwrap().remove(&depd);
          let val = &mut self.incrs.entry(depd).or_insert((Size::None, None, ChangeLog::empty())).0;
          *val = max(*val, size);

          if dependents.values().all(|ds| !ds.contains(&depd)) {
            queue.push_back((depd, *val));
          }
        }
      }
    }

    Ok(())
  }

  pub fn sort_and_dedup(&mut self) -> Result<()> {
    for (.., change_log) in self.incrs.values_mut() {
      change_log.entries.sort_by_key(|(pr, _)| *pr.closed_at());

      let mut seen_commits = HashSet::new();
      for (pr, size) in &mut change_log.entries {
        for LoggedCommit { oid, duplicate, .. } in &mut pr.commits {
          if seen_commits.contains(oid) {
            *duplicate = true;
          }
          seen_commits.insert(oid.clone());
        }
        *size = pr.commits().iter().filter(|c| c.included()).map(|c| c.size).max().unwrap_or(Size::None);
      }
    }
    Ok(())
  }

  pub fn handle_last_commits(&mut self, lasts: &HashMap<ProjectId, String>) -> Result<()> {
    for (proj_id, found_commit) in lasts {
      if let Some((_, last_commit, _)) = self.incrs.get_mut(proj_id) {
        *last_commit = Some(found_commit.clone());
      }
    }
    Ok(())
  }

  pub fn build(self) -> Plan { Plan { incrs: self.incrs, ineffective: self.ineffective } }

  fn slice_to(&mut self, id: String) -> Result<()> {
    let prev = self.prev.0.slice(id);
    let file = ConfigFile::from_slice(&prev)?;
    self.prev = (prev, file);
    Ok(())
  }
}

struct LastCommitBuilder<'s, C: StateRead> {
  on_line_commit: Option<String>,
  last_commits: HashMap<ProjectId, String>,
  prev: (Slice<'s>, ConfigFile),
  current: &'s Config<C>
}

impl<'s, C: StateRead> LastCommitBuilder<'s, C> {
  fn create(prev: Slice<'s>, current: &'s Config<C>) -> Result<LastCommitBuilder<'s, C>> {
    let file = ConfigFile::from_slice(&prev)?;
    let builder = LastCommitBuilder { on_line_commit: None, last_commits: HashMap::new(), prev: (prev, file), current };
    Ok(builder)
  }

  pub fn start_line_commit(&mut self, commit: &CommitData) -> Result<()> {
    let id = commit.id().to_string();
    self.on_line_commit = Some(id.clone());
    self.slice_to(id)?;
    Ok(())
  }

  pub fn finish_line_commit(&mut self) -> Result<()> { Ok(()) }

  pub fn start_line_file(&mut self, path: &str) -> Result<()> {
    let commit_id = self.on_line_commit.as_ref().ok_or_else(|| versio_error!("Not on a line commit"))?;

    for prev_project in self.prev.1.projects() {
      let proj_id = prev_project.id();
      if self.current.get_project(proj_id).is_some() && prev_project.does_cover(path)? {
        self.last_commits.insert(proj_id, commit_id.clone());
      }
    }
    Ok(())
  }

  pub fn finish_line_file(&mut self) -> Result<()> { Ok(()) }

  pub fn build(self) -> Result<HashMap<ProjectId, String>> { Ok(self.last_commits) }

  fn slice_to(&mut self, id: String) -> Result<()> {
    let prev = self.prev.0.slice(id);
    let file = ConfigFile::from_slice(&prev)?;
    self.prev = (prev, file);
    Ok(())
  }
}
