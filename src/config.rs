//! The configuration and top-level commands for Versio.

use crate::analyze::{analyze, Analysis, AnnotatedMark};
use crate::error::Result;
use crate::git::{CommitData, FullPr, Repo};
use crate::scan::parts::{deserialize_parts, Part};
use crate::scan::{JsonScanner, Scanner, TomlScanner, XmlScanner, YamlScanner};
use crate::source::{CurrentSource, Mark, MarkedData, NamedData, PrevSource, SliceSource, Source, CONFIG_FILENAME};
use chrono::{DateTime, FixedOffset};
use glob::{glob_with, MatchOptions, Pattern};
use regex::Regex;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use std::borrow::Cow;
use std::cmp::{max, Ord, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};

type ProjectId = u32;

pub struct Mono {
  current: Config<CurrentSource>,
  previous: Config<PrevSource>
}

impl Mono {
  pub fn open<P: AsRef<Path>>(dir: P) -> Result<Mono> {
    Ok(Mono {
      current: Config::from_source(CurrentSource::open(dir.as_ref())?)?,
      previous: Config::from_source(PrevSource::open(dir.as_ref())?)?
    })
  }

  pub fn here() -> Result<Mono> { Mono::open(".") }
  pub fn current_source(&self) -> &CurrentSource { self.current.source() }
  pub fn previous_source(&self) -> &PrevSource { self.previous.source() }
  pub fn current_config(&self) -> &Config<CurrentSource> { &self.current }
  pub fn previous_config(&self) -> &Config<PrevSource> { &self.previous }
  pub fn repo(&self) -> Result<&Repo> { self.previous_source().repo() }
  pub fn pull(&self) -> Result<()> { self.previous_source().pull() }
  pub fn is_configured(&self) -> Result<bool> { Config::has_config_file(self.current_source()) }

  pub fn set_by_id(&self, id: ProjectId, val: &str, new_tags: &mut NewTags) -> Result<()> {
    let last_commits = find_last_commits(self)?;
    self.current_config().set_by_id(id, val, last_commits.get(&id), new_tags)
  }

  pub fn set_by_name(&self, name: &str, val: &str, new_tags: &mut NewTags) -> Result<()> {
    let curt_cfg = self.current_config();
    let id = curt_cfg.find_unique(name)?;
    let last_commits = find_last_commits(self)?;
    curt_cfg.set_by_id(id, val, last_commits.get(&id), new_tags)
  }

  pub fn keyed_files<'a>(&'a self) -> Result<impl Iterator<Item = Result<(String, String)>> + 'a> {
    self.previous_source().keyed_files()
  }

  pub fn diff(&self) -> Result<Analysis> {
    let prev_at = self.previous_config().annotate()?;
    let curt_at = self.current_config().annotate()?;
    Ok(analyze(prev_at, curt_at))
  }
}

/// Find the last covering commit ID, if any, for each current project.
fn find_last_commits(mono: &Mono) -> Result<HashMap<ProjectId, String>> {
  let prev_config = mono.previous_config();
  let curt_config = mono.current_config();
  let prev = mono.previous_source();

  let mut last_finder = prev_config.start_last_finder(&curt_config);

  // Consider the in-line commits to determine the last commit (if any) for each project.
  for commit in prev.line_commits()? {
    last_finder.consider_line_commit(&commit)?;
    for file in commit.files() {
      last_finder.consider_line_file(file)?;
      last_finder.finish_line_file()?;
    }
    last_finder.finish_line_commit()?;
  }

  last_finder.finish_finder()
}

pub fn configure_plan(mono: &Mono) -> Result<Plan> {
  let prev_config = mono.previous_config();
  let curt_config = mono.current_config();
  let prev = mono.previous_source();
  let mut plan = prev_config.start_plan(&curt_config);

  // Consider the grouped, unsquashed commits to determine project sizing and changelogs.
  for pr in prev.changes()?.groups().values() {
    plan.consider_pr(pr)?;
    for commit in pr.included_commits() {
      plan.consider_commit(commit.clone())?;
      for file in commit.files() {
        plan.consider_file(file)?;
        plan.finish_file()?;
      }
      plan.finish_commit()?;
    }
    plan.finish_pr()?;
  }

  let last_commits = find_last_commits(mono)?;
  plan.consider_last_commits(&last_commits)?;

  // Some projects might depend on other projects.
  plan.consider_deps()?;

  // Sort projects by earliest closed date, mark duplicate commits.
  plan.sort_and_dedup()?;

  plan.finish_plan()
}

pub struct ShowFormat {
  pub wide: bool,
  pub version_only: bool
}

impl ShowFormat {
  pub fn new(wide: bool, version_only: bool) -> ShowFormat { ShowFormat { wide, version_only } }
}

pub struct Config<S: Source> {
  source: S,
  file: ConfigFile
}

impl Config<PrevSource> {
  fn start_plan<'s, C: Source>(&'s self, current: &'s Config<C>) -> PlanConsider<'s, C> {
    PlanConsider::new(self, current)
  }

  fn start_last_finder<'s, C: Source>(&'s self, current: &'s Config<C>) -> LastCommitFinder<'s, C> {
    LastCommitFinder::new(self, current)
  }

  fn slice(&self, spec: String) -> Result<Config<SliceSource>> { Config::from_source(self.source.slice(spec)) }
}

