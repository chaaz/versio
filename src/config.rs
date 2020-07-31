//! The configuration and top-level commands for Versio.

use crate::either::{IterEither2 as E2, IterEither3 as E3};
use crate::analyze::{analyze, Analysis, AnnotatedMark};
use crate::error::Result;
use crate::git::{CommitData, FullPr, Repo};
use crate::github::{changes, line_commits, Changes};
use crate::state::OldTags;
use crate::scan::parts::{deserialize_parts, Part};
use crate::scan::{JsonScanner, Scanner, TomlScanner, XmlScanner, YamlScanner};
use crate::state::ReadState;
use crate::mark::{Mark, MarkedData, NamedData};
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
use std::marker::PhantomData;
use std::iter;
use std::convert::identity;

type ProjectId = u32;

pub struct Config<S: StateRead> {
  state: S,
  file: ConfigFile
}

impl Config<CurrentState> {
  pub fn prev_tag(&self) -> &str { self.file.prev_tag() }

  pub fn find_old_tags(&self, repo: &Repo) -> Result<OldTags> {
    let mut by_prefix_id = HashMap::new();  // Map<prefix, Map<oid, Vec<tag>>>

    for tag_prefix in self.file.projects().map(|p| p.tag_prefix()).filter_map(identity) {
      let fnmatch = if tag_prefix.is_empty() {
        // TODO: this fnmatch pattern doesn't seem right
        "v[[digit]]*.[[digit]]*.[[digit]]*"
      } else {
        // tag_prefix must be alphanum + '-', so no escaping necessary
        // TODO: this fnmatch pattern doesn't seem right
        &format!("{}-v[[digit]]*.[[digit]]*.[[digit]]*", tag_prefix)
      };
      for tag in repo.tag_names(Some(fnmatch))?.iter().filter_map(identity) {
        let hash = repo.revparse_oid(&format!("{}^{{}}", tag))?;
        let by_id = by_prefix_id.entry(tag_prefix.to_string()).or_insert(HashMap::new());
        
        // TODO: if adding to non-empty list, sort by tag timestamp (make these annotated and use
        // `Tag.tagger().when()` ?), latest first
        by_id.entry(hash).or_insert(Vec::new()).push(tag.to_string());
      }
    }

    let mut by_prefix = HashMap::new();
    let mut not_after = HashMap::new();
    let mut not_after_walk = HashMap::new();
    for commit_oid in repo.walk_head_to(self.prev_tag())? {
      let commit_oid = commit_oid?;
      for (prefix, by_id) in by_prefix_id.into_iter() {
        let not_after_walk = not_after_walk.entry(prefix.clone()).or_insert(Vec::new());
        not_after_walk.push(commit_oid.clone());
        if let Some(tags) = by_id.remove(&commit_oid) {
          let old_tags = by_prefix.entry(prefix.clone()).or_insert(Vec::new());
          let best_ind = old_tags.len();
          old_tags.extend_from_slice(&tags);
          let not_after_by_oid = not_after.entry(prefix).or_insert(HashMap::new());
          for later_commit_oid in not_after_walk.drain(..) {
            not_after_by_oid.insert(later_commit_oid, best_ind);
          }
        }
      }
    }

    Ok(OldTags::new(by_prefix, not_after))
  }
}

impl<'s> Config<PrevState<'r>> {
  fn slice(&self, spec: String) -> Result<Config<PrevState<'r>>> { Config::from_state(self.state.slice(spec)) }
}

impl<S: StateRead> Config<S> {
  pub fn from_state(state: S) -> Result<Config<S>> {
    let file = ConfigFile::load(&state)?;
    Ok(Config { state, file })
  }

  // pub fn has_config_file(state: S) -> Result<bool> { state.has(CONFIG_FILENAME.as_ref()) }
  // pub fn state(&self) -> &S { &self.state }
  // pub fn file(&self) -> &ConfigFile { &self.file }
  // pub fn get_project(&self, id: ProjectId) -> Option<&Project> { self.file.projects.iter().find(|p| p.id == id) }

  fn find_unique(&self, name: &str) -> Result<ProjectId> {
    let mut iter = self.file.projects.iter().filter(|p| p.name.contains(name)).map(|p| p.id);
    let id = iter.next().ok_or_else(|| versio_error!("No project named {}", name))?;
    if iter.next().is_some() {
      return versio_err!("Multiple projects with name {}", name);
    }
    Ok(id)
  }

  pub fn annotate(&self) -> Result<Vec<AnnotatedMark>> {
    self.file.projects.iter().map(|p| p.annotate(&self.state)).collect()
  }
}

