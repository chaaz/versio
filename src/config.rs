//! The configuration and top-level commands for Versio.

use crate::analyze::AnnotatedMark;
use crate::error::Result;
use crate::json::JsonScanner;
use crate::parts::{deserialize_parts, Part};
use crate::toml::TomlScanner;
use crate::yaml::YamlScanner;
use crate::{CurrentSource, Mark, MarkedData, NamedData, PrevSource, Scanner, Source, CONFIG_FILENAME};
use glob::{glob, Pattern};
use regex::Regex;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use std::cmp::{max, Ord, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

pub fn configure_plan<'s>(
  prev: &'s PrevSource, curt: &'s CurrentSource
) -> Result<(Plan, Config<&'s PrevSource>, Config<&'s CurrentSource>)> {
  let prev_config = Config::from_source(prev)?;
  let curt_config = Config::from_source(curt)?;
  let mut plan = prev_config.start_plan(&curt_config);

  for result in prev.repo()?.get_keyed_files()? {
    let (key, path) = result?;
    plan.consider(&key, &path)?;
  }
  plan.consider_deps()?;

  let plan = plan.finish_plan()?;
  Ok((plan, prev_config, curt_config))
}

pub struct ShowFormat {
  pub wide: bool,
  pub version_only: bool
}

impl ShowFormat {
  pub fn new(wide: bool, version_only: bool) -> ShowFormat { ShowFormat { wide, version_only } }
}

pub struct Config<S: Source> {
  source: S,
  file: ConfigFile
}

impl<S: Source> Config<S> {
  pub fn from_source(source: S) -> Result<Config<S>> {
    let file = ConfigFile::load(&source)?;
    Ok(Config { source, file })
  }

  fn start_plan<'s, C: Source>(&'s self, current: &'s Config<C>) -> PlanConsider<S, C> {
    PlanConsider::new(self, current)
  }

  pub fn annotate(&self) -> Result<Vec<AnnotatedMark>> {
    self.file.projects.iter().map(|p| p.annotate(&self.source)).collect()
  }

  pub fn check(&self) -> Result<()> {
    for project in &self.file.projects {
      project.check(&self.source)?;
    }
    Ok(())
  }

  pub fn get_mark(&self, id: u32) -> Option<Result<MarkedData>> {
    self.get_project(id).map(|p| p.get_mark(&self.source))
  }

  pub fn show(&self, format: ShowFormat) -> Result<()> {
    let name_width = self.file.projects.iter().map(|p| p.name.len()).max().unwrap_or(0);

    for project in &self.file.projects {
      project.show(&self.source, name_width, &format)?;
    }
    Ok(())
  }

  pub fn get_project(&self, id: u32) -> Option<&Project> { self.file.projects.iter().find(|p| p.id == id) }

  pub fn show_id(&self, id: u32, format: ShowFormat) -> Result<()> {
    let project = self.get_project(id).ok_or_else(|| versio_error!("No such project {}", id))?;
    project.show(&self.source, 0, &format)
  }

  pub fn show_names(&self, name: &str, format: ShowFormat) -> Result<()> {
    let filter = |p: &&Project| p.name.contains(name);
    let name_width = self.file.projects.iter().filter(filter).map(|p| p.name.len()).max().unwrap_or(0);

    for project in self.file.projects.iter().filter(filter) {
      project.show(&self.source, name_width, &format)?;
    }
    Ok(())
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

pub struct Plan {
  incrs: Vec<(u32, Size)>
}

impl Plan {
  pub fn incrs(&self) -> &Vec<(u32, Size)> { &self.incrs }
}

struct PlanConsider<'s, P: Source, C: Source> {
  prev: &'s Config<P>,
  current: &'s Config<C>,
  incrs: HashMap<u32, Size>
}