impl<'s> Config<SliceSource<'s>> {
  fn slice(&self, spec: String) -> Result<Config<SliceSource<'s>>> { Config::from_source(self.source.slice(spec)) }
}

impl<S: Source> Config<S> {
  pub fn has_config_file(source: S) -> Result<bool> { source.has(CONFIG_FILENAME.as_ref()) }
  pub fn source(&self) -> &S { &self.source }

  pub fn from_source(source: S) -> Result<Config<S>> {
    let file = ConfigFile::load(&source)?;
    Ok(Config { source, file })
  }

  pub fn annotate(&self) -> Result<Vec<AnnotatedMark>> {
    self.file.projects.iter().map(|p| p.annotate(&self.source)).collect()
  }

  pub fn check(&self) -> Result<()> {
    for project in &self.file.projects {
      project.check(&self.source)?;
    }
    Ok(())
  }

  pub fn get_mark(&self, id: ProjectId) -> Option<Result<MarkedData>> {
    self.get_project(id).map(|p| p.get_mark(&self.source))
  }

  pub fn show(&self, format: ShowFormat) -> Result<()> {
    let name_width = self.file.projects.iter().map(|p| p.name.len()).max().unwrap_or(0);

    for project in &self.file.projects {
      project.show(&self.source, name_width, &format)?;
    }
    Ok(())
  }

  pub fn show_id(&self, id: ProjectId, format: ShowFormat) -> Result<()> {
    let project = self.get_project(id).ok_or_else(|| versio_error!("No such project {}", id))?;
    project.show(&self.source, 0, &format)
  }

  pub fn show_names(&self, name: &str, format: ShowFormat) -> Result<()> {
    let filter = |p: &&Project| p.name.contains(name);
    let name_width = self.file.projects.iter().filter(filter).map(|p| p.name.len()).max().unwrap_or(0);

    for project in self.file.projects.iter().filter(filter) {
      project.show(&self.source, name_width, &format)?;
    }
    Ok(())
  }

  pub fn set_by_id(
    &self, id: ProjectId, val: &str, _last_commit: Option<&String>, new_tags: &mut NewTags
  ) -> Result<()> {
    let project =
      self.file.projects.iter().find(|p| p.id == id).ok_or_else(|| versio_error!("No such project {}", id))?;
    project.set_value(&self.source, val, new_tags)
  }

  pub fn forward_by_id(
    &self, id: ProjectId, val: &str, last_commit: Option<&String>, new_tags: &mut NewTags,
    wrote_something: bool
  ) -> Result<()> {
    let project =
      self.file.projects.iter().find(|p| p.id == id).ok_or_else(|| versio_error!("No such project {}", id))?;
    project.forward_value(val, last_commit, new_tags, wrote_something)
  }

  pub fn get_project(&self, id: ProjectId) -> Option<&Project> { self.file.projects.iter().find(|p| p.id == id) }

  fn find_unique(&self, name: &str) -> Result<ProjectId> {
    let mut iter = self.file.projects.iter().filter(|p| p.name.contains(name)).map(|p| p.id);
    let id = iter.next().ok_or_else(|| versio_error!("No project named {}", name))?;
    if iter.next().is_some() {
      return versio_err!("Multiple projects with name {}", name);
    }
    Ok(id)
  }
}

pub struct Plan {
  incrs: HashMap<ProjectId, (Size, Option<String>, ChangeLog)>, // proj ID, incr size, last_commit, change log
  ineffective: Vec<SizedPr>                                     // PRs that didn't apply to any project
}

impl Plan {
  pub fn incrs(&self) -> &HashMap<ProjectId, (Size, Option<String>, ChangeLog)> { &self.incrs }
  pub fn ineffective(&self) -> &[SizedPr] { &self.ineffective }
}

pub struct ChangeLog {
  entries: Vec<(SizedPr, Size)>
}

impl ChangeLog {
  pub fn empty() -> ChangeLog { ChangeLog { entries: Vec::new() } }
  pub fn entries(&self) -> &[(SizedPr, Size)] { &self.entries }

  pub fn add_entry(&mut self, pr: SizedPr, size: Size) { self.entries.push((pr, size)); }
}

pub struct SizedPr {
  number: u32,
  closed_at: DateTime<FixedOffset>,
  commits: Vec<SizedPrCommit>
}

impl SizedPr {
  pub fn empty(number: u32, closed_at: DateTime<FixedOffset>) -> SizedPr {
    SizedPr { number, commits: Vec::new(), closed_at }
  }

  pub fn capture(pr: &FullPr) -> SizedPr {
    SizedPr { number: pr.number(), closed_at: *pr.closed_at(), commits: Vec::new() }
  }

  pub fn number(&self) -> u32 { self.number }
  pub fn closed_at(&self) -> &DateTime<FixedOffset> { &self.closed_at }
  pub fn commits(&self) -> &[SizedPrCommit] { &self.commits }
}

pub struct SizedPrCommit {
  oid: String,
  message: String,
  size: Size,
  applies: bool,
  duplicate: bool
}

impl SizedPrCommit {
  pub fn new(oid: String, message: String, size: Size) -> SizedPrCommit {
    SizedPrCommit { oid, message, size, applies: false, duplicate: false }
  }

