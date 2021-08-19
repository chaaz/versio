//! The mechanisms used to read and write state, both current and historical.

use crate::config::{HookSet, ProjectId};
use crate::errors::{Result, ResultExt as _};
use crate::git::{FromTagBuf, Repo, Slice};
use crate::mark::{NamedData, Picker};
use log::{trace, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::mem::take;
use std::path::{Path, PathBuf};
use path_slash::{PathExt as _, PathBufExt as _};

pub trait StateRead: FilesRead {
  fn latest_tag(&self, proj: &ProjectId) -> Option<&String>;
}

impl<S: StateRead> StateRead for &S {
  fn latest_tag(&self, proj: &ProjectId) -> Option<&String> { <S as StateRead>::latest_tag(*self, proj) }
}

pub trait FilesRead {
  fn has_file(&self, path: &Path) -> Result<bool>;
  fn read_file(&self, path: &Path) -> Result<String>;
  fn subdirs(&self, root: Option<&String>, regex: &str) -> Result<Vec<String>>;
}

impl<F: FilesRead> FilesRead for &F {
  fn has_file(&self, path: &Path) -> Result<bool> { <F as FilesRead>::has_file(*self, path) }
  fn read_file(&self, path: &Path) -> Result<String> { <F as FilesRead>::read_file(*self, path) }
  fn subdirs(&self, root: Option<&String>, regex: &str) -> Result<Vec<String>> {
    <F as FilesRead>::subdirs(*self, root, regex)
  }
}

pub struct CurrentState {
  files: CurrentFiles,
  tags: OldTags
}

impl FilesRead for CurrentState {
  fn has_file(&self, path: &Path) -> Result<bool> { self.files.has_file(path) }
  fn read_file(&self, path: &Path) -> Result<String> { self.files.read_file(path) }
  fn subdirs(&self, root: Option<&String>, regex: &str) -> Result<Vec<String>> { self.files.subdirs(root, regex) }
}

impl StateRead for CurrentState {
  fn latest_tag(&self, proj: &ProjectId) -> Option<&String> { self.tags.latest(proj) }
}

impl CurrentState {
  pub fn new(root: PathBuf, tags: OldTags) -> CurrentState { CurrentState { files: CurrentFiles::new(root), tags } }
  pub fn old_tags(&self) -> &OldTags { &self.tags }
}

pub struct CurrentFiles {
  root: PathBuf
}

impl FilesRead for CurrentFiles {
  fn has_file(&self, path: &Path) -> Result<bool> { Ok(self.root.join(path).exists()) }
  fn read_file(&self, path: &Path) -> Result<String> { Ok(std::fs::read_to_string(&self.root.join(path))?) }

  fn subdirs(&self, root: Option<&String>, regex: &str) -> Result<Vec<String>> {
    let filter = Regex::new(regex)?;
    let root = root.map(|s| s.as_str()).unwrap_or(".");
    PathBuf::from_slash(root)
      .read_dir()?
      .filter_map(|e| e.map(|e| e.file_name().into_string().ok()).transpose())
      .filter(|n| n.as_ref().map(|n| filter.is_match(n)).unwrap_or(true))
      .map(|r| r.map_err(|e| e.into()))
      .collect()
  }
}

impl CurrentFiles {
  pub fn new(root: PathBuf) -> CurrentFiles { CurrentFiles { root } }
}

pub struct PrevState<'r> {
  files: PrevFiles<'r>,
  tags: OldTags
}

impl<'r> FilesRead for PrevState<'r> {
  fn has_file(&self, path: &Path) -> Result<bool> { self.files.has_file(path) }
  fn read_file(&self, path: &Path) -> Result<String> { self.files.read_file(path) }
  fn subdirs(&self, root: Option<&String>, regex: &str) -> Result<Vec<String>> { self.files.subdirs(root, regex) }
}

impl<'r> StateRead for PrevState<'r> {
  fn latest_tag(&self, proj: &ProjectId) -> Option<&String> { self.tags.latest(proj) }
}

impl<'r> PrevState<'r> {
  pub fn new(slice: Slice<'r>, tags: OldTags) -> PrevState { PrevState { files: PrevFiles::new(slice), tags } }
}

pub struct PrevFiles<'r> {
  slice: Slice<'r>
}

impl<'r> FilesRead for PrevFiles<'r> {
  fn has_file(&self, path: &Path) -> Result<bool> { self.slice.has_blob(&path.to_slash_lossy()) }
  fn read_file(&self, path: &Path) -> Result<String> { read_from_slice(&self.slice, path) }

