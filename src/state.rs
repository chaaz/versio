//! The mechanisms used to read and write state, both current and historical.

use crate::config::ProjectId;
use crate::error::Result;
use crate::git::{Repo, Slice};
use crate::mark::{NamedData, Picker};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::fs::OpenOptions;
use std::io::Write;

pub trait StateRead {
  /// Find the commit hash it's reading from; returns `None` if it is located at the current state.
  fn commit_oid(&self) -> Option<String>;

  fn has_file(&self, path: &Path) -> Result<bool>;
  fn read_file(&self, path: &Path) -> Result<String>;
  fn latest_tag(&self, prefix: &str) -> Option<&String>;
}

pub struct CurrentState {
  root: PathBuf,
  tags: OldTags
}

impl StateRead for CurrentState {
  fn commit_oid(&self) -> Option<String> { None }
  fn has_file(&self, path: &Path) -> Result<bool> { Ok(self.root.join(path).exists()) }
  fn read_file(&self, path: &Path) -> Result<String> { Ok(std::fs::read_to_string(&self.root.join(path))?) }
  fn latest_tag(&self, prefix: &str) -> Option<&String> { self.tags.latest(prefix) }
}

impl CurrentState {
  pub fn new(root: PathBuf, tags: OldTags) -> CurrentState { CurrentState { root, tags } }

  pub fn open<P: AsRef<Path>>(dir: P, tags: OldTags) -> Result<CurrentState> {
    Ok(CurrentState { root: Repo::root_dir(dir)?, tags })
  }

  pub fn slice<'r>(&self, spec: String, repo: &'r Repo) -> Result<PrevState<'r>> {
    let commit_oid = repo.revparse_oid(&spec)?;
    let old_tags = self.tags.slice_earlier(&commit_oid)?;
    Ok(PrevState::new(repo.slice(spec), commit_oid, old_tags))
  }
}

pub struct PrevState<'r> {
  slice: Slice<'r>,
  commit_oid: String,
  tags: OldTags
}

impl<'r> StateRead for PrevState<'r> {
  fn commit_oid(&self) -> Option<String> { Some(self.commit_oid.clone()) }
  fn has_file(&self, path: &Path) -> Result<bool> { self.has(path) }
  fn read_file(&self, path: &Path) -> Result<String> { PrevState::read(&self.slice, path) }
  fn latest_tag(&self, prefix: &str) -> Option<&String> { self.tags.latest(prefix) }
}

impl<'r> PrevState<'r> {
  fn new(slice: Slice<'r>, commit_oid: String, tags: OldTags) -> PrevState { PrevState { slice, commit_oid, tags } }

  pub fn sliced(slice: Slice<'r>, tags: OldTags) -> Result<PrevState> {
    let commit_oid = slice.repo().revparse_oid(slice.refspec())?;
    Ok(PrevState::new(slice, commit_oid, tags))
  }

  pub fn slice(&self, spec: String) -> Result<PrevState<'r>> {
    let commit_oid = self.slice.repo().revparse_oid(self.slice.refspec())?;
    let old_tags = self.tags.slice_earlier(&commit_oid)?;
    Ok(PrevState::new(self.slice.slice(spec), commit_oid, old_tags))
  }

  fn has(&self, path: &Path) -> Result<bool> { self.slice.has_blob(path) }

  pub fn read<P: AsRef<Path>>(slice: &Slice, path: P) -> Result<String> {
    let blob = slice.blob(path.as_ref())?;
    let cont: &str = std::str::from_utf8(blob.content())?;
    Ok(cont.to_string())
  }

  // pub fn line_commits(&self) -> Result<Vec<CommitData>> {
  //   let base = self.slice.refspec().to_string();
  //   let head = self.repo().branch_name().to_string();
  //   line_commits(&self.repo(), head, base)
  // }
}

pub struct OldTags {
  by_prefix: HashMap<String, Vec<String>>,
  not_after: HashMap<String, HashMap<String, usize>>
}

impl OldTags {
  pub fn new(by_prefix: HashMap<String, Vec<String>>, not_after: HashMap<String, HashMap<String, usize>>) -> OldTags {
    OldTags { by_prefix, not_after }
  }

  fn latest(&self, prefix: &str) -> Option<&String> { self.by_prefix.get(prefix).and_then(|p| p.first()) }

  // /// Get the latest string that doesn't come after the given boundry oid
  // fn not_after(&self, prefix: &str, boundry: &str) -> Option<&String> {
  //   self.not_after.get(boundry).and_then(|m| m.get(prefix)).map(|i| &self.by_prefix[prefix][*i])
  // }