  pub fn applies(&self) -> bool { self.applies }
  pub fn duplicate(&self) -> bool { self.duplicate }
  pub fn included(&self) -> bool { self.applies && !self.duplicate }
  pub fn oid(&self) -> &str { &self.oid }
  pub fn message(&self) -> &str { &self.message }
  pub fn size(&self) -> Size { self.size }
}

pub struct PlanConsider<'s, C: Source> {
  on_pr_sizes: HashMap<ProjectId, SizedPr>,
  on_ineffective: Option<SizedPr>,
  on_commit: Option<CommitData>,
  prev: OnPrev<'s>,
  current: &'s Config<C>,
  incrs: HashMap<ProjectId, (Size, Option<String>, ChangeLog)>, // proj ID, incr size, last_commit, change log
  ineffective: Vec<SizedPr>                                     // PRs that didn't apply to any project
}

impl<'s, C: Source> PlanConsider<'s, C> {
  fn new(prev: &'s Config<PrevSource>, current: &'s Config<C>) -> PlanConsider<'s, C> {
    let prev = OnPrev::Initial(prev);
    PlanConsider {
      on_pr_sizes: HashMap::new(),
      on_ineffective: None,
      on_commit: None,
      prev,
      current,
      incrs: HashMap::new(),
      ineffective: Vec::new()
    }
  }

  pub fn consider_pr(&mut self, pr: &FullPr) -> Result<()> {
    self.on_pr_sizes = self.current.file.projects.iter().map(|p| (p.id(), SizedPr::capture(pr))).collect();
    self.on_ineffective = Some(SizedPr::capture(pr));
    Ok(())
  }

  pub fn finish_pr(&mut self) -> Result<()> {
    let mut found = false;
    for (proj_id, sized_pr) in self.on_pr_sizes.drain() {
      let (size, _, change_log) = self.incrs.entry(proj_id).or_insert((Size::None, None, ChangeLog::empty()));
      let pr_size = sized_pr.commits.iter().filter(|c| c.applies).map(|c| c.size).max();
      if let Some(pr_size) = pr_size {
        found = true;
        *size = max(*size, pr_size);
        change_log.add_entry(sized_pr, pr_size);
      }
    }

    let ineffective = self.on_ineffective.take().unwrap();
    if !found {
      self.ineffective.push(ineffective);
    }

    Ok(())
  }

  pub fn consider_commit(&mut self, commit: CommitData) -> Result<()> {
    let id = commit.id().to_string();
    let kind = commit.kind().to_string();
    let summary = commit.summary().to_string();
    self.on_commit = Some(commit);
    self.prev = self.prev.slice(id.clone())?;

    for (proj_id, sized_pr) in &mut self.on_pr_sizes {
      if let Some(cur_project) = self.current.get_project(*proj_id) {
        let size = cur_project.size(&self.current.file.sizes, &kind)?;
        sized_pr.commits.push(SizedPrCommit::new(id.clone(), summary.clone(), size));
      }
    }

    Ok(())
  }

  pub fn finish_commit(&mut self) -> Result<()> { Ok(()) }

  pub fn consider_file(&mut self, path: &str) -> Result<()> {
    let commit = self.on_commit.as_ref().ok_or_else(|| versio_error!("Not on a commit"))?;
    let commit_id = commit.id();

    for prev_project in &self.prev.file().projects {
      if let Some(sized_pr) = self.on_pr_sizes.get_mut(&prev_project.id) {
        if prev_project.does_cover(path)? {
          let SizedPrCommit { applies, .. } = sized_pr.commits.iter_mut().find(|c| c.oid == commit_id).unwrap();
          *applies = true;
        }
      }
    }
    Ok(())
  }

  pub fn finish_file(&mut self) -> Result<()> { Ok(()) }

  pub fn consider_deps(&mut self) -> Result<()> {
    // Use a modified Kahn's algorithm to traverse deps in order.
    let mut queue: VecDeque<(ProjectId, Size)> = VecDeque::new();

    let mut dependents: HashMap<ProjectId, HashSet<ProjectId>> = HashMap::new();
    for project in &self.current.file.projects {
      for dep in &project.depends {
        dependents.entry(*dep).or_insert_with(HashSet::new).insert(project.id);
      }

      if project.depends.is_empty() {
        if let Some((size, ..)) = self.incrs.get(&project.id) {
          queue.push_back((project.id, *size));
        } else {
          queue.push_back((project.id, Size::None))
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
        for SizedPrCommit { oid, duplicate, .. } in &mut pr.commits {
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

  pub fn consider_last_commits(&mut self, lasts: &HashMap<ProjectId, String>) -> Result<()> {
    for (proj_id, found_commit) in lasts {
      if let Some((_, last_commit, _)) = self.incrs.get_mut(proj_id) {
        *last_commit = Some(found_commit.clone());
      }
    }
    Ok(())
  }

  pub fn finish_plan(self) -> Result<Plan> { Ok(Plan { incrs: self.incrs, ineffective: self.ineffective }) }
}

enum OnPrev<'s> {
  Initial(&'s Config<PrevSource>),
  Updated(Config<SliceSource<'s>>)
}

impl<'s> OnPrev<'s> {
  pub fn file(&self) -> &ConfigFile {
    match self {
      OnPrev::Initial(c) => &c.file,
      OnPrev::Updated(c) => &c.file
    }
  }

  pub fn slice(&self, spec: String) -> Result<OnPrev<'s>> {
    match self {
      OnPrev::Updated(c) => c.slice(spec).map(OnPrev::Updated),
      OnPrev::Initial(c) => c.slice(spec).map(OnPrev::Updated)
    }
  }
}

pub struct LastCommitFinder<'s, C: Source> {
  on_line_commit: Option<String>,
  last_commits: HashMap<ProjectId, String>,
  prev: OnPrev<'s>,
  current: &'s Config<C>
}

impl<'s, C: Source> LastCommitFinder<'s, C> {
  fn new(prev: &'s Config<PrevSource>, current: &'s Config<C>) -> LastCommitFinder<'s, C> {
    let prev = OnPrev::Initial(prev);
    LastCommitFinder { on_line_commit: None, last_commits: HashMap::new(), prev, current }
  }

  pub fn consider_line_commit(&mut self, commit: &CommitData) -> Result<()> {
    let id = commit.id().to_string();
    self.on_line_commit = Some(id.clone());
    self.prev = self.prev.slice(id)?;
    Ok(())
  }

  pub fn finish_line_commit(&mut self) -> Result<()> { Ok(()) }

  pub fn consider_line_file(&mut self, path: &str) -> Result<()> {
    let commit_id = self.on_line_commit.as_ref().ok_or_else(|| versio_error!("Not on a line commit"))?;

    for prev_project in &self.prev.file().projects {
      let proj_id = prev_project.id();
      if self.current.get_project(proj_id).is_some() && prev_project.does_cover(path)? {
        self.last_commits.insert(proj_id, commit_id.clone());
      }
    }
    Ok(())
  }

  pub fn finish_line_file(&mut self) -> Result<()> { Ok(()) }

  pub fn finish_finder(self) -> Result<HashMap<ProjectId, String>> { Ok(self.last_commits) }
}

#[derive(Deserialize, Debug)]
pub struct ConfigFile {
  projects: Vec<Project>,
  #[serde(deserialize_with = "deserialize_sizes", default)]
  sizes: HashMap<String, Size>
}

impl ConfigFile {
  pub fn load(source: &dyn Source) -> Result<ConfigFile> {
    match source.load(CONFIG_FILENAME.as_ref())? {
      Some(data) => ConfigFile::read(data.data()),
      None => Ok(ConfigFile::empty())
    }
  }

  pub fn empty() -> ConfigFile { ConfigFile { projects: Vec::new(), sizes: HashMap::new() } }

  pub fn read(data: &str) -> Result<ConfigFile> {
    let file: ConfigFile = serde_yaml::from_str(data)?;
    file.validate()?;
    Ok(file)
  }

  /// Check that IDs are unique, etc.
  fn validate(&self) -> Result<()> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let mut prefs = HashSet::new();

    for p in &self.projects {
      if ids.contains(&p.id) {
        return versio_err!("id {} is duplicated", p.id);
      }
      ids.insert(p.id);

      if names.contains(&p.name) {
        return versio_err!("name {} is duplicated", p.name);
      }
      names.insert(p.name.clone());

      if let Some(pref) = &p.tag_prefix {
        if prefs.contains(pref) {
          return versio_err!("tag_prefix {} is duplicated", pref);
        }
        if !legal_tag(pref) {
          return versio_err!("illegal tag_prefix \"{}\"", pref);
        }
        prefs.insert(pref.clone());
      }
    }

    // TODO: no circular deps

    Ok(())
  }
}

fn legal_tag(prefix: &str) -> bool {
  prefix.is_empty()
    || ((prefix.starts_with('_') || prefix.chars().next().unwrap().is_alphabetic())
      && (prefix.chars().all(|c| c.is_ascii() && (c == '_' || c == '-' || c.is_alphanumeric()))))
}

#[derive(Deserialize, Debug)]
pub struct Project {
  name: String,
  id: ProjectId,
  root: Option<String>,
  #[serde(default)]
  includes: Vec<String>,
  #[serde(default)]
  excludes: Vec<String>,
  #[serde(default)]
  depends: Vec<ProjectId>,
  change_log: Option<String>,
  located: Location,
  tag_prefix: Option<String>
}

impl Project {
  fn annotate(&self, source: &dyn Source) -> Result<AnnotatedMark> {
    Ok(AnnotatedMark::new(self.id, self.name.clone(), self.get_mark(source)?))
  }

  pub fn root(&self) -> &Option<String> { &self.root }
  pub fn name(&self) -> &str { &self.name }
  pub fn id(&self) -> ProjectId { self.id }
  pub fn depends(&self) -> &[ProjectId] { &self.depends }

  pub fn change_log(&self) -> Option<Cow<str>> {
    self.change_log.as_ref().map(|change_log| {
      if let Some(root) = &self.root {
        Cow::Owned(PathBuf::from(root).join(change_log).to_string_lossy().to_string())
      } else {
        Cow::Borrowed(change_log.as_str())
      }
    })
  }

  pub fn tag_prefix(&self) -> &Option<String> { &self.tag_prefix }

  pub fn write_change_log(&self, cl: &ChangeLog, src: &dyn Source) -> Result<Option<String>> {
    // TODO: only write change log if any commits are found, else return `None`

    if let Some(cl_path) = self.change_log().as_ref() {
      let log_path = src.root_dir().join(cl_path.deref());
      std::fs::write(&log_path, construct_change_log_html(cl)?)?;
      Ok(Some(cl_path.to_string()))
    } else {
      Ok(None)
    }
  }

  fn get_mark(&self, source: &dyn Source) -> Result<MarkedData> { self.located.get_mark(source, &self.root) }

  fn size(&self, parent_sizes: &HashMap<String, Size>, kind: &str) -> Result<Size> {
    let kind = kind.trim();
    if kind.ends_with('!') {
      return Ok(Size::Major);
    }
    parent_sizes.get(kind).copied().map(Ok).unwrap_or_else(|| {
      parent_sizes.get("*").copied().map(Ok).unwrap_or_else(|| versio_err!("Unknown kind \"{}\".", kind))
    })
  }

  pub fn does_cover(&self, path: &str) -> Result<bool> {
    let excludes = self.excludes.iter().try_fold::<_, _, Result<_>>(false, |val, cov| {
      Ok(val || Pattern::new(&self.rooted_pattern(cov))?.matches_with(path, match_opts()))
    })?;

    if excludes {
      return Ok(false);
    }

    self.includes.iter().try_fold(false, |val, cov| {
      Ok(val || Pattern::new(&self.rooted_pattern(cov))?.matches_with(path, match_opts()))
    })
  }

  fn check(&self, source: &dyn Source) -> Result<()> {
    // Check that we can find the given mark.
    self.get_mark(source)?;

    self.check_excludes()?;

    // Check that each pattern includes at least one file.
    for cov in &self.includes {
      let pattern = self.rooted_pattern(cov);
      let cover = absolutize_pattern(&pattern, source.root_dir());
      if !glob_with(&cover, match_opts())?.any(|_| true) {
        return versio_err!("No files in proj. {} covered by \"{}\".", self.id, cover);
      }
    }

    Ok(())
  }

  /// Ensure that we don't have excludes without includes.
  fn check_excludes(&self) -> Result<()> {
    if !self.excludes.is_empty() && self.includes.is_empty() {
      return versio_err!("Proj {} has excludes, but no includes.", self.id);
    }

    Ok(())
  }

  fn show(&self, source: &dyn Source, name_width: usize, format: &ShowFormat) -> Result<()> {
    let mark = self.get_mark(source)?;
    if format.version_only {
      println!("{}", mark.value());
    } else if format.wide {
      println!("{:>4}. {:width$} : {}", self.id, self.name, mark.value(), width = name_width);
    } else {
      println!("{:width$} : {}", self.name, mark.value(), width = name_width);
    }
    Ok(())
  }

  fn set_value(&self, source: &dyn Source, val: &str, new_tags: &mut NewTags) -> Result<()> {
    let mut mark = self.get_mark(source)?;
    mark.write_new_value(val)?;

    self.will_commit(val, new_tags)
  }

  fn forward_value(
    &self, val: &str, last_commit: Option<&String>, new_tags: &mut NewTags, wrote_something: bool
  ) -> Result<()> {
    if wrote_something {
      return self.will_commit(val, new_tags);
    }

    if let Some(tag_prefix) = &self.tag_prefix {
      if let Some(last_commit) = last_commit {
        if tag_prefix.is_empty() {
          new_tags.change_tag(format!("v{}", val), last_commit);
        } else {
          new_tags.change_tag(format!("{}-v{}", tag_prefix, val), last_commit);
        }
      }
    }

    Ok(())
  }

  fn will_commit(&self, val: &str, new_tags: &mut NewTags) -> Result<()> {
    new_tags.flag_commit();
    if let Some(tag_prefix) = &self.tag_prefix {
      if tag_prefix.is_empty() {
        new_tags.add_tag(format!("v{}", val));
      } else {
        new_tags.add_tag(format!("{}-v{}", tag_prefix, val));
      }
    }

    Ok(())
  }

  fn rooted_pattern(&self, pat: &str) -> String {
    if let Some(root) = &self.root {
      PathBuf::from(root).join(pat).to_string_lossy().to_string()
    } else {
      pat.to_string()
    }
  }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum Location {
  File(FileLocation),
  Tag(TagLocation)
}

impl Location {
  pub fn get_mark(&self, source: &dyn Source, root: &Option<String>) -> Result<MarkedData> {
    match self {
      Location::File(l) => l.get_mark(source, root),
      Location::Tag(l) => l.get_mark(source, root)
    }
  }

  #[cfg(test)]
  pub fn picker(&self) -> &Picker {
    match self {
      Location::File(l) => &l.picker,
      _ => panic!("Not a file location")
    }
  }
}

#[derive(Deserialize, Debug)]
struct TagLocation {}

impl TagLocation {
  pub fn get_mark(&self, _source: &dyn Source, _root: &Option<String>) -> Result<MarkedData> { unimplemented!() }
}

#[derive(Deserialize, Debug)]
struct FileLocation {
  file: String,
  #[serde(flatten)]
  picker: Picker
}

impl FileLocation {
  pub fn get_mark(&self, source: &dyn Source, root: &Option<String>) -> Result<MarkedData> {
    let file = match root {
      Some(root) => PathBuf::from(root).join(&self.file),
      None => PathBuf::from(&self.file)
    };

    let data = source.load(&file)?.ok_or_else(|| versio_error!("No file at {}.", file.to_string_lossy()))?;
    self.picker.get_mark(data).map_err(|e| versio_error!("Can't mark {}: {:?}", file.to_string_lossy(), e))
  }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum Picker {
  Json(JsonPicker),
  Yaml(YamlPicker),
  Toml(TomlPicker),
  Xml(XmlPicker),
  Line(LinePicker),
  File(FilePicker)
}

impl Picker {
  #[cfg(test)]
  pub fn picker_type(&self) -> &'static str {
    match self {
      Picker::Json(_) => "json",
      Picker::Yaml(_) => "yaml",
      Picker::Toml(_) => "toml",
      Picker::Xml(_) => "xml",
      Picker::Line(_) => "line",
      Picker::File(_) => "file"
    }
  }

  pub fn get_mark(&self, data: NamedData) -> Result<MarkedData> {
    match self {
      Picker::Json(p) => p.scan(data),
      Picker::Yaml(p) => p.scan(data),
      Picker::Toml(p) => p.scan(data),
      Picker::Xml(p) => p.scan(data),
      Picker::Line(p) => p.scan(data),
      Picker::File(p) => p.scan(data)
    }
  }
}

#[derive(Deserialize, Debug)]
struct JsonPicker {
  #[serde(deserialize_with = "deserialize_parts")]
  json: Vec<Part>
}

impl JsonPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { JsonScanner::new(self.json.clone()).scan(data) }
}

#[derive(Deserialize, Debug)]
struct YamlPicker {
  #[serde(deserialize_with = "deserialize_parts")]
  yaml: Vec<Part>
}

impl YamlPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { YamlScanner::new(self.yaml.clone()).scan(data) }
}