pub struct Plan {
  incrs: HashMap<ProjectId, (Size, Option<String>, ChangeLog)>, // proj ID, incr size, last_commit, change log
  ineffective: Vec<LoggedPr>                                     // PRs that didn't apply to any project
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

pub struct PlanBuilder<'s, C: StateRead> {
  on_pr_sizes: HashMap<ProjectId, LoggedPr>,
  on_ineffective: Option<LoggedPr>,
  on_commit: Option<CommitData>,
  prev: (Slice<'s>, ConfigFile),
  current: ConfigFile,
  incrs: HashMap<ProjectId, (Size, Option<String>, ChangeLog)>, // proj ID, incr size, last_commit, change log
  ineffective: Vec<LoggedPr>                                    // PRs that didn't apply to any project
}

impl<'s, C: StateRead> PlanBuilder<'s, C> {
  fn new(prev: Slice<'s>, current: &'s Config<C>) -> PlanBuilder<'s, C> {
    let prev_file = ConfigFile::from_slice(&prev)?;
    PlanBuilder {
      on_pr_sizes: HashMap::new(),
      on_ineffective: None,
      on_commit: None,
      prev: (prev, pref_file),
      current,
      incrs: HashMap::new(),
      ineffective: Vec::new()
    }
  }

  pub fn start_pr(&mut self, pr: &FullPr) -> Result<()> {
    self.on_pr_sizes = self.current.file.projects.iter().map(|p| (p.id(), LoggedPr::capture(pr))).collect();
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
        let size = cur_project.size(&self.current.file.sizes, &kind)?;
        logged_pr.commits.push(LoggedCommit::new(id.clone(), summary.clone(), size));
      }
    }

    Ok(())
  }

  pub fn finish_commit(&mut self) -> Result<()> { Ok(()) }

  pub fn start_file(&mut self, path: &str) -> Result<()> {
    let commit = self.on_commit.as_ref().ok_or_else(|| versio_error!("Not on a commit"))?;
    let commit_id = commit.id();

    for prev_project in &self.prev.1.projects {
      if let Some(logged_pr) = self.on_pr_sizes.get_mut(&prev_project.id) {
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
  }
}

pub struct LastCommitBuilder<'s, C: StateRead> {
  on_line_commit: Option<String>,
  last_commits: HashMap<ProjectId, String>,
  prev: (Slice<'s>, ConfigFile),
  current: &'s Config<C>
}

impl<'s, C: StateRead> LastCommitBuilder<'s, C> {
  fn new(prev: Slice<'s>, current: &'s Config<C>) -> LastCommitBuilder<'s, C> {
    let file = ConfigFile::from_slice(&prev)?;
    LastCommitBuilder { on_line_commit: None, last_commits: HashMap::new(), prev: (prev, file), current }
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

    for prev_project in &self.prev.1.projects {
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
  }
}

#[derive(Deserialize, Debug)]
pub struct ConfigFile {
  #[serde(default)]
  options: Options,
  projects: Vec<Project>,
  #[serde(deserialize_with = "deserialize_sizes", default)]
  sizes: HashMap<String, Size>
}

impl ConfigFile {
  pub fn from_state<S: StateRead>(state: &S) -> Result<ConfigFile> {
    ConfigFile::read(state.read_file(CONFIG_FILENAME.as_ref())?.into())
  }

  pub fn from_slice(slice: &Slice) -> Result<ConfigFile> {
    let root = slice.repo().working_dir()?;
    ConfigFile::read(PrevState::read(slice, root, CONFIG_FILENAME)?)
  }

  pub fn empty() -> ConfigFile {
    ConfigFile { options: Default::default(), projects: Vec::new(), sizes: HashMap::new() }
  }

  pub fn read(data: &str) -> Result<ConfigFile> {
    let file: ConfigFile = serde_yaml::from_str(data)?;
    file.validate()?;
    Ok(file)
  }

  pub fn prev_tag(&self) -> &str { self.options.prev_tag() }
  pub fn projects(&self) -> &[Project] { &self.projects }

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

#[derive(Deserialize, Debug)]
struct Options {
  prev_tag: String
}

impl Default for Options {
  fn default() -> Options { Options { prev_tag: "versio-prev".into() } }
}

impl Options {
  pub fn prev_tag(&self) -> &str { &self.prev_tag }
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
  fn annotate<S: StateRead>(&self, old_tags: &OldTags, state: &S) -> Result<AnnotatedMark> {
    Ok(AnnotatedMark::new(self.id, self.name.clone(), self.get_value(old_tags, state)?))
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

  pub fn write_change_log<S: StateRead>(&self, cl: &ChangeLog, root: &Path) -> Result<Option<String>> {
    // TODO: only write change log if any commits are found, else return `None`

    if let Some(cl_path: &str) = self.change_log().as_ref() {
      // let log_path = root.join(cl_path.deref());
      let log_path = root.join(cl_path.deref());
      std::fs::write(&log_path, construct_change_log_html(cl)?)?;
      Ok(Some(cl_path.to_string()))
    } else {
      Ok(None)
    }
  }

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

    self
      .includes
      .iter()
      .try_fold(false, |val, cov| Ok(val || Pattern::new(&self.rooted_pattern(cov))?.matches_with(path, match_opts())))
  }

  fn check<S: StateRead>(&self, old_tags: &OldTags, state: &S) -> Result<()> {
    // Check that we can find the given mark.
    self.get_value(old_tags, state)?;

    self.check_excludes()?;
    let root_dir = state.root_dir();

    // Check that each pattern includes at least one file.
    for cov in &self.includes {
      let pattern = self.rooted_pattern(cov);
      let cover = absolutize_pattern(&pattern, root_dir);
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

  fn get_value<S: StateRead>(&self, read: &S) -> Result<String> {
    self.located.read_value(read, &self.root, self.tag_prefix())
  }

  fn set_value(&self, write: &mut StateWrite, val: &str) -> Result<()> {
    self.located.write_value(write, &self.root, val)
    self.forward_tag(write, val)
  }

  fn forward_tag(&self, write: &mut StateWrite, val: &str) -> Result<()> {
    if let Some(tag_prefix) = &self.tag_prefix {
      let tag = if tag_prefix.is_empty() {
        format!("v{}", val)
      } else {
        format!("{}-v{}", tag_prefix, val)
      };
      write.tag_head_or_last(tag, self.id)?;
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
  pub fn write_value(&self, write: &mut StateWrite, root: &Option<String>, val: &str) -> Result<()> {
    match self {
      Location::File(l) => l.write_value(write, root, val),
      Location::Tag(l) => Ok(())
    }
  }

  pub fn read_value<S: StateRead>(&self, read: &S, root: &Option<String>, pref: &Option<String>) -> Result<String> {
    match self {
      Location::File(l) => l.read_value(read, root),
      Location::Tag(l) => l.read_value(read, pref)
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
struct TagLocation {
  tags: TagSpec
}

impl TagLocation {
  fn read_value<S: StateRead>(&self, read: &S, prefix: &Option<String>) -> Result<String> {
    // TODO: restructure types to make it impossible to have a tags project w/out a tag_prefix
    let prefix = prefix.as_ref().ok_or_else(|| versio_error!("No tag prefix for tag location."))?;

    // TODO: use TagSpec default instead of Err
    Ok(read.latest_tag(prefix).ok_or_else(|| versio_error!("No tag found for {}", prefix))?.clone())
  }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum TagSpec {
  DefaultTag(DefaultTagSpec),
  MajorTag(MajorTagSpec)
}

#[derive(Deserialize, Debug)]
struct DefaultTagSpec {
  default: String
}

#[derive(Deserialize, Debug)]
struct MajorTagSpec {
  major: u32
}

#[derive(Clone, Deserialize, Debug)]
struct FileLocation {
  file: String,
  #[serde(flatten)]
  picker: Picker
}

impl FileLocation {
  pub fn write_value(&self, write: &mut StateWrite, root: &Option<String>, val: &str) -> Result<()> {
    let file = self.rooted(root);
    write.update_mark(PickPath::new(file, self.picker.clone()), val.to_string())
  }

  pub fn read_value<S: StateRead>(&self, read: &S, root: &Option<String>) -> Result<String> {
    let file = self.rooted(root);
    let data: String = state.read_file(&file)?
    self.picker.find(&data).map(|m| m.into_value())
  }

  pub fn rooted(&self, root: &Option<String>) -> PathBuf {
    match root {
      Some(root) => PathBuf::from(root).join(&self.file),
      None => PathBuf::from(&self.file)
    }
  }
}

#[derive(Clone, Deserialize, Debug)]
#[serde(untagged)]
pub enum Picker {
  Json(ScanningPicker<JsonScanner>),
  Yaml(ScanningPicker<YamlScanner>),
  Toml(ScanningPicker<TomlScanner>),
  Xml(ScanningPicker<XmlScanner>),
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

  pub fn scan(&self, data: NamedData) -> Result<MarkedData> {
    match self {
      Picker::Json(p) => p.scan(data),
      Picker::Yaml(p) => p.scan(data),
      Picker::Toml(p) => p.scan(data),
      Picker::Xml(p) => p.scan(data),
      Picker::Line(p) => p.scan(data),
      Picker::File(p) => p.scan(data)
    }
  }

  pub fn find(&self, data: &str) -> Result<Mark> {
    match self {
      Picker::Json(p) => p.find(data),
      Picker::Yaml(p) => p.find(data),
      Picker::Toml(p) => p.find(data),
      Picker::Xml(p) => p.find(data),
      Picker::Line(p) => p.find(data),
      Picker::File(p) => p.find(data)
    }
  }
}

#[derive(Deserialize)]
struct ScanningPicker<T: Scanner> {
  #[serde(deserialize_with = "deserialize_parts")]
  parts: Vec<Part>,
  _scan: PhantomData<T>
}

impl<T: Scanner> Clone for ScanningPicker<T> {
  fn clone(&self) -> ScanningPicker<T> {
    ScanningPicker { parts: self.parts.clone(), _scan: PhantomData }
  }
}

impl<T: Scanner> fmt::Debug for ScanningPicker<T> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "ScanningPicker {{ {:?} }}", self.parts)
  }
}

impl<T: Scanner> ScanningPicker<T> {
  pub fn find(&self, data: &str) -> Result<Mark> { T::build(self.parts.clone()).find(data) }
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { T::build(self.parts.clone()).scan(data) }
}

#[derive(Clone, Deserialize, Debug)]
pub struct LinePicker {
  pattern: String
}

impl LinePicker {
  pub fn find(&self, data: &str) -> Result<Mark> { LinePicker::find_reg_data(data, &self.pattern) }
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { LinePicker::scan_reg_data(data, &self.pattern) }

  fn find_reg_data(data: &str, pattern: &str) -> Result<Mark> {
    let pattern = Regex::new(pattern)?;
    let found = pattern.captures(data).ok_or_else(|| versio_error!("No match for {}", pattern))?;
    let item = found.get(1).ok_or_else(|| versio_error!("No capture group in {}.", pattern))?;
    let value = item.as_str().to_string();
    let index = item.start();
    Ok(Mark::make(value, index)?)
  }

  fn scan_reg_data(data: NamedData, pattern: &str) -> Result<MarkedData> {
    let mark = LinePicker::find_reg_data(data.data(), pattern)?;
    Ok(data.mark(mark))
  }
}

#[derive(Clone, Deserialize, Debug)]
pub struct FilePicker {}

impl FilePicker {
  pub fn find(&self, data: &str) -> Result<Mark> {
    let value = data.trim_end().to_string();
    Ok(Mark::make(value, 0)?)
  }

  pub fn scan(&self, data: NamedData) -> Result<MarkedData> {
    let mark = self.find(data.data())?;
    Ok(data.mark(mark))
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

// pub struct ShowFormat {
//   pub wide: bool,
//   pub version_only: bool
// }
// 
// impl ShowFormat {
//   pub fn new(wide: bool, version_only: bool) -> ShowFormat { ShowFormat { wide, version_only } }
// }

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

// pub struct NewTags {
//   pending_commit: bool,
//   tags_for_new_commit: Vec<String>,
//   changed_tags: HashMap<String, String>
// }
// 
// impl Default for NewTags {
//   fn default() -> NewTags { NewTags::new() }
// }
// 
// impl NewTags {
//   pub fn new() -> NewTags {
//     NewTags { tags_for_new_commit: Vec::new(), changed_tags: HashMap::new(), pending_commit: false }
//   }
// 
//   pub fn should_commit(&self) -> bool { self.pending_commit }
//   pub fn tags_for_new_commit(&self) -> &[String] { &self.tags_for_new_commit }
//   pub fn changed_tags(&self) -> &HashMap<String, String> { &self.changed_tags }
//   pub fn flag_commit(&mut self) { self.pending_commit = true; }
//   pub fn add_tag(&mut self, tag: String) { self.tags_for_new_commit.push(tag) }
//   pub fn change_tag(&mut self, tag: String, commit: &str) { self.changed_tags.insert(tag, commit.to_string()); }
// }

#[cfg(test)]
mod test {
  use super::{ConfigFile, FileLocation, ScanningPicker, LinePicker, Location, Picker, Project, Size};
  use crate::scan::parts::Part;
  use std::marker::PhantomData;

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

    let mark = LinePicker::find_reg_data(data, "v(\\d+\\.\\d+\\.\\d+)").unwrap();
    assert_eq!("1.2.3", mark.value());
    assert_eq!(32, mark.start());
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
      located: Location::File(FileLocation {
        file: "package.json".into(),
        picker: Picker::Json(ScanningPicker { _scan: PhantomData, parts: vec![Part::Map("version".into())] })
      }),
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
      located: Location::File(FileLocation {
        file: "package.json".into(),
        picker: Picker::Json(ScanningPicker { _scan: PhantomData, parts: vec![Part::Map("version".into())] })
      }),
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
      located: Location::File(FileLocation {
        file: "package.json".into(),
        picker: Picker::Json(ScanningPicker { _scan: PhantomData, parts: vec![Part::Map("version".into())] })
      }),
      tag_prefix: None
    };

    assert!(proj.check_excludes().is_err());
  }
}
