//! Management of reading and writing marks to files.

use crate::error::Result;
use crate::scan::parts::{deserialize_parts, Part};
use crate::scan::{JsonScanner, Scanner, TomlScanner, XmlScanner, YamlScanner};
use regex::Regex;
use serde::Deserialize;
use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

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
pub struct ScanningPicker<T: Scanner> {
  #[serde(deserialize_with = "deserialize_parts")]
  parts: Vec<Part>,
  _scan: PhantomData<T>
}

impl<T: Scanner> Clone for ScanningPicker<T> {
  fn clone(&self) -> ScanningPicker<T> { ScanningPicker { parts: self.parts.clone(), _scan: PhantomData } }
}

impl<T: Scanner> fmt::Debug for ScanningPicker<T> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "ScanningPicker {{ {:?} }}", self.parts) }
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

pub struct NamedData {
  writeable_path: PathBuf,
  data: String
}

impl From<NamedData> for String {
  fn from(d: NamedData) -> String { d.data }
}

impl NamedData {
  pub fn new(writeable_path: PathBuf, data: String) -> NamedData { NamedData { writeable_path, data } }
  pub fn writeable_path(&self) -> &Path { &self.writeable_path }
  pub fn data(&self) -> &str { &self.data }
  pub fn mark(self, mark: Mark) -> MarkedData { MarkedData::new(self.writeable_path, self.data, mark) }
}

pub struct MarkedData {
  writeable_path: PathBuf,
  data: String,
  mark: Mark
}

impl MarkedData {
  pub fn new(writeable_path: PathBuf, data: String, mark: Mark) -> MarkedData {
    MarkedData { writeable_path, data, mark }
  }

  pub fn value(&self) -> &str { self.mark.value() }
  pub fn start(&self) -> usize { self.mark.start() }

  pub fn write_new_value(&mut self, new_val: &str) -> Result<()> {
    self.set_value(new_val)?;
    self.write()?;
    Ok(())
  }

  fn set_value(&mut self, new_val: &str) -> Result<()> {
    let st = self.start();
    let ed = st + self.value().len();
    self.data.replace_range(st .. ed, &new_val);
    self.mark.set_value(new_val.to_string());
    Ok(())
  }

  fn write(&self) -> Result<()> { Ok(std::fs::write(&self.writeable_path, &self.data)?) }
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
  pub fn into_value(self) -> String { self.value }
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