#[derive(Deserialize, Debug)]
struct TomlPicker {
  #[serde(deserialize_with = "deserialize_parts")]
  toml: Vec<Part>
}

impl TomlPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { TomlScanner::new(self.toml.clone()).scan(data) }
}

#[derive(Deserialize, Debug)]
struct XmlPicker {
  #[serde(deserialize_with = "deserialize_parts")]
  xml: Vec<Part>
}

impl XmlPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { XmlScanner::new(self.xml.clone()).scan(data) }
}

#[derive(Deserialize, Debug)]
struct LinePicker {
  pattern: String
}

impl LinePicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { find_reg_data(data, &self.pattern) }
}

fn find_reg_data(data: NamedData, pattern: &str) -> Result<MarkedData> {
  let pattern = Regex::new(pattern)?;
  let found = pattern.captures(data.data()).ok_or_else(|| versio_error!("No match for {}", pattern))?;
  let item = found.get(1).ok_or_else(|| versio_error!("No capture group in {}.", pattern))?;
  let value = item.as_str().to_string();
  let index = item.start();
  Ok(data.mark(Mark::make(value, index)?))
}

#[derive(Deserialize, Debug)]
struct FilePicker {}

impl FilePicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> {
    let value = data.data().trim_end().to_string();
    Ok(data.mark(Mark::make(value, 0)?))
  }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
  Fail,
  Major,
  Minor,
  Patch,
  None
}

