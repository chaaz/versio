//! The configuration and top-level commands for Versio.

use crate::error::Result;
use crate::json::JsonLoad;
use crate::toml::TomlLoad;
use crate::yaml::YamlLoad;
use crate::{Load, Mark, MarkedData};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

pub fn load_config<P: AsRef<Path>>(dir: P) -> Result<Config> {
  let dir = dir.as_ref();
  let rc = dir.join(".versio.yaml");
  let data = read_to_string(rc)?;
  let mut config = read_config(&data)?;
  config.dirname = dir.to_path_buf();
  Ok(config)
}

pub fn read_config(data: &str) -> Result<Config> {
  let config: Config = serde_yaml::from_str(&data)?;
  config.validate()?;

  Ok(config)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
  #[serde(skip)]
  dirname: PathBuf,
  projects: Vec<Project>
}

impl Config {
  pub fn show(&self) -> Result<()> {
    let name_width = self.projects.iter().map(|p| p.name.len()).max().unwrap_or(0);

    for project in &self.projects {
      project.show(&self.dirname, name_width, false)?;
    }
    Ok(())
  }

  pub fn get_name(&self, name: &str, vonly: bool) -> Result<()> {
    let filter = |p: &&Project| p.name.contains(name);
    let name_width = self.projects.iter().filter(filter).map(|p| p.name.len()).max().unwrap_or(0);

    for project in self.projects.iter().filter(filter) {
      project.show(&self.dirname, name_width, vonly)?;
    }
    Ok(())
  }

  pub fn get_id(&self, id: u32, vonly: bool) -> Result<()> {
    let project = self.projects.iter().find(|p| p.id == id).ok_or_else(|| versio_error!("No such project {}", id))?;
    project.show(&self.dirname, 0, vonly)
  }

  pub fn set_by_name(&self, name: &str, val: &str) -> Result<()> {
    let id = self.find_unique(name)?;
    self.set_by_id(id, val)
  }

  pub fn set_by_id(&self, id: u32, val: &str) -> Result<()> {
    let project = self.projects.iter().find(|p| p.id == id).ok_or_else(|| versio_error!("No such project {}", id))?;
    project.set_value(&self.dirname, val)
  }

  fn find_unique(&self, name: &str) -> Result<u32> {
    let mut iter = self.projects.iter().filter(|p| p.name.contains(name)).map(|p| p.id);
    let id = iter.next().ok_or_else(|| versio_error!("No project named {}", name))?;
    if iter.next().is_some() {
      return versio_err!("Multiple projects with name {}", name);
    }
    Ok(id)
  }

  /// Check that IDs are unique, etc.
  pub fn validate(&self) -> Result<()> {
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
  pub fn show(&self, dirname: &Path, name_width: usize, vonly: bool) -> Result<()> {
    let mark = self.located.get_mark(dirname)?;
    if vonly {
      println!("{}", mark.value());
    } else {
      println!("{:width$} : {}", self.name, mark.value(), width = name_width);
    }
    Ok(())
  }

  pub fn set_value(&self, dirname: &Path, val: &str) -> Result<()> {
    let mut mark = self.located.get_mark(dirname)?;
    mark.update_file(val)
  }
}

#[derive(Serialize, Deserialize, Debug)]
struct Location {
  file: String,
  #[serde(flatten)]
  picker: Picker
}

impl Location {
  pub fn get_mark(&self, dirname: &Path) -> Result<MarkedData> {
    self.picker.get_mark(&dirname.join(&self.file)).map_err(|e| versio_error!("Can't mark {}: {:?}", self.file, e))
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

  pub fn get_mark(&self, filename: &Path) -> Result<MarkedData> {
    match self {
      Picker::Json(p) => p.load(filename),
      Picker::Yaml(p) => p.load(filename),
      Picker::Toml(p) => p.load(filename),
      Picker::Line(p) => p.load(filename),
      Picker::File(p) => p.load(filename)
    }
  }
}

#[derive(Serialize, Deserialize, Debug)]
struct JsonPicker {
  json: String
}

impl JsonPicker {
  pub fn load(&self, filename: &Path) -> Result<MarkedData> { JsonLoad::new(self.json.as_str()).load(filename) }
}

#[derive(Serialize, Deserialize, Debug)]
struct YamlPicker {
  yaml: String
}

impl YamlPicker {
  pub fn load(&self, filename: &Path) -> Result<MarkedData> { YamlLoad::new(self.yaml.as_str()).load(filename) }
}

#[derive(Serialize, Deserialize, Debug)]
struct TomlPicker {
  toml: String
}

impl TomlPicker {
  pub fn load(&self, filename: &Path) -> Result<MarkedData> { TomlLoad::new(self.toml.as_str()).load(filename) }
}

#[derive(Serialize, Deserialize, Debug)]
struct LinePicker {
  pattern: String
}

impl LinePicker {
  pub fn load(&self, filename: &Path) -> Result<MarkedData> {
    let data = std::fs::read_to_string(&filename)?;
    find_reg_data(data, &self.pattern, Some(filename.to_path_buf()))
  }
}

fn find_reg_data(data: String, pattern: &str, filename: Option<PathBuf>) -> Result<MarkedData> {
  let pattern = Regex::new(pattern)?;
  let found = pattern.captures(&data).ok_or_else(|| versio_error!("No match for {}", pattern))?;
  let item = found.get(1).ok_or_else(|| versio_error!("No capture group in {}.", pattern))?;
  let value = item.as_str().to_string();
  let index = item.start();
  Ok(MarkedData::new(filename, data, Mark::new(value, index)))
}

#[derive(Serialize, Deserialize, Debug)]
struct FilePicker {}

impl FilePicker {
  pub fn load(&self, filename: &Path) -> Result<MarkedData> {
    let data = std::fs::read_to_string(&filename)?;
    let value = data.trim_end().to_string();
    Ok(MarkedData::new(Some(filename.to_path_buf()), data, Mark::new(value, 0)))
  }
}

#[cfg(test)]
mod test {
  use super::{find_reg_data, read_config};

  #[test]
  fn test_load() {
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

    let config = read_config(data).unwrap();

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

    assert!(read_config(config).is_err());
  }

  #[test]
  fn test_find_reg() {
    let data = r#"
This is text.
Current rev is "v1.2.3" because it is."#;

    let marked_data = find_reg_data(data.to_string(), "v(\\d+\\.\\d+\\.\\d+)", None).unwrap();
    assert_eq!("1.2.3", marked_data.value());
    assert_eq!(32, marked_data.start());
  }
}
