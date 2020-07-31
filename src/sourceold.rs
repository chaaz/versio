//! Versio is a version management utility.

use crate::mark::NamedData;
use crate::error::Result;
use crate::git::{CommitData, Repo, Slice};
use crate::github::{line_commits};
use std::path::{Path, PathBuf};

pub const CONFIG_FILENAME: &str = ".versio.yaml";

pub trait Source {
  type Loaded: Into<String>;

  fn root_dir(&self) -> &Path;
  fn load(&self, rel_path: &Path) -> Result<Option<Self::Loaded>>;
  fn has(&self, rel_path: &Path) -> Result<bool>;
  fn commit_oid(&self) -> Result<Option<String>>;
}

impl<S: Source> Source for &S {
  type Loaded = S::Loaded;

  fn root_dir(&self) -> &Path { <S as Source>::root_dir(*self) }
  fn load(&self, rel_path: &Path) -> Result<Option<S::Loaded>> { <S as Source>::load(*self, rel_path) }
  fn has(&self, rel_path: &Path) -> Result<bool> { <S as Source>::has(*self, rel_path) }
  fn commit_oid(&self) -> Result<Option<String>> { <S as Source>::commit_oid(*self) }
}

pub struct CurrentSource {
  root_dir: PathBuf
}

impl CurrentSource {
  pub fn open<P: AsRef<Path>>(dir: P) -> Result<CurrentSource> { Ok(CurrentSource { root_dir: Repo::root_dir(dir)? }) }
}

impl Source for CurrentSource {
  type Loaded = NamedData;

  fn root_dir(&self) -> &Path { &self.root_dir }
  fn has(&self, rel_path: &Path) -> Result<bool> { Ok(self.root_dir.join(rel_path).exists()) }
  fn commit_oid(&self) -> Result<Option<String>> { Ok(None) }

  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>> {
    let path = self.root_dir.join(rel_path);
    if Path::exists(&path) {
      let data = std::fs::read_to_string(&path)?;
      Ok(Some(NamedData::new(path, data)))
    } else {
      Ok(None)
    }
  }

}

// pub struct PrevSource {
//   repo: Repo,
//   spec: String,
//   root_dir: PathBuf
// }
//
// impl Source for PrevSource {
//   type Loaded = String;
//
//   fn root_dir(&self) -> &Path { &self.root_dir }
//   fn has(&self, rel_path: &Path) -> Result<bool> { self.has_path(rel_path) }
//   fn load(&self, rel_path: &Path) -> Result<Option<String>> { self.load_path(rel_path).map(Some) }
// }
//
// impl PrevSource {
//   pub fn open(dir: &Path, spec: String) -> Result<PrevSource> {
//     let repo = Repo::open(dir)?;
//     let root_dir = repo.working_dir()?.to_path_buf();
//     Ok(PrevSource { repo, spec, root_dir })
//   }
//
//   pub fn slice(&self, spec: String) -> SliceSource { SliceSource::at(self.repo.slice(spec), self.root_dir.clone()) }
//
//   pub fn has_remote(&self) -> bool { self.repo.has_remote() }
//   pub fn has_path(&self, rel_path: &Path) -> Result<bool> { self.repo.slice(self.spec.clone()).has_blob(rel_path) }
//   pub fn repo(&self) -> Result<&Repo> { Ok(&self.repo) }
//   pub fn pull(&self) -> Result<()> { self.repo.pull() }
//   pub fn make_changes(&self, new_tags: &[String]) -> Result<bool> { self.repo.make_changes(new_tags) }
//
//   fn load_path<P: AsRef<Path>>(&self, rel_path: P) -> Result<String> {
//     let prev = self.repo.slice(self.spec.clone());
//     let blob = prev.blob(rel_path)?;
//     let cont: &str = std::str::from_utf8(blob.content())?;
//     Ok(cont.to_string())
//   }
//
//   pub fn changes(&self) -> Result<Changes> {
//     let base = self.repo.slice(self.spec.clone()).refspec().to_string();
//     let head = self.repo.branch_name().to_string();
//     changes(&self.repo, head, base)
//   }
//
//   pub fn line_commits(&self) -> Result<Vec<CommitData>> {
//     let base = self.repo.slice(self.spec.clone()).refspec().to_string();
//     let head = self.repo.branch_name().to_string();
//     line_commits(&self.repo, head, base)
//   }
//
//   pub fn keyed_files<'a>(&'a self) -> Result<impl Iterator<Item = Result<(String, String)>> + 'a> {
//     let changes = self.changes()?;
//     let prs = changes.into_groups().into_iter().map(|(_, v)| v).filter(|pr| !pr.best_guess());
//
//     let mut vec = Vec::new();
//     for pr in prs {
//       vec.push(pr_keyed_files(&self.repo, pr));
//     }
//
//     Ok(vec.into_iter().flatten())
//   }
// }

pub struct SliceSource<'r> {
  slice: Slice<'r>,
  root_dir: PathBuf
}

impl<'r> Source for SliceSource<'r> {
  type Loaded = String;
  fn root_dir(&self) -> &Path { &self.root_dir }
  fn has(&self, rel_path: &Path) -> Result<bool> { self.has_path(rel_path) }
  fn load(&self, rel_path: &Path) -> Result<Option<String>> { self.load_path(rel_path).map(Some) }
  fn commit_oid(&self) -> Result<Option<String>> { Ok(Some(self.slice.repo().revparse_oid(self.slice.refspec())?)) }
}

impl<'r> SliceSource<'r> {
  pub fn new(slice: Slice<'r>) -> Result<SliceSource> {
    let root_dir = slice.repo().working_dir()?;
    Ok(SliceSource::at(slice, root_dir.to_path_buf()))
  }

  pub fn at(slice: Slice<'r>, root_dir: PathBuf) -> SliceSource { SliceSource { slice, root_dir } }

  pub fn slice(&self, spec: String) -> SliceSource<'r> {
    SliceSource::at(self.slice.slice(spec), self.root_dir.clone())
  }

  pub fn has_remote(&self) -> bool { self.repo().has_remote() }
  pub fn has_path(&self, rel_path: &Path) -> Result<bool> { self.slice.has_blob(rel_path) }
  pub fn repo(&self) -> &Repo { &self.slice.repo() }
  pub fn pull(&self) -> Result<()> { self.repo().pull() }
  pub fn make_changes(&self, new_tags: &[String]) -> Result<bool> { self.repo().make_changes(new_tags) }

  fn load_path<P: AsRef<Path>>(&self, rel_path: P) -> Result<String> {
    let blob = self.slice.blob(rel_path)?;
    let cont: &str = std::str::from_utf8(blob.content())?;
    Ok(cont.to_string())
  }

  pub fn line_commits(&self) -> Result<Vec<CommitData>> {
    let base = self.slice.refspec().to_string();
    let head = self.repo().branch_name().to_string();
    line_commits(&self.repo(), head, base)
  }
}