impl Size {
  fn is_size(v: &str) -> bool { Size::from_str(v).is_ok() }

  fn from_str(v: &str) -> Result<Size> {
    match v {
      "major" => Ok(Size::Major),
      "minor" => Ok(Size::Minor),
      "patch" => Ok(Size::Patch),
      "none" => Ok(Size::None),
      "fail" => Ok(Size::Fail),
      other => versio_err!("Unknown size: {}", other)
    }
  }

  fn parts(v: &str) -> Result<[u32; 3]> {
    let parts: Vec<_> = v.split('.').map(|p| p.parse()).collect::<std::result::Result<_, _>>()?;
    if parts.len() != 3 {
      return versio_err!("Not a 3-part version: {}", v);
    }
    Ok([parts[0], parts[1], parts[2]])
  }

  pub fn less_than(v1: &str, v2: &str) -> Result<bool> {
    let p1 = Size::parts(v1)?;
    let p2 = Size::parts(v2)?;

    Ok(p1[0] < p2[0] || (p1[0] == p2[0] && (p1[1] < p2[1] || (p1[1] == p2[1] && p1[2] < p2[2]))))
  }

  pub fn apply(self, v: &str) -> Result<String> {
    let parts = Size::parts(v)?;

    let newv = match self {
      Size::Major => format!("{}.{}.{}", parts[0] + 1, 0, 0),
      Size::Minor => format!("{}.{}.{}", parts[0], parts[1] + 1, 0),
      Size::Patch => format!("{}.{}.{}", parts[0], parts[1], parts[2] + 1),
      Size::None => format!("{}.{}.{}", parts[0], parts[1], parts[2]),
      Size::Fail => return versio_err!("'fail' size encountered.")
    };

    Ok(newv)
  }
}

