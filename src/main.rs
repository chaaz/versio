#[macro_use]
pub mod error;
pub mod config;
pub mod git;
pub mod json;
pub mod opts;
pub mod toml;
pub mod yaml;

use crate::error::Result;
use crate::git::{fetch, merge_after_fetch, prev_blob};
use git2::{Oid, Repository};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const CONFIG_FILENAME: &str = ".versio.yaml";

pub trait Source {
  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>>;
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

#[derive(Clone)]
pub struct PrevSource {
  inner: Arc<Mutex<PrevSourceInner>>
}

impl PrevSource {
  pub fn open<P: AsRef<Path>>(root_dir: P) -> Result<PrevSource> {
    let inner = PrevSourceInner::open(root_dir)?;
    Ok(PrevSource { inner: Arc::new(Mutex::new(inner)) })
  }
}

impl Source for PrevSource {
  fn load(&self, rel_path: &Path) -> Result<Option<NamedData>> { self.inner.lock()?.load(rel_path) }
}

pub struct PrevSourceInner {
  repo: Repository,
  fetched: Option<(String, Option<Oid>)>,
  _merged: bool
}

impl PrevSourceInner {
  pub fn open<P: AsRef<Path>>(root_dir: P) -> Result<PrevSourceInner> {
    let repo = Repository::open(root_dir)?;
    Ok(PrevSourceInner { repo, fetched: None, _merged: false })
  }

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

  fn _maybe_pull(&mut self) -> Result<()> {
    self.maybe_fetch()?;
    self._maybe_merge_after()
  }

  fn maybe_fetch(&mut self) -> Result<()> {
    if self.fetched.is_none() {
      self.fetched = Some(fetch(&self.repo, None, None)?);
    }
    Ok(())
  }

  fn _maybe_merge_after(&mut self) -> Result<()> {
    if !self._merged {
      let fetched = self.fetched.as_ref().unwrap();
      merge_after_fetch(&self.repo, &fetched.0, fetched.1)?;
      self._merged = true;
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
  pub fn new(value: String, byte_start: usize) -> Mark { Mark { value, byte_start } }
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

pub fn convert_mark(data: &str, cmark: CharMark) -> Mark {
  let start = data.char_indices().nth(cmark.char_start()).unwrap().0;
  Mark::new(cmark.value, start)
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
