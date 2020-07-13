//! Versio is a version management utility.

use crate::either::{IterEither2 as E2, IterEither3 as E3};
use crate::error::Result;
use crate::git::{CommitData, FullPr, Repo, Slice};
use crate::github::{changes, line_commits, Changes};
use regex::Regex;
use std::iter;
use std::path::{Path, PathBuf};

pub const CONFIG_FILENAME: &str = ".versio.yaml";

pub trait Source {
  fn root_dir(&self) -> &Path;
  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>>;
  fn has(&self, rel_path: &Path) -> Result<bool>;
}

impl<S: Source> Source for &S {
  fn root_dir(&self) -> &Path { <S as Source>::root_dir(*self) }
  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>> { <S as Source>::load(*self, rel_path) }
  fn has(&self, rel_path: &Path) -> Result<bool> { <S as Source>::has(*self, rel_path) }
}

pub struct CurrentSource {
  root_dir: PathBuf
}

impl CurrentSource {
  pub fn open<P: AsRef<Path>>(dir: P) -> Result<CurrentSource> { Ok(CurrentSource { root_dir: Repo::root_dir(dir)? }) }
}

impl Source for CurrentSource {
  fn root_dir(&self) -> &Path { &self.root_dir }

  fn has(&self, rel_path: &Path) -> Result<bool> { Ok(self.root_dir.join(rel_path).exists()) }

  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>> {
    let path = self.root_dir.join(rel_path);
    if Path::exists(&path) {
      let data = std::fs::read_to_string(&path)?;
      Ok(Some(NamedData::new(Some(path), data)))
    } else {
      Ok(None)
    }
  }
}

pub struct PrevSource {
  repo: Repo,
  spec: String,
  root_dir: PathBuf
}

impl Source for PrevSource {
  fn root_dir(&self) -> &Path { &self.root_dir }
  fn has(&self, rel_path: &Path) -> Result<bool> { self.has_path(rel_path) }
  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>> { self.load_path(rel_path).map(Some) }
}

impl PrevSource {
  pub fn open(dir: &Path) -> Result<PrevSource> {
    let repo = Repo::open(dir)?;
    let spec = repo.prev().refspec().to_string();
    let root_dir = repo.working_dir()?.to_path_buf();
    Ok(PrevSource { repo, spec, root_dir })
  }

  pub fn open_at<P: AsRef<Path>>(dir: P, spec: String) -> Result<PrevSource> {
    let repo = Repo::open(dir.as_ref())?;
    let root_dir = repo.working_dir()?.to_path_buf();
    Ok(PrevSource { repo, spec, root_dir })
  }

  pub fn slice(&self, spec: String) -> SliceSource { SliceSource::new(self.repo.slice(spec), self.root_dir.clone()) }

  pub fn has_remote(&self) -> bool { self.repo.has_remote() }
  pub fn has_path(&self, rel_path: &Path) -> Result<bool> { self.repo.slice(self.spec.clone()).has_blob(rel_path) }
  pub fn repo(&self) -> Result<&Repo> { Ok(&self.repo) }
  pub fn pull(&self) -> Result<()> { self.repo.pull() }
  pub fn make_changes(&self, new_tags: &[String]) -> Result<bool> { self.repo.make_changes(new_tags) }

  fn load_path<P: AsRef<Path>>(&self, rel_path: P) -> Result<NamedData> {
    let prev = self.repo.slice(self.spec.clone());
    let blob = prev.blob(rel_path)?;
    let cont: &str = std::str::from_utf8(blob.content())?;
    Ok(NamedData::new(None, cont.to_string()))
  }

  pub fn changes(&self) -> Result<Changes> {
    let base = self.repo.slice(self.spec.clone()).refspec().to_string();
    let head = self.repo.branch_name().to_string();
    changes(&self.repo, head, base)
  }

  pub fn line_commits(&self) -> Result<Vec<CommitData>> {
    let base = self.repo.slice(self.spec.clone()).refspec().to_string();
    let head = self.repo.branch_name().to_string();
    line_commits(&self.repo, head, base)
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
}

pub struct SliceSource<'r> {
  slice: Slice<'r>,
  root_dir: PathBuf
}