impl fmt::Display for Size {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      Size::Major => write!(f, "major"),
      Size::Minor => write!(f, "minor"),
      Size::Patch => write!(f, "patch"),
      Size::None => write!(f, "none"),
      Size::Fail => write!(f, "fail")
    }
  }
}

impl PartialOrd for Size {
  fn partial_cmp(&self, other: &Size) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Size {
  fn cmp(&self, other: &Size) -> Ordering {
    match self {
      Size::Fail => match other {
        Size::Fail => Ordering::Equal,
        _ => Ordering::Greater
      },
      Size::Major => match other {
        Size::Fail => Ordering::Less,
        Size::Major => Ordering::Equal,
        _ => Ordering::Greater
      },
      Size::Minor => match other {
        Size::Major | Size::Fail => Ordering::Less,
        Size::Minor => Ordering::Equal,
        _ => Ordering::Greater
      },
      Size::Patch => match other {
        Size::None => Ordering::Greater,
        Size::Patch => Ordering::Equal,
        _ => Ordering::Less
      },
      Size::None => match other {
        Size::None => Ordering::Equal,
        _ => Ordering::Less
      }
    }
  }
}

fn deserialize_sizes<'de, D: Deserializer<'de>>(desr: D) -> std::result::Result<HashMap<String, Size>, D::Error> {
  struct MapVisitor;

  impl<'de> Visitor<'de> for MapVisitor {
    type Value = HashMap<String, Size>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("a list of sizes") }

    fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
    where
      M: MapAccess<'de>
    {
      let mut result = HashMap::new();
      let mut using_angular = false;

      while let Some(val) = map.next_key::<String>()? {
        match val.as_str() {
          val if Size::is_size(val) => {
            let size = Size::from_str(val).unwrap();
            let keys: Vec<String> = map.next_value()?;
            for key in keys {
              if result.contains_key(&key) {
                return Err(de::Error::custom(format!("Duplicated kind \"{}\".", key)));
              }
              result.insert(key, size);
            }
          }
          "use_angular" => {
            using_angular = map.next_value()?;
          }
          _ => return Err(de::Error::custom(format!("Unrecognized sizes key \"{}\".", val)))
        }
      }

      // Based on the angular standard:
      // https://github.com/angular/angular.js/blob/master/DEVELOPERS.md#-git-commit-guidelines
      if using_angular {
        insert_if_missing(&mut result, "feat", Size::Minor);
        insert_if_missing(&mut result, "fix", Size::Patch);
        insert_if_missing(&mut result, "docs", Size::None);
        insert_if_missing(&mut result, "style", Size::None);
        insert_if_missing(&mut result, "refactor", Size::None);
        insert_if_missing(&mut result, "perf", Size::None);
        insert_if_missing(&mut result, "test", Size::None);
        insert_if_missing(&mut result, "chore", Size::None);
        insert_if_missing(&mut result, "build", Size::None);
      }

      Ok(result)
    }
  }