  fn subdirs(&self, root: Option<&String>, regex: &str) -> Result<Vec<String>> { self.slice.subdirs(root, regex) }
}

impl<'r> PrevFiles<'r> {
  pub fn from_slice(slice: Slice<'r>) -> Result<PrevFiles> { Ok(PrevFiles::new(slice)) }

  pub fn new(slice: Slice<'r>) -> PrevFiles { PrevFiles { slice } }
  pub fn slice_to(&self, spec: FromTagBuf) -> Result<PrevFiles<'r>> { PrevFiles::from_slice(self.slice.slice(spec)) }
}

#[derive(Debug)]
pub struct OldTags {
  current: HashMap<ProjectId, String>,
  prev: HashMap<ProjectId, String>
}

impl OldTags {
  pub fn new(current: HashMap<ProjectId, String>, prev: HashMap<ProjectId, String>) -> OldTags {
    OldTags { current, prev }
  }

  pub fn latest(&self, proj: &ProjectId) -> Option<&String> { self.current.get(proj) }
  pub fn current(&self) -> &HashMap<ProjectId, String> { &self.current }

  pub fn slice_to_prev(&self) -> Result<OldTags> { Ok(OldTags::new(self.prev.clone(), HashMap::new())) }
}

#[derive(Deserialize, Serialize)]
pub struct StateWrite {
  writes: Vec<FileWrite>,
  proj_writes: HashSet<ProjectId>,
  tag_head: Vec<String>,
  tag_commit: HashMap<String, String>,
  tag_head_or_last: Vec<(String, ProjectId)>,
  new_tags: HashMap<ProjectId, String>
}

impl Default for StateWrite {
  fn default() -> StateWrite { StateWrite::new() }
}

impl StateWrite {
  pub fn new() -> StateWrite {
    StateWrite {
      writes: Vec::new(),
      tag_head: Vec::new(),
      tag_commit: HashMap::new(),
      tag_head_or_last: Vec::new(),
      proj_writes: HashSet::new(),
      new_tags: HashMap::new()
    }
  }

  pub fn write_file<C: ToString>(&mut self, file: PathBuf, content: C, proj_id: &ProjectId) -> Result<()> {
    self.writes.push(FileWrite::Write { path: file, val: content.to_string() });
    self.proj_writes.insert(proj_id.clone());
    Ok(())
  }

  pub fn update_mark<C: ToString>(&mut self, pick: PickPath, content: C, proj_id: &ProjectId) -> Result<()> {
    self.writes.push(FileWrite::Update { pick, val: content.to_string() });
    self.proj_writes.insert(proj_id.clone());
    Ok(())
  }

  pub fn tag_head_or_last<T: ToString>(&mut self, vers: &str, tag: T, proj: &ProjectId) -> Result<()> {
    let tag = tag.to_string();
    trace!("head_or_last on {} tagged with {}.", proj, tag);
    self.tag_head_or_last.push((tag, proj.clone()));
    self.new_tags.insert(proj.clone(), vers.to_string());
    Ok(())
  }

  pub fn commit(&mut self, repo: &Repo, data: CommitArgs) -> Result<()> {
    for write in &self.writes {
      write.write()?;
    }
    let did_write = !self.writes.is_empty();
    self.writes.clear();

    for proj_id in &self.proj_writes {
      if let Some((root, hooks)) = data.hooks.get(proj_id) {
        hooks.execute_post_write(root)?;
      }
    }

    let me = take(self);
    let prev_tag = data.prev_tag.to_string();
    let last_commits = data.last_commits.clone();
    let old_tags = data.old_tags.clone();
    let mut commit_state = CommitState::new(me, did_write, prev_tag, last_commits, old_tags, data.advance_prev);

    if data.pause {
      let file = OpenOptions::new().create(true).write(true).truncate(true).open(".versio-paused")?;
      Ok(serde_json::to_writer(file, &commit_state)?)
    } else {
      commit_state.resume(repo)
    }
  }
}

pub struct CommitArgs<'a> {
  prev_tag: &'a str,
  last_commits: &'a HashMap<ProjectId, String>,
  old_tags: &'a HashMap<ProjectId, String>,
  advance_prev: bool,
  hooks: &'a HashMap<ProjectId, (Option<&'a String>, &'a HookSet)>,
  pause: bool
}

impl<'a> CommitArgs<'a> {
  pub fn new(
    prev_tag: &'a str, last_commits: &'a HashMap<ProjectId, String>, old_tags: &'a HashMap<ProjectId, String>,
    advance_prev: bool, hooks: &'a HashMap<ProjectId, (Option<&'a String>, &'a HookSet)>, pause: bool
  ) -> CommitArgs<'a> {
    CommitArgs { prev_tag, last_commits, old_tags, advance_prev, hooks, pause }
  }
}