impl<'s, P: Source, C: Source> PlanConsider<'s, P, C> {
  fn new(prev: &'s Config<P>, current: &'s Config<C>) -> PlanConsider<'s, P, C> {
    PlanConsider { prev, current, incrs: HashMap::new() }
  }

  pub fn finish_plan(&mut self) -> Result<Plan> {
    let incrs = self
      .current
      .file
      .projects
      .iter()
      .map(|p| if let Some(size) = self.incrs.get(&p.id) { (p.id, *size) } else { (p.id, Size::None) })
      .collect();

    Ok(Plan { incrs })
  }

  pub fn consider(&mut self, kind: &str, path: &str) -> Result<()> {
    for prev_project in &self.prev.file.projects {
      if let Some(cur_project) = self.current.get_project(prev_project.id) {
        let size = cur_project.size(&self.current.file.sizes, kind)?;
        if prev_project.does_cover(path)? {
          let val = self.incrs.entry(prev_project.id).or_insert(Size::None);
          *val = max(*val, size);
        }
      }
    }
    Ok(())
  }

  pub fn consider_deps(&mut self) -> Result<()> {
    // Use a modified Kahn's algorithm to traverse deps in order.
    let mut queue = VecDeque::new();

    let mut dependents: HashMap<u32, HashSet<u32>> = HashMap::new();
    for project in &self.prev.file.projects {
      for dep in &project.depends {
        dependents.entry(*dep).or_insert_with(HashSet::new).insert(project.id);
      }

      if project.depends.is_empty() {
        if let Some(&size) = self.incrs.get(&project.id) {
          queue.push_back((project.id, size));
        } else {
          queue.push_back((project.id, Size::None))
        }
      }
    }

    while let Some((id, size)) = queue.pop_front() {
      let val = self.incrs.entry(id).or_insert(Size::None);
      *val = max(*val, size);

      let depds: Option<HashSet<u32>> = dependents.get(&id).cloned();
      if let Some(depds) = depds {
        for depd in depds {
          dependents.get_mut(&id).unwrap().remove(&depd);
          let val = self.incrs.entry(depd).or_insert(Size::None);
          *val = max(*val, size);

          if dependents.values().all(|ds| !ds.contains(&depd)) {
            queue.push_back((depd, *val));
          }
        }
      }
    }

    Ok(())
  }
}

#[derive(Deserialize, Debug)]
pub struct ConfigFile {
  projects: Vec<Project>,
  #[serde(deserialize_with = "deserialize_sizes", default)]
  sizes: HashMap<String, Size>
}

impl ConfigFile {
  pub fn load(source: &dyn Source) -> Result<ConfigFile> {
    match source.load(CONFIG_FILENAME.as_ref())? {
      Some(data) => ConfigFile::read(data.data()),
      None => Ok(ConfigFile::empty())
    }
  }

  pub fn empty() -> ConfigFile { ConfigFile { projects: Vec::new(), sizes: HashMap::new() } }

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

    // TODO: no circular deps

    Ok(())
  }
}

#[derive(Deserialize, Debug)]
pub struct Project {
  name: String,
  id: u32,
  #[serde(default)]
  covers: Vec<String>,
  #[serde(default)]
  depends: Vec<u32>,
  located: Location
}

impl Project {
  fn annotate(&self, source: &dyn Source) -> Result<AnnotatedMark> {
    Ok(AnnotatedMark::new(self.id, self.name.clone(), self.located.get_mark(source)?))
  }

  pub fn name(&self) -> &str { &self.name }

  fn get_mark(&self, source: &dyn Source) -> Result<MarkedData> {
    self.located.get_mark(source)
  }

  fn size(&self, parent_sizes: &HashMap<String, Size>, kind: &str) -> Result<Size> {
    parent_sizes.get(kind).copied().map(Ok).unwrap_or_else(|| {
      parent_sizes.get("-").copied().map(Ok).unwrap_or_else(|| versio_err!("Can't handle unconventional."))
    })
  }

  pub fn does_cover(&self, path: &str) -> Result<bool> {
    self.covers.iter().fold(Ok(false), |val, cov| {
      if val.is_err() || *val.as_ref().unwrap() {
        return val;
      }
      Ok(Pattern::new(cov)?.matches(path))
    })
  }

  fn check(&self, source: &dyn Source) -> Result<()> {
    self.located.get_mark(source)?;
    for cover in &self.covers {
      if glob(cover)?.count() == 0 {
        return versio_err!("No files covered by \"{}\".", cover);
      }
    }
    Ok(())
  }

  fn show(&self, source: &dyn Source, name_width: usize, format: &ShowFormat) -> Result<()> {
    let mark = self.located.get_mark(source)?;
    if format.version_only {
      println!("{}", mark.value());
    } else if format.wide {
      println!("{:>4}. {:width$} : {}", self.id, self.name, mark.value(), width = name_width);
    } else {
      println!("{:width$} : {}", self.name, mark.value(), width = name_width);
    }
    Ok(())
  }

  fn set_value(&self, source: &dyn Source, val: &str) -> Result<()> {
    let mut mark = self.located.get_mark(source)?;
    mark.write_new_value(val)
  }
}

#[derive(Deserialize, Debug)]
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

#[derive(Deserialize, Debug)]
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

#[derive(Deserialize, Debug)]
struct JsonPicker {
  #[serde(deserialize_with = "deserialize_parts")]
  json: Vec<Part>
}

impl JsonPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { JsonScanner::new(self.json.clone()).scan(data) }
}

#[derive(Deserialize, Debug)]
struct YamlPicker {
  #[serde(deserialize_with = "deserialize_parts")]
  yaml: Vec<Part>
}