  desr.deserialize_map(MapVisitor)
}

fn insert_if_missing(result: &mut HashMap<String, Size>, key: &str, val: Size) {
  if !result.contains_key(key) {
    result.insert(key.to_string(), val);
  }
}

fn match_opts() -> MatchOptions { MatchOptions { require_literal_separator: true, ..Default::default() } }

fn absolutize_pattern<'a>(cover: &'a str, root_dir: &Path) -> Cow<'a, str> {
  let cover = Path::new(cover);
  if !cover.has_root() {
    Cow::Owned(root_dir.join(cover).to_string_lossy().into_owned())
  } else {
    Cow::Borrowed(cover.to_str().unwrap())
  }
}

fn construct_change_log_html(cl: &ChangeLog) -> Result<String> {
  let mut output = String::new();
  output.push_str("<html>\n");
  output.push_str("<body>\n");

  output.push_str("<ul>\n");
  for (pr, size) in cl.entries() {
    if !pr.commits().iter().any(|c| c.included()) {
      continue;
    }
    if pr.number() == 0 {
      // "PR zero" is the top-level set of commits.
      output.push_str(&format!("  <li>Other commits : {} </li>\n", size));
    } else {
      output.push_str(&format!("  <li>PR {} : {} </li>\n", pr.number(), size));
    }
    output.push_str("  <ul>\n");
    for c /* (oid, msg, size, appl, dup) */ in pr.commits().iter().filter(|c| c.included()) {
      let symbol = if c.duplicate() {
        "(dup) "
      } else if c.applies() {
        ""
      } else {
        "(not appl) "
      };
      output.push_str(&format!("    <li>{}commit {} ({}) : {}</li>\n", symbol, &c.oid()[.. 7], c.size(), c.message()));
    }
    output.push_str("  </ul>\n");
  }
  output.push_str("</ul>\n");

  output.push_str("</body>\n");
  output.push_str("</html>\n");

  Ok(output)
}

pub struct NewTags {
  pending_commit: bool,
  tags_for_new_commit: Vec<String>,
  changed_tags: HashMap<String, String>
}

impl Default for NewTags {
  fn default() -> NewTags { NewTags::new() }
}

impl NewTags {
  pub fn new() -> NewTags {
    NewTags { tags_for_new_commit: Vec::new(), changed_tags: HashMap::new(), pending_commit: false }
  }

  pub fn should_commit(&self) -> bool { self.pending_commit }

  pub fn flag_commit(&mut self) { self.pending_commit = true; }
  pub fn add_tag(&mut self, tag: String) { self.tags_for_new_commit.push(tag) }

  pub fn change_tag(&mut self, tag: String, commit: &str) {
    self.changed_tags.insert(tag, commit.to_string());
  }

  pub fn tags_for_new_commit(&self) -> &[String] { &self.tags_for_new_commit }
  pub fn changed_tags(&self) -> &HashMap<String, String> { &self.changed_tags }
}

#[cfg(test)]
mod test {
  use super::{find_reg_data, ConfigFile, Size, Project, Location, FileLocation, Picker, JsonPicker};
  use crate::source::NamedData;
  use crate::scan::parts::Part;