impl<'r> Source for SliceSource<'r> {
  fn root_dir(&self) -> &Path { &self.root_dir }
  fn has(&self, rel_path: &Path) -> Result<bool> { self.has_path(rel_path) }
  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>> { self.load_path(rel_path).map(Some) }
}

impl<'r> SliceSource<'r> {
  pub fn new(slice: Slice<'r>, root_dir: PathBuf) -> SliceSource { SliceSource { slice, root_dir } }

  pub fn has_path(&self, rel_path: &Path) -> Result<bool> { self.slice.has_blob(rel_path) }

  pub fn slice(&self, spec: String) -> SliceSource<'r> {
    SliceSource::new(self.slice.slice(spec), self.root_dir.clone())
  }

  fn load_path<P: AsRef<Path>>(&self, rel_path: P) -> Result<NamedData> {
    let blob = self.slice.blob(rel_path)?;
    let cont: &str = std::str::from_utf8(blob.content())?;
    Ok(NamedData::new(None, cont.to_string()))
  }
}

pub struct NamedData {
  writeable_path: Option<PathBuf>,
  data: String
}

impl NamedData {
  pub fn new(writeable_path: Option<PathBuf>, data: String) -> NamedData { NamedData { writeable_path, data } }
  pub fn writeable_path(&self) -> &Option<PathBuf> { &self.writeable_path }
  pub fn data(&self) -> &str { &self.data }
  pub fn mark(self, mark: Mark) -> MarkedData { MarkedData::new(self.writeable_path, self.data, mark) }
}

pub struct MarkedData {
  writeable_path: Option<PathBuf>,
  data: String,
  mark: Mark
}

impl MarkedData {
  pub fn new(writeable_path: Option<PathBuf>, data: String, mark: Mark) -> MarkedData {
    MarkedData { writeable_path, data, mark }
  }

  pub fn write_new_value(&mut self, new_val: &str) -> Result<()> {
    // Fail before setting internals.
    if self.writeable_path.is_none() {
      return versio_err!("Can't write value: no writeable path");
    }

    self.set_value(new_val)?;
    self.write()?;
    Ok(())
  }

  fn set_value(&mut self, new_val: &str) -> Result<()> {
    let st = self.mark.start();
    let ed = st + self.mark.value().len();
    self.data.replace_range(st .. ed, &new_val);
    self.mark.set_value(new_val.to_string());
    Ok(())
  }

  fn write(&self) -> Result<()> {
    self
      .writeable_path
      .as_ref()
      .ok_or_else(|| versio_error!("Can't write file: none exists."))
      .and_then(|writeable_path| Ok(std::fs::write(writeable_path, &self.data)?))?;

    Ok(())
  }

  pub fn value(&self) -> &str { self.mark.value() }
  pub fn start(&self) -> usize { self.mark.start() }
  pub fn data(&self) -> &str { &self.data }
  pub fn writeable_path(&self) -> &Option<PathBuf> { &self.writeable_path }
}

#[derive(Debug)]
pub struct Mark {
  value: String,
  byte_start: usize
}

impl Mark {
  pub fn make(value: String, byte_start: usize) -> Result<Mark> {
    let regex = Regex::new(r"\A\d+\.\d+\.\d+\z")?;
    if !regex.is_match(&value) {
      return versio_err!("Value \"{}\" is not a version.", value);
    }

    Ok(Mark { value, byte_start })
  }

  pub fn value(&self) -> &str { &self.value }
  pub fn set_value(&mut self, new_val: String) { self.value = new_val; }
  pub fn start(&self) -> usize { self.byte_start }
}

#[derive(Debug)]
pub struct CharMark {
  value: String,
  char_start: usize
}

impl CharMark {
  pub fn new(value: String, char_start: usize) -> CharMark { CharMark { value, char_start } }
  pub fn value(&self) -> &str { &self.value }
  pub fn char_start(&self) -> usize { self.char_start }

  pub fn into_byte_mark(self, data: &str) -> Result<Mark> {
    let start = data.char_indices().nth(self.char_start).unwrap().0;
    Mark::make(self.value, start)
  }
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
