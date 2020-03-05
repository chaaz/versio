//! The configuration and top-level commands for Versio.

use crate::analyze::AnnotatedMark;
use crate::error::Result;
use crate::json::JsonScanner;
use crate::toml::TomlScanner;
use crate::yaml::YamlScanner;
use crate::{Mark, MarkedData, NamedData, Scanner, Source, CONFIG_FILENAME};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub struct Config<S: Source> {
  source: S,
  file: ConfigFile
}

impl<S: Source> Config<S> {
  pub fn from_source(source: S) -> Result<Config<S>> {
    let file = ConfigFile::load(&source)?;
    Ok(Config { source, file })
  }

  pub fn annotate(&self) -> Result<Vec<AnnotatedMark>> {
    self.file.projects.iter().map(|p| p.annotate(&self.source)).collect()
  }

  pub fn show(&self) -> Result<()> {
    let name_width = self.file.projects.iter().map(|p| p.name.len()).max().unwrap_or(0);

    for project in &self.file.projects {
      project.show(&self.source, name_width, false)?;
    }
    Ok(())
  }

  pub fn get_name(&self, name: &str, vonly: bool) -> Result<()> {
    let filter = |p: &&Project| p.name.contains(name);
    let name_width = self.file.projects.iter().filter(filter).map(|p| p.name.len()).max().unwrap_or(0);

    for project in self.file.projects.iter().filter(filter) {
      project.show(&self.source, name_width, vonly)?;
    }
    Ok(())
  }

  pub fn get_id(&self, id: u32, vonly: bool) -> Result<()> {
    let project =
      self.file.projects.iter().find(|p| p.id == id).ok_or_else(|| versio_error!("No such project {}", id))?;
    project.show(&self.source, 0, vonly)
  }

  pub fn set_by_name(&self, name: &str, val: &str) -> Result<()> {
    let id = self.find_unique(name)?;
    self.set_by_id(id, val)
  }

  pub fn set_by_id(&self, id: u32, val: &str) -> Result<()> {
    let project =
      self.file.projects.iter().find(|p| p.id == id).ok_or_else(|| versio_error!("No such project {}", id))?;
    project.set_value(&self.source, val)
  }

  fn find_unique(&self, name: &str) -> Result<u32> {
    let mut iter = self.file.projects.iter().filter(|p| p.name.contains(name)).map(|p| p.id);
    let id = iter.next().ok_or_else(|| versio_error!("No project named {}", name))?;
    if iter.next().is_some() {
      return versio_err!("Multiple projects with name {}", name);
    }
    Ok(id)
  }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigFile {
  projects: Vec<Project>
}

impl ConfigFile {
  pub fn load(source: &dyn Source) -> Result<ConfigFile> {
    match source.load(CONFIG_FILENAME.as_ref())? {
      Some(data) => ConfigFile::read(data.data()),
      None => Ok(ConfigFile::empty())
    }
  }

  pub fn empty() -> ConfigFile { ConfigFile { projects: Vec::new() } }

  pub fn read(data: &str) -> Result<ConfigFile> {
    let file: ConfigFile = serde_yaml::from_str(data)?;
    file.validate()?;
    Ok(file)
  }

  /// Check that IDs are unique, etc.
  fn validate(&self) -> Result<()> {
    let mut ids = HashSet::new();
    for p in &self.projects {
      if ids.contains(&p.id) {
        return versio_err!("Id {} is duplicated", p.id);
      }
      ids.insert(p.id);
    }

    Ok(())
  }
}

#[derive(Serialize, Deserialize, Debug)]
struct Project {
  name: String,
  id: u32,
  #[serde(default)]
  covers: Vec<String>,
  #[serde(default)]
  depends: Vec<u32>,
  located: Location
}

impl Project {
  pub fn annotate(&self, source: &dyn Source) -> Result<AnnotatedMark> {
    Ok(AnnotatedMark::new(self.id, self.name.clone(), self.located.get_mark(source)?))
  }

  pub fn show(&self, source: &dyn Source, name_width: usize, vonly: bool) -> Result<()> {
    let mark = self.located.get_mark(source)?;
    if vonly {
      println!("{}", mark.value());
    } else {
      println!("{:width$} : {}", self.name, mark.value(), width = name_width);
    }
    Ok(())
  }