impl YamlPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { YamlScanner::new(self.yaml.clone()).scan(data) }
}

#[derive(Deserialize, Debug)]
struct TomlPicker {
  #[serde(deserialize_with = "deserialize_parts")]
  toml: Vec<Part>
}

impl TomlPicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> { TomlScanner::new(self.toml.clone()).scan(data) }
}

#[derive(Deserialize, Debug)]
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
  Ok(data.mark(Mark::make(value, index)?))
}

#[derive(Deserialize, Debug)]
struct FilePicker {}

impl FilePicker {
  pub fn scan(&self, data: NamedData) -> Result<MarkedData> {
    let value = data.data().trim_end().to_string();
    Ok(data.mark(Mark::make(value, 0)?))
  }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
  Major,
  Minor,
  Patch,
  None
}

impl Size {
  fn parts(v: &str) -> Result<[u32; 3]> {
    let parts: Vec<_> = v.split('.').iter().collect();
    if parts.len() != 3 {
      return versio_err!("Not a 3-part version: {}", v);
    }
    Ok([parts[0], parts[1], parts[2]])
  }

  pub fn less_than(v1: &str, v2: &str) -> Result<bool> {
    let p1 = Size::parts(v1)?;
    let p2 = Size::parts(v1)?;

    p1[0] < p2[0] || (p1[0] == p2[0] && (p1[1] < p2[1] || (p1[1] == p2[1] && p1[2] < p2[2])))
  }

  pub fn apply(&self, v: &str) -> {
    let parts = Size::parts(v)?;

    let newv = match self {
      Size::Major => format!("{}.{}.{}", parts[0] + 1, parts[1], parts[2]),
      Size::Minor => format!("{}.{}.{}", parts[0], parts[1] + 1, parts[2]),
      Size::Patch => format!("{}.{}.{}", parts[0], parts[1], parts[2] + 1),
      Size::None => format!("{}.{}.{}", parts[0], parts[1], parts[2]),
    };

    Ok(newv)
  }
}

impl fmt::Display for Size {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      Size::Major => write!(f, "major"),
      Size::Minor => write!(f, "minor"),
      Size::Patch => write!(f, "patch"),
      Size::None => write!(f, "none")
    }
  }
}

impl PartialOrd for Size {
  fn partial_cmp(&self, other: &Size) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Size {
  fn cmp(&self, other: &Size) -> Ordering {
    match self {
      Size::Major => match other {
        Size::Major => Ordering::Equal,
        _ => Ordering::Greater
      },
      Size::Minor => match other {
        Size::Major => Ordering::Less,
        Size::Minor => Ordering::Equal,
        _ => Ordering::Greater
      },
      Size::Patch => match other {
        Size::None => Ordering::Greater,
        Size::Patch => Ordering::Equal,
        _ => Ordering::Less
      },
      Size::None => match other {
        Size::None => Ordering::Equal,
        _ => Ordering::Less
      }
    }
  }
}

fn deserialize_sizes<'de, D: Deserializer<'de>>(desr: D) -> std::result::Result<HashMap<String, Size>, D::Error> {
  struct MapVisitor;

  impl<'de> Visitor<'de> for MapVisitor {
    type Value = HashMap<String, Size>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("a list of sizes") }

    fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
    where
      M: MapAccess<'de>
    {
      let mut result = HashMap::new();
      while let Some((val, keys)) = map.next_entry::<Size, Vec<String>>()? {
        for key in keys {
          if result.contains_key(&key) {
            return Err(de::Error::custom(format!("Duplicated kind \"{}\".", key)));
          }
          result.insert(key, val);
        }
      }

      Ok(result)
    }
  }

  desr.deserialize_map(MapVisitor)
}

#[cfg(test)]
mod test {
  use super::{find_reg_data, ConfigFile, Size};
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

  #[test]
  fn test_sizes() {
    let config = r#"
projects: []
sizes:
  major: [ break ]
  minor: [ feat ]
  patch: [ fix, "-" ]
  none: [ none ]
"#;

    let config = ConfigFile::read(config).unwrap();
    assert_eq!(&Size::Minor, config.sizes.get("feat").unwrap());
    assert_eq!(&Size::Major, config.sizes.get("break").unwrap());
    assert_eq!(&Size::Patch, config.sizes.get("fix").unwrap());
    assert_eq!(&Size::Patch, config.sizes.get("-").unwrap());
    assert_eq!(&Size::None, config.sizes.get("none").unwrap());
  }

  #[test]
  fn test_sizes_dup() {
    let config = r#"
projects: []
sizes:
  major: [ break, feat ]
  minor: [ feat ]
  patch: [ fix, "-" ]
  none: [ none ]
"#;

    assert!(ConfigFile::read(config).is_err());
  }
}
