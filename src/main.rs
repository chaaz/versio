#[macro_use]
pub mod error;
pub mod config;
pub mod git;
pub mod json;
pub mod opts;
pub mod toml;
pub mod yaml;

use crate::error::Result;
use std::path::{Path, PathBuf};

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
  let start = data.char_indices().skip(cmark.char_start()).next().unwrap().0;
  Mark::new(cmark.value, start)
}

pub struct MarkedData {
  fname: Option<PathBuf>,
  data: String,
  mark: Mark
}

impl MarkedData {
  pub fn new(fname: Option<PathBuf>, data: String, mark: Mark) -> MarkedData { MarkedData { fname, data, mark } }

  pub fn update_file(&mut self, new_val: &str) -> Result<()> {
    self.update(new_val)?;
    self.write()?;
    Ok(())
  }

  pub fn update(&mut self, new_val: &str) -> Result<()> {
    let st = self.mark.start();
    let ed = st + self.mark.value().len();
    self.data.replace_range(st .. ed, &new_val);
    self.mark.set_value(new_val.to_string());
    Ok(())
  }

  pub fn write(&self) -> Result<()> {
    self
      .fname
      .as_ref()
      .ok_or_else(|| versio_error!("Can't write file: none exists."))
      .and_then(|fname| Ok(std::fs::write(fname, &self.data)?))?;

    Ok(())
  }

  pub fn value(&self) -> &str { self.mark.value() }
  pub fn start(&self) -> usize { self.mark.start() }
  pub fn data(&self) -> &str { &self.data }
  pub fn filename(&self) -> &Option<PathBuf> { &self.fname }
}

pub trait Load {
  fn load<P: AsRef<Path>>(&self, filename: P) -> Result<MarkedData>;
  fn read(&self, data: String, fname: Option<PathBuf>) -> Result<MarkedData>;
}

fn main() -> Result<()> { opts::execute() }
