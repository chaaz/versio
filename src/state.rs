//! The mechanisms used to read and write state, both current and historical.

use crate::config::ProjectId;
use crate::error::Result;
use crate::git::{Repo, Slice};
use crate::mark::{NamedData, Picker};
use std::collections::HashMap;
use std::convert::identity;
use std::path::{Path, PathBuf};

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
  pub fn open<P: AsRef<Path>>(dir: P, tags: OldTags) -> Result<CurrentState> {
    Ok(CurrentState { root: Repo::root_dir(dir)?, tags })
  }

  pub fn slice<'r>(&self, spec: String, repo: &'r Repo) -> Result<PrevState<'r>> {
    let commit_oid = repo.revparse_oid(&spec)?;
    Ok(PrevState::new(repo.slice(spec), commit_oid, self.tags.slice_earlier(&commit_oid)?))
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
    Ok(PrevState::new(self.slice.slice(spec), commit_oid, self.tags.slice_earlier(&commit_oid)?))
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

  /// Get the latest string that doesn't come after the given boundry oid
  fn not_after(&self, prefix: &str, boundry: &str) -> Option<&String> {
    self.not_after.get(boundry).and_then(|m| m.get(prefix)).map(|i| &self.by_prefix[prefix][*i])
  }

  /// Construct a tags index for an earlier commit; a `latest` call on the returned index will match the
  /// `not_after(new_oid)` on this index.
  pub fn slice_earlier(&self, new_oid: &str) -> Result<OldTags> { unimplemented!() }
}

pub struct StateWrite {
  writes: Vec<FileWrite>,
  tag_head: Vec<String>,
  tag_commit: HashMap<String, String>,
  tag_head_or_last: Vec<(String, ProjectId)>
}

impl StateWrite {
  pub fn new() -> StateWrite {
    StateWrite { writes: Vec::new(), tag_head: Vec::new(), tag_commit: HashMap::new(), tag_head_or_last: Vec::new() }
  }

  pub fn write_file<C: ToString>(&mut self, file: PathBuf, content: C) -> Result<()> {
    self.writes.push(FileWrite::Write { path: file, val: content.to_string() });
    Ok(())
  }

  pub fn append_file<C: ToString>(&mut self, file: PathBuf, content: C) -> Result<()> {
    self.writes.push(FileWrite::Append { path: file, val: content.to_string() });
    Ok(())
  }

  pub fn update_mark<C: ToString>(&mut self, pick: PickPath, content: C) -> Result<()> {
    self.writes.push(FileWrite::Update { pick, val: content.to_string() });
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

  pub fn commit(&mut self, repo: &Repo) -> Result<()> {
    for write in &self.writes {
      write.write()?;
    }
    let did_write = !self.writes.is_empty();
    self.writes.clear();

    if did_write {
      self.perform_commit(repo)?;
    }

    for tag in &self.tag_head {
      self.update_tag_head(tag)?;
    }
    self.tag_head.clear();

    for (tag, proj_id) in &self.tag_head_or_last {
      if self.has_written(*proj_id) {
        self.update_tag_head(tag)?;
      } else {
        self.update_tag(self.latest_commit(*proj_id)?, tag)?;
      }
    }
    self.tag_head_or_last.clear();

    for (tag, oid) in &self.tag_commit {
      self.update_tag(oid, tag)?;
    }
    self.tag_commit.clear();

    Ok(())
  }

  fn perform_commit(&self, repo: &Repo) -> Result<bool> { repo.make_changes(self.new_tags()) }
  fn update_tag_head(&self, tags: &str) -> Result<()> { unimplemented!() }
  fn update_tag(&self, oid: &str, tag: &str) -> Result<()> { unimplemented!() }
  fn new_tags(&self) -> &[String] { unimplemented!() }
  fn has_written(&self, proj_id: ProjectId) -> bool { unimplemented!() }
  fn latest_commit(&self, proj_id: ProjectId) -> Result<&str> { unimplemented!() }
}

enum FileWrite {
  Write { path: PathBuf, val: String },
  Append { path: PathBuf, val: String },
  Update { pick: PickPath, val: String }
}

impl FileWrite {
  pub fn write(&self) -> Result<()> {
    match self {
      FileWrite::Write { path, val } => unimplemented!(),
      FileWrite::Append { path, val } => unimplemented!(),
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

    let mark = self.picker.scan(data)?;
    mark.write_new_value(val)?;
    Ok(())
  }

  pub fn read_value(&self) -> Result<String> {
    let data = std::fs::read_to_string(&self.file)?;
    self.picker.find(&data).map(|m| m.into_value())
  }
}

fn find_old_tags<'s, I: Iterator<Item = &'s str>>(prefixes: I, prev_tag: &str, repo: &Repo) -> Result<OldTags> {
  let mut by_prefix_id = HashMap::new(); // Map<prefix, Map<oid, Vec<tag>>>

  // for tag_prefix in self.file.projects().map(|p| p.tag_prefix()).filter_map(identity)

  for tag_prefix in prefixes {
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
  for commit_oid in repo.walk_head_to(prev_tag)? {
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