  #[test]
  fn test_scan() {
    let data = r#"
projects:
  - name: everything
    id: 1
    includes: ["**/*"]
    located:
      file: "toplevel.json"
      json: "version"

  - name: project1
    id: 2
    includes: ["project1/**/*"]
    located:
      file: "project1/Cargo.toml"
      toml: "version"

  - name: "combined a and b"
    id: 3
    includes: ["nested/project_a/**/*", "nested/project_b/**/*"]
    located:
      file: "nested/version.txt"
      pattern: "v([0-9]+\\.[0-9]+\\.[0-9]+) .*"

  - name: "build image"
    id: 4
    depends: [2, 3]
    located:
      file: "build/VERSION""#;

    let config = ConfigFile::read(data).unwrap();

    assert_eq!(config.projects[0].id, 1);
    assert_eq!("line", config.projects[2].located.picker().picker_type());
  }

  #[test]
  fn test_validate() {
    let config = r#"
projects:
  - name: p1
    id: 1
    includes: ["**/*"]
    located: { file: f1 }

  - name: project1
    id: 1
    includes: ["**/*"]
    located: { file: f2 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_names() {
    let config = r#"
projects:
  - name: p1
    id: 1
    includes: ["**/*"]
    located: { file: f1 }

  - name: p1
    id: 2
    includes: ["**/*"]
    located: { file: f2 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_illegal_prefix() {
    let config = r#"
projects:
  - name: p1
    id: 1
    tag_prefix: "ixth*&o"
    includes: ["**/*"]
    located: { file: f1 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_unascii_prefix() {
    let config = r#"
projects:
  - name: p1
    id: 1
    tag_prefix: "ixthïo"
    includes: ["**/*"]
    located: { file: f1 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_prefix() {
    let config = r#"
projects:
  - name: p1
    id: 1
    tag_prefix: proj
    includes: ["**/*"]
    located: { file: f1 }

  - name: p2
    id: 2
    tag_prefix: proj
    includes: ["**/*"]
    located: { file: f2 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_ok() {
    let config = r#"
projects:
  - name: p1
    id: 1
    tag_prefix: "_proj1-abc"
    includes: ["**/*"]
    located: { file: f1 }

  - name: p2
    id: 2
    tag_prefix: proj2
    includes: ["**/*"]
    located: { file: f2 }
    "#;

    assert!(ConfigFile::read(config).is_ok());
  }

  #[test]
  fn test_find_reg() {
    let data = r#"
This is text.
Current rev is "v1.2.3" because it is."#;

    let marked_data = find_reg_data(NamedData::new(None, data.to_string()), "v(\\d+\\.\\d+\\.\\d+)").unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(32, marked_data.start());
  }

  #[test]
  fn test_sizes() {
    let config = r#"
projects: []
sizes:
  major: [ break ]
  minor: [ feat ]
  patch: [ fix, "-" ]
  none: [ none ]
"#;

    let config = ConfigFile::read(config).unwrap();
    assert_eq!(&Size::Minor, config.sizes.get("feat").unwrap());
    assert_eq!(&Size::Major, config.sizes.get("break").unwrap());
    assert_eq!(&Size::Patch, config.sizes.get("fix").unwrap());
    assert_eq!(&Size::Patch, config.sizes.get("-").unwrap());
    assert_eq!(&Size::None, config.sizes.get("none").unwrap());
  }

  #[test]
  fn test_sizes_dup() {
    let config = r#"
projects: []
sizes:
  major: [ break, feat ]
  minor: [ feat ]
  patch: [ fix, "-" ]
  none: [ none ]
"#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_include_w_root() {
    let proj = Project {
      name: "test".into(),
      id: 1,
      root: Some("base".into()),
      includes: vec!["**/*".into()],
      excludes: Vec::new(),
      depends: Vec::new(),
      change_log: None,
      located: Location::File(
        FileLocation {
          file: "package.json".into(),
          picker: Picker::Json(JsonPicker { json: vec![Part::Map("version".into())] }),
        }
      ),
      tag_prefix: None
    };

    assert!(proj.does_cover("base/somefile.txt").unwrap());
    assert!(!proj.does_cover("outerfile.txt").unwrap());
  }

  #[test]
  fn test_exclude_w_root() {
    let proj = Project {
      name: "test".into(),
      id: 1,
      root: Some("base".into()),
      includes: vec!["**/*".into()],
      excludes: vec!["internal/**/*".into()],
      depends: Vec::new(),
      change_log: None,
      located: Location::File(
        FileLocation {
          file: "package.json".into(),
          picker: Picker::Json(JsonPicker { json: vec![Part::Map("version".into())] }),
        }
      ),
      tag_prefix: None
    };

    assert!(!proj.does_cover("base/internal/infile.txt").unwrap());
  }

  #[test]
  fn test_excludes_check() {
    let proj = Project {
      name: "test".into(),
      id: 1,
      root: Some("base".into()),
      includes: vec![],
      excludes: vec!["internal/**/*".into()],
      depends: Vec::new(),
      change_log: None,
      located: Location::File(
        FileLocation {
          file: "package.json".into(),
          picker: Picker::Json(JsonPicker { json: vec![Part::Map("version".into())] }),
        }
      ),
      tag_prefix: None
    };

    assert!(proj.check_excludes().is_err());
  }
}