  /// Construct a tags index for an earlier commit; a `latest` call on the returned index will match the
  /// `not_after(new_oid)` on this index.
  pub fn slice_earlier(&self, new_oid: &str) -> Result<OldTags> {
    let mut by_prefix = HashMap::new();
    let mut not_after = HashMap::new();

    for (pref, afts) in &self.not_after {
      let ind: usize = *afts.get(new_oid).ok_or_else(|| versio_error!("Bad new_oid {}", new_oid))?;
      let list = self.by_prefix.get(pref).ok_or_else(|| versio_error!("Illegal prefix {} oid for {}", pref, new_oid))?;
      let list = list[ind ..].to_vec();
      by_prefix.insert(pref.clone(), list);

      let new_afts =
        afts.iter().filter_map(|(oid, i)| if i >= &ind { Some((oid.clone(), i - ind)) } else { None }).collect();
      not_after.insert(pref.clone(), new_afts);
    }

    Ok(OldTags::new(by_prefix, not_after))
  }
}

pub struct StateWrite {
  writes: Vec<FileWrite>,
  proj_writes: HashSet<ProjectId>,
  tag_head: Vec<String>,
  tag_commit: HashMap<String, String>,
  tag_head_or_last: Vec<(String, ProjectId)>
}

impl Default for StateWrite {
  fn default() -> StateWrite { StateWrite::new() }
}

impl StateWrite {
  pub fn new() -> StateWrite {
    StateWrite {
      writes: Vec::new(), tag_head: Vec::new(), tag_commit: HashMap::new(), tag_head_or_last: Vec::new(),
      proj_writes: HashSet::new()
    }
  }

  pub fn write_file<C: ToString>(&mut self, file: PathBuf, content: C, proj_id: ProjectId) -> Result<()> {
    self.writes.push(FileWrite::Write { path: file, val: content.to_string() });
    self.proj_writes.insert(proj_id);
    Ok(())
  }

  pub fn append_file<C: ToString>(&mut self, file: PathBuf, content: C, proj_id: ProjectId) -> Result<()> {
    self.writes.push(FileWrite::Append { path: file, val: content.to_string() });
    self.proj_writes.insert(proj_id);
    Ok(())
  }

  pub fn update_mark<C: ToString>(&mut self, pick: PickPath, content: C, proj_id: ProjectId) -> Result<()> {
    self.writes.push(FileWrite::Update { pick, val: content.to_string() });
    self.proj_writes.insert(proj_id);
    Ok(())
  }

  pub fn tag_head<T: ToString>(&mut self, tag: T) -> Result<()> {
    self.tag_head.push(tag.to_string());
    Ok(())
  }

  pub fn tag_head_or_last<T: ToString>(&mut self, tag: T, proj: ProjectId) -> Result<()> {
    self.tag_head_or_last.push((tag.to_string(), proj));
    Ok(())
  }

  pub fn tag_commit<T: ToString, O: ToString>(&mut self, commit_oid: O, tag: T) -> Result<()> {
    self.tag_commit.insert(tag.to_string(), commit_oid.to_string());
    Ok(())
  }

  pub fn commit(&mut self, repo: &Repo, last_commits: &HashMap<ProjectId, String>) -> Result<()> {
    for write in &self.writes {
      write.write()?;
    }
    let did_write = !self.writes.is_empty();
    self.writes.clear();

    if did_write {
      repo.commit()?;
    }

    for tag in &self.tag_head {
      repo.update_tag_head(tag)?;
    }
    self.tag_head.clear();

    for (tag, proj_id) in &self.tag_head_or_last {
      if self.proj_writes.contains(&proj_id) {
        repo.update_tag_head(tag)?;
      } else if let Some(oid) = last_commits.get(proj_id) {
        repo.update_tag(tag, oid)?;
      } else {
        println!("Latest commit for project {} unknown: tagging head.", proj_id);
        repo.update_tag_head(tag)?;
      }
    }
    self.tag_head_or_last.clear();

    for (tag, oid) in &self.tag_commit {
      repo.update_tag(tag, oid)?;
    }
    self.tag_commit.clear();

    Ok(())
  }
}

enum FileWrite {
  Write { path: PathBuf, val: String },
  Append { path: PathBuf, val: String },
  Update { pick: PickPath, val: String }
}

impl FileWrite {
  pub fn write(&self) -> Result<()> {
    match self {
      FileWrite::Write { path, val } => Ok(std::fs::write(path, &val)?),
      FileWrite::Append { path, val } => {
        let mut file = OpenOptions::new().append(true).open(path)?;
        Ok(file.write_all(val.as_bytes())?)
      }
      FileWrite::Update { pick, val } => pick.write_value(val)
    }
  }
}

pub struct PickPath {
  file: PathBuf,
  picker: Picker
}

impl PickPath {
  pub fn new(file: PathBuf, picker: Picker) -> PickPath { PickPath { file, picker } }

  pub fn write_value(&self, val: &str) -> Result<()> {
    let data = std::fs::read_to_string(&self.file)?;
    let data = NamedData::new(self.file.clone(), data);

    let mut mark = self.picker.scan(data)?;
    mark.write_new_value(val)?;
    Ok(())
  }

  pub fn read_value(&self) -> Result<String> {
    let data = std::fs::read_to_string(&self.file)?;
    self.picker.find(&data).map(|m| m.into_value())
  }
}
