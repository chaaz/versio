use crate::error::Result;
use crate::yaml::YamlLoad;
use crate::{Load, MarkedData};
use serde::{Deserialize, Serialize};
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
  #[serde(skip)]
  dirname: PathBuf,
  projects: Vec<Project>
}

impl Config {
  pub fn show(&self) -> Result<()> {
    for project in &self.projects {
      project.show(&self.dirname)?;
    }
    Ok(())
  }
}

#[derive(Serialize, Deserialize, Debug)]
struct Project {
  name: String,
  id: u32,
  #[serde(flatten)]
  watches: Watches,
  located: Location
}

impl Project {
  pub fn show(&self, dirname: &Path) -> Result<()> {
    let mark = self.located.get_mark(dirname)?;
    println!("{} : {}", self.name, mark.value());
    Ok(())
  }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum Watches {
  Covers(Covers),
  Depends(Depends)
}

#[derive(Serialize, Deserialize, Debug)]
struct Depends {
  depends: Vec<u32>
}

#[derive(Serialize, Deserialize, Debug)]
struct Covers {
  covers: Vec<String>
}

#[derive(Serialize, Deserialize, Debug)]
struct Location {
  file: String,
  #[serde(flatten)]
  picker: Picker
}

impl Location {
  pub fn get_mark(&self, dirname: &Path) -> Result<MarkedData> { self.picker.get_mark(&dirname.join(&self.file)) }
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
  pub fn load(&self, _filename: &Path) -> Result<MarkedData> { unimplemented!() }
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
  pub fn load(&self, _filename: &Path) -> Result<MarkedData> { unimplemented!() }
}

#[derive(Serialize, Deserialize, Debug)]
struct LinePicker {
  pattern: String
}

impl LinePicker {
  pub fn load(&self, _filename: &Path) -> Result<MarkedData> { unimplemented!() }
}

#[derive(Serialize, Deserialize, Debug)]
struct FilePicker {}

impl FilePicker {
  pub fn load(&self, _filename: &Path) -> Result<MarkedData> { unimplemented!() }
}

pub fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
  let data = read_to_string(path)?;
  Ok(serde_yaml::from_str(&data)?)
}

#[cfg(test)]
mod test {
  use super::read_config;

  #[test]
  fn test_load() {
    let config = read_config("/Users/charlie/.versio.yaml").unwrap();
    assert_eq!(config.projects[0].id, 1);
    assert_eq!("line", config.projects[2].located.picker._type());
  }
}