  pub fn set_value(&self, source: &dyn Source, val: &str) -> Result<()> {
    let mut mark = self.located.get_mark(source)?;
    mark.write_new_value(val)
  }
}

#[derive(Serialize, Deserialize, Debug)]
struct Location {
  file: String,
  #[serde(flatten)]
  picker: Picker
}

impl Location {
  pub fn get_mark(&self, source: &dyn Source) -> Result<MarkedData> {
    let data = source.load(&self.file.as_ref())?.ok_or_else(|| versio_error!("No file at {}.", self.file))?;
    self.picker.get_mark(data).map_err(|e| versio_error!("Can't mark {}: {:?}", self.file, e))
  }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum Picker {
  Json(JsonPicker),
  Yaml(YamlPicker),
  Toml(TomlPicker),
  Line(LinePicker),
  File(FilePicker)
}

impl Picker {
  pub fn _type(&self) -> &'static str {
    match self {
      Picker::Json(_) => "json",
      Picker::Yaml(_) => "yaml",
      Picker::Toml(_) => "toml",
      Picker::Line(_) => "line",
      Picker::File(_) => "file"
    }
  }

  pub fn get_mark(&self, data: NamedData) -> Result<MarkedData> {
    match self {
      Picker::Json(p) => p.scan(data),
      Picker::Yaml(p) => p.scan(data),
      Picker::Toml(p) => p.scan(data),
      Picker::Line(p) => p.scan(data),
      Picker::File(p) => p.scan(data)
    }
  }
}

#[derive(Serialize, Deserialize, Debug)]
struct JsonPicker {
  json: String
}

impl JsonPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { JsonScanner::new(self.json.as_str()).scan(data) }
}

#[derive(Serialize, Deserialize, Debug)]
struct YamlPicker {
  yaml: String
}

impl YamlPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { YamlScanner::new(self.yaml.as_str()).scan(data) }
}

#[derive(Serialize, Deserialize, Debug)]
struct TomlPicker {
  toml: String
}

impl TomlPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { TomlScanner::new(self.toml.as_str()).scan(data) }
}

#[derive(Serialize, Deserialize, Debug)]
struct LinePicker {
  pattern: String
}

impl LinePicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { find_reg_data(data, &self.pattern) }
}

fn find_reg_data(data: NamedData, pattern: &str) -> Result<MarkedData> {
  let pattern = Regex::new(pattern)?;
  let found = pattern.captures(data.data()).ok_or_else(|| versio_error!("No match for {}", pattern))?;
  let item = found.get(1).ok_or_else(|| versio_error!("No capture group in {}.", pattern))?;
  let value = item.as_str().to_string();
  let index = item.start();
  Ok(data.mark(Mark::new(value, index)))
}

#[derive(Serialize, Deserialize, Debug)]
struct FilePicker {}

impl FilePicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> {
    let value = data.data().trim_end().to_string();
    Ok(data.mark(Mark::new(value, 0)))
  }
}

#[cfg(test)]
mod test {
  use super::{find_reg_data, ConfigFile};
  use crate::NamedData;

  #[test]
  fn test_scan() {
    let data = r#"
projects:
  - name: everything
    id: 1
    covers: ["**"]
    located:
      file: "toplevel.json"
      json: "version"

  - name: project1
    id: 2
    covers: ["project1/**"]
    located:
      file: "project1/Cargo.toml"
      toml: "version"

  - name: "combined a and b"
    id: 3
    covers: ["nested/project_a/**", "nested/project_b/**"]
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
    assert_eq!("line", config.projects[2].located.picker._type());
  }

  #[test]
  fn test_validate() {
    let config = r#"
projects:
  - name: p1
    id: 1
    covers: ["**"]
    located: { file: f1 }

  - name: project1
    id: 1
    covers: ["**"]
    located: { file: f2 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_find_reg() {
    let data = r#"
This is text.
Current rev is "v1.2.3" because it is."#;

    let marked_data = find_reg_data(NamedData::new(None, data.to_string()), "v(\\d+\\.\\d+\\.\\d+)").unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(32, marked_data.start());
  }
}
