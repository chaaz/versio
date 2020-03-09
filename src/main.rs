#[macro_use]
pub mod error;
pub mod analyze;
pub mod config;
pub mod either;
pub mod git;
pub mod json;
pub mod opts;
pub mod parts;
pub mod toml;
pub mod yaml;

use crate::error::Result;
use crate::git::{add_and_commit, fetch, get_changed_since, merge_after_fetch, prev_blob, FetchResults};
use git2::{Oid, Repository};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

pub const CONFIG_FILENAME: &str = ".versio.yaml";

pub trait Source {
  fn root_dir(&self) -> &Path;
  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>>;
}

impl<S: Source> Source for &S {
  fn root_dir(&self) -> &Path { <S as Source>::root_dir(*self) }
  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>> { <S as Source>::load(*self, rel_path) }
}

pub struct CurrentSource {
  root_dir: PathBuf
}

impl CurrentSource {
  pub fn open<P: AsRef<Path>>(root_dir: P) -> Result<CurrentSource> {
    let root_dir =
      Repository::open(root_dir)?.workdir().ok_or_else(|| versio_error!("No working directory."))?.to_path_buf();
    Ok(CurrentSource { root_dir })
  }
}

impl Source for CurrentSource {
  fn root_dir(&self) -> &Path { &self.root_dir }

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

pub struct RepoGuard<'a> {
  guard: MutexGuard<'a, PrevSourceInner>
}

impl<'a> RepoGuard<'a> {
  pub fn repo(&self) -> &Repository { &self.guard.repo }

  pub fn get_keyed_files<'b>(&'b mut self) -> Result<impl Iterator<Item = Result<(String, String)>> + 'b> {
    self.guard.get_keyed_files()
  }

  pub fn add_and_commit(&mut self) -> Result<Option<Oid>> { self.guard.add_and_commit() }
}

#[derive(Clone)]
pub struct PrevSource {
  root_dir: PathBuf,
  inner: Arc<Mutex<PrevSourceInner>>
}

impl PrevSource {
  pub fn open<P: AsRef<Path>>(root_dir: P) -> Result<PrevSource> {
    let root_dir = root_dir.as_ref();
    let inner = PrevSourceInner::open(root_dir)?;
    Ok(PrevSource { root_dir: root_dir.to_path_buf(), inner: Arc::new(Mutex::new(inner)) })
  }

  pub fn set_fetch(&mut self, fetch: bool) -> Result<()> {
    self.inner.lock()?.set_fetch(fetch);
    Ok(())
  }

  pub fn set_merge(&mut self, merge: bool) -> Result<()> {
    self.inner.lock()?.set_merge(merge);
    Ok(())
  }

  pub fn repo(&self) -> Result<RepoGuard> { Ok(RepoGuard { guard: self.inner.lock()? }) }
}

impl Source for PrevSource {
  fn root_dir(&self) -> &Path { &self.root_dir }

  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>> { self.inner.lock()?.load(rel_path) }
}

pub struct PrevSourceInner {
  repo: Repository,
  should_fetch: bool,
  will_merge: bool,
  fetch_results: Option<FetchResults>,
  merged: bool
}

impl PrevSourceInner {
  pub fn open(root_dir: &Path) -> Result<PrevSourceInner> {
    let repo = Repository::open(root_dir)?;
    Ok(PrevSourceInner { repo, should_fetch: true, will_merge: false, fetch_results: None, merged: false })
  }

  fn set_fetch(&mut self, fetch: bool) { self.should_fetch = fetch; }

  fn set_merge(&mut self, merge: bool) { self.will_merge = merge; }

  fn load<P: AsRef<Path>>(&mut self, rel_path: P) -> Result<Option<NamedData>> {
    self.maybe_fetch()?;
    let blob = prev_blob(&self.repo, rel_path)?;
    blob
      .map(|blob| {
        let cont: Result<&str> = Ok(std::str::from_utf8(blob.content())?);
        cont.map(|cont| NamedData::new(None, cont.to_string()))
      })
      .transpose()
  }

  fn get_keyed_files<'a>(&'a mut self) -> Result<impl Iterator<Item = Result<(String, String)>> + 'a> {
    self.maybe_fetch()?;
    get_changed_since(&self.repo)
  }

  pub fn add_and_commit(&mut self) -> Result<Option<Oid>> {
    add_and_commit(
      &self.repo,
      self.fetch_results.as_ref().ok_or_else(|| versio_error!("Can't commit w/out prior fetch."))?
    )
  }

  fn maybe_fetch(&mut self) -> Result<()> {
    if self.will_merge {
      self.maybe_fetch_opts(true)?;
      self.maybe_merge_after()
    } else {
      self.maybe_fetch_opts(false)
    }
  }

  fn maybe_fetch_opts(&mut self, force: bool) -> Result<()> {
    if (self.should_fetch || force) && self.fetch_results.is_none() {
      self.fetch_results = Some(fetch(&self.repo, None, None)?);
    }
    Ok(())
  }

  fn maybe_merge_after(&mut self) -> Result<()> {
    if !self.merged {
      let fetch_results = self.fetch_results.as_ref().unwrap();
      merge_after_fetch(&self.repo, fetch_results)?;
      self.merged = true;
    }
    Ok(())
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
}

pub fn convert_mark(data: &str, cmark: CharMark) -> Result<Mark> {
  let start = data.char_indices().nth(cmark.char_start()).unwrap().0;
  Mark::make(cmark.value, start)
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

pub trait Scanner {
  fn scan(&self, data: NamedData) -> Result<MarkedData>;
}

fn main() -> Result<()> { opts::execute() }