fn fill_from_old(old: &HashMap<ProjectId, String>, new_tags: &mut HashMap<ProjectId, String>) {
  for (proj_id, tag) in old {
    if !new_tags.contains_key(proj_id) {
      new_tags.insert(proj_id.clone(), tag.clone());
    }
  }
}

/// A command to commit, tag, and push everything
#[derive(Deserialize, Serialize)]
pub struct CommitState {
  write: StateWrite,
  did_write: bool,
  prev_tag: String,
  last_commits: HashMap<ProjectId, String>,
  old_tags: HashMap<ProjectId, String>,
  advance_prev: bool
}

impl CommitState {
  pub fn new(
    write: StateWrite, did_write: bool, prev_tag: String, last_commits: HashMap<ProjectId, String>,
    old_tags: HashMap<ProjectId, String>, advance_prev: bool
  ) -> CommitState {
    CommitState { write, did_write, prev_tag, last_commits, old_tags, advance_prev }
  }

  pub fn resume(&mut self, repo: &Repo) -> Result<()> {
    if self.did_write {
      trace!("Wrote files, so committing.");
      repo.commit()?;
    } else {
      trace!("No files written, so not committing.");
    }

    for tag in &self.write.tag_head {
      repo.update_tag_head(tag)?;
    }
    self.write.tag_head.clear();

    for (tag, proj_id) in &self.write.tag_head_or_last {
      if self.write.proj_writes.contains(proj_id) {
        repo.update_tag_head(tag)?;
      } else if let Some(oid) = self.last_commits.get(proj_id) {
        repo.update_tag(tag, oid)?;
      } else {
        warn!("Latest commit for project {} unknown: tagging head.", proj_id);
        repo.update_tag_head(tag)?;
      }
    }
    self.write.tag_head_or_last.clear();
    self.write.proj_writes.clear();

    for (tag, oid) in &self.write.tag_commit {
      repo.update_tag(tag, oid)?;
    }
    self.write.tag_commit.clear();

    if self.advance_prev {
      fill_from_old(&self.old_tags, &mut self.write.new_tags);
      let msg = serde_json::to_string(&PrevTagMessage::new(std::mem::take(&mut self.write.new_tags)))?;
      repo.update_tag_head_anno(&self.prev_tag, &msg)?;
    }

    Ok(())
  }
}

#[derive(Deserialize, Serialize)]
pub struct PrevTagMessage {
  versions: HashMap<ProjectId, String>
}

impl Default for PrevTagMessage {
  fn default() -> PrevTagMessage { PrevTagMessage { versions: HashMap::new() } }
}

impl PrevTagMessage {
  pub fn new(versions: HashMap<ProjectId, String>) -> PrevTagMessage { PrevTagMessage { versions } }
  pub fn into_versions(self) -> HashMap<ProjectId, String> { self.versions }
}

#[derive(Deserialize, Serialize)]
enum FileWrite {
  Write { path: PathBuf, val: String },
  Update { pick: PickPath, val: String }
}

impl FileWrite {
  pub fn write(&self) -> Result<()> {
    match self {
      FileWrite::Write { path, val } => {
        Ok(std::fs::write(path, &val).chain_err(|| format!("Can't write to {}", path.to_string_lossy()))?)
      }
      // FileWrite::Append { path, val } => {
      //   let mut file = OpenOptions::new().append(true).open(path)?;
      //   Ok(file.write_all(val.as_bytes())?)
      // }
      FileWrite::Update { pick, val } => pick.write_value(val)
    }
  }
}

#[derive(Deserialize, Serialize)]
pub struct PickPath {
  file: PathBuf,
  picker: Picker
}

impl PickPath {
  pub fn new(file: PathBuf, picker: Picker) -> PickPath { PickPath { file, picker } }

  pub fn write_value(&self, val: &str) -> Result<()> {
    let data =
      std::fs::read_to_string(&self.file).chain_err(|| format!("Can't read file {}.", self.file.to_string_lossy()))?;
    let data = NamedData::new(self.file.clone(), data);
    let mut mark = self.picker.scan(data)?;
    mark.write_new_value(val)?;
    Ok(())
  }
}

pub fn read_from_slice<P: AsRef<Path>>(slice: &Slice, path: P) -> Result<String> {
  let path = path.as_ref().to_slash_lossy();
  let blob = slice.blob(&path)?;
  let cont: &str = std::str::from_utf8(blob.content()).chain_err(|| format!("Not UTF8 content: {}", path))?;
  Ok(cont.to_string())
}
