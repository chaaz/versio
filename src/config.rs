//! The configuration and top-level commands for Versio.

use crate::analyze::AnnotatedMark;
use crate::errors::{Result, ResultExt};
use crate::git::{Repo, Slice};
use crate::mark::{FilePicker, LinePicker, Picker, ScanningPicker};
use crate::mono::ChangeLog;
use crate::scan::parts::{deserialize_parts, Part};
use crate::state::{CurrentState, PickPath, PrevState, StateRead, StateWrite};
use error_chain::bail;
use glob::{glob_with, MatchOptions, Pattern};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, Unexpected, Visitor};
use serde::Deserialize;
use std::borrow::Cow;
use std::cmp::{Ord, Ordering};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub const CONFIG_FILENAME: &str = ".versio.yaml";

pub type ProjectId = u32;

pub struct Config<S: StateRead> {
  state: S,
  file: ConfigFile
}

impl Config<CurrentState> {
  pub fn prev_tag(&self) -> &str { self.file.prev_tag() }

  pub fn slice<'r>(&self, spec: String, repo: &'r Repo) -> Result<Config<PrevState<'r>>> {
    Config::from_state(self.state.slice(spec, repo)?)
  }

  pub fn slice_to_prev<'r>(&self, repo: &'r Repo) -> Result<Config<PrevState<'r>>> {
    self.slice(self.prev_tag().to_string(), repo)
  }
}

impl<'r> Config<PrevState<'r>> {
  pub fn slice(&self, spec: String) -> Result<Config<PrevState<'r>>> { Config::from_state(self.state.slice(spec)?) }
}

impl<S: StateRead> Config<S> {
  pub fn new(state: S, file: ConfigFile) -> Config<S> { Config { state, file } }

  pub fn from_state(state: S) -> Result<Config<S>> {
    let file = ConfigFile::from_state(&state)?;
    Ok(Config::new(state, file))
  }

  pub fn file(&self) -> &ConfigFile { &self.file }
  pub fn state_read(&self) -> &S { &self.state }
  pub fn projects(&self) -> &[Project] { &self.file.projects() }
  pub fn get_project(&self, id: ProjectId) -> Option<&Project> { self.file.get_project(id) }
  pub fn is_configured(&self) -> Result<bool> { self.state.has_file(CONFIG_FILENAME.as_ref()) }

  pub fn find_unique(&self, name: &str) -> Result<ProjectId> {
    let mut iter = self.file.projects.iter().filter(|p| p.name.contains(name)).map(|p| p.id);
    let id = iter.next().ok_or_else(|| bad!("No project named {}", name))?;
    if iter.next().is_some() {
      bail!("Multiple projects with name {}", name);
    }
    Ok(id)
  }

  pub fn annotate(&self) -> Result<Vec<AnnotatedMark>> {
    self.file.projects.iter().map(|p| p.annotate(&self.state)).collect()
  }
}

#[derive(Deserialize, Debug)]
pub struct ConfigFile {
  #[serde(default)]
  options: Options,
  projects: Vec<Project>,
  #[serde(deserialize_with = "deserialize_sizes", default)]
  sizes: HashMap<String, Size>
}

impl ConfigFile {
  pub fn from_state<S: StateRead>(state: &S) -> Result<ConfigFile> {
    ConfigFile::read(&state.read_file(CONFIG_FILENAME.as_ref())?)
  }

  pub fn from_slice(slice: &Slice) -> Result<ConfigFile> { ConfigFile::read(&PrevState::read(slice, CONFIG_FILENAME)?) }

  pub fn from_dir<P: AsRef<Path>>(p: P) -> Result<ConfigFile> {
    let path = p.as_ref();
    let file = path.join(CONFIG_FILENAME);
    let data = std::fs::read_to_string(&file).chain_err(|| format!("Can't read \"{}\".", file.to_string_lossy()))?;
    ConfigFile::read(&data)
  }

  pub fn empty() -> ConfigFile {
    ConfigFile { options: Default::default(), projects: Vec::new(), sizes: HashMap::new() }
  }

  pub fn read(data: &str) -> Result<ConfigFile> {
    let file: ConfigFile = serde_yaml::from_str(data)?;
    file.validate()?;
    Ok(file)
  }

  pub fn prev_tag(&self) -> &str { self.options.prev_tag() }
  pub fn projects(&self) -> &[Project] { &self.projects }
  pub fn get_project(&self, id: ProjectId) -> Option<&Project> { self.projects.iter().find(|p| p.id == id) }
  pub fn sizes(&self) -> &HashMap<String, Size> { &self.sizes }

  /// Check that IDs are unique, etc.
  fn validate(&self) -> Result<()> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    let mut prefs = HashSet::new();

    for p in &self.projects {
      if ids.contains(&p.id) {
        bail!("id {} is duplicated", p.id);
      }
      ids.insert(p.id);

      if names.contains(&p.name) {
        bail!("name {} is duplicated", p.name);
      }
      names.insert(p.name.clone());

      if let Some(pref) = &p.tag_prefix {
        if prefs.contains(pref) {
          bail!("tag_prefix {} is duplicated", pref);
        }
        if !legal_tag(pref) {
          bail!("illegal tag_prefix \"{}\"", pref);
        }
        prefs.insert(pref.clone());
      }
    }

    // TODO: no circular deps

    Ok(())
  }
}

#[derive(Deserialize, Debug)]
struct Options {
  prev_tag: String
}

impl Default for Options {
  fn default() -> Options { Options { prev_tag: "versio-prev".into() } }
}

impl Options {
  pub fn prev_tag(&self) -> &str { &self.prev_tag }
}

fn legal_tag(prefix: &str) -> bool {
  prefix.is_empty()
    || ((prefix.starts_with('_') || prefix.chars().next().unwrap().is_alphabetic())
      && (prefix.chars().all(|c| c.is_ascii() && (c == '_' || c == '-' || c.is_alphanumeric()))))
}

#[derive(Deserialize, Debug)]
pub struct Project {
  name: String,
  id: ProjectId,
  root: Option<String>,
  #[serde(default)]
  includes: Vec<String>,
  #[serde(default)]
  excludes: Vec<String>,
  #[serde(default)]
  depends: Vec<ProjectId>,
  change_log: Option<String>,
  #[serde(deserialize_with = "deserialize_located")]
  located: Location,
  tag_prefix: Option<String>
}

impl Project {
  pub fn id(&self) -> ProjectId { self.id }
  pub fn name(&self) -> &str { &self.name }
  pub fn root(&self) -> &Option<String> { &self.root }
  pub fn depends(&self) -> &[ProjectId] { &self.depends }

  fn annotate<S: StateRead>(&self, state: &S) -> Result<AnnotatedMark> {
    Ok(AnnotatedMark::new(self.id, self.name.clone(), self.get_value(state)?))
  }

  pub fn change_log(&self) -> Option<Cow<str>> {
    self.change_log.as_ref().map(|change_log| {
      if let Some(root) = &self.root {
        Cow::Owned(PathBuf::from(root).join(change_log).to_string_lossy().to_string())
      } else {
        Cow::Borrowed(change_log.as_str())
      }
    })
  }

  pub fn tag_prefix(&self) -> &Option<String> { &self.tag_prefix }

  pub fn write_change_log(&self, write: &mut StateWrite, cl: &ChangeLog) -> Result<Option<PathBuf>> {
    if cl.is_empty() {
      return Ok(None);
    }

    if let Some(cl_path) = self.change_log().as_ref() {
      let log_path = if let Some(root) = self.root() {
        Path::new(root).join(cl_path.as_ref())
      } else {
        Path::new(cl_path.as_ref()).to_path_buf()
      };
      write.write_file(log_path.clone(), construct_change_log_html(cl)?, self.id())?;
      Ok(Some(log_path))
    } else {
      Ok(None)
    }
  }

  pub fn size(&self, parent_sizes: &HashMap<String, Size>, kind: &str) -> Result<Size> {
    let kind = kind.trim();
    if kind.ends_with('!') {
      return Ok(Size::Major);
    }
    parent_sizes
      .get(kind)
      .copied()
      .map(Ok)
      .unwrap_or_else(|| parent_sizes.get("*").copied().map(Ok).unwrap_or_else(|| err!("Unknown kind \"{}\".", kind)))
  }

  pub fn does_cover(&self, path: &str) -> Result<bool> {
    let excludes = self.excludes.iter().try_fold::<_, _, Result<_>>(false, |val, cov| {
      Ok(val || Pattern::new(&self.rooted_pattern(cov))?.matches_with(path, match_opts()))
    })?;

    if excludes {
      return Ok(false);
    }

    self
      .includes
      .iter()
      .try_fold(false, |val, cov| Ok(val || Pattern::new(&self.rooted_pattern(cov))?.matches_with(path, match_opts())))
  }

  pub fn check<S: StateRead>(&self, state: &S) -> Result<()> {
    // Check that we can find the given mark.
    self.get_value(state)?;

    self.check_excludes()?;

    // Check that each pattern includes at least one file.
    for cov in &self.includes {
      let pattern = self.rooted_pattern(cov);
      if !glob_with(&pattern, match_opts())?.any(|_| true) {
        return err!("No files in proj. {} covered by \"{}\".", self.id, pattern);
      }
    }

    Ok(())
  }

  /// Ensure that we don't have excludes without includes.
  fn check_excludes(&self) -> Result<()> {
    if !self.excludes.is_empty() && self.includes.is_empty() {
      return err!("Proj {} has excludes, but no includes.", self.id);
    }

    Ok(())
  }

  pub fn get_value<S: StateRead + ?Sized>(&self, read: &S) -> Result<String> {
    self.located.read_value(read, &self.root, self.tag_prefix())
  }

  pub fn set_value(&self, write: &mut StateWrite, val: &str) -> Result<()> {
    self.located.write_value(write, &self.root, val, self.id)?;
    self.forward_tag(write, val)
  }

  pub fn forward_tag(&self, write: &mut StateWrite, val: &str) -> Result<()> {
    if let Some(tag_prefix) = &self.tag_prefix {
      let tag = if tag_prefix.is_empty() { format!("v{}", val) } else { format!("{}-v{}", tag_prefix, val) };
      write.tag_head_or_last(tag, self.id)?;
    }
    Ok(())
  }

  fn rooted_pattern(&self, pat: &str) -> String {
    if let Some(root) = &self.root {
      PathBuf::from(root).join(pat).to_string_lossy().to_string()
    } else {
      pat.to_string()
    }
  }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum Location {
  File(FileLocation),
  Tag(TagLocation)
}

impl Location {
  pub fn write_value(&self, write: &mut StateWrite, root: &Option<String>, val: &str, id: ProjectId) -> Result<()> {
    match self {
      Location::File(l) => l.write_value(write, root, val, id),
      Location::Tag(_) => Ok(())
    }
  }

  pub fn read_value<S: StateRead + ?Sized>(
    &self, read: &S, root: &Option<String>, pref: &Option<String>
  ) -> Result<String> {
    match self {
      Location::File(l) => l.read_value(read, root),
      Location::Tag(l) => l.read_value(read, pref)
    }
  }

  #[cfg(test)]
  pub fn picker(&self) -> &Picker {
    match self {
      Location::File(l) => &l.picker,
      _ => panic!("Not a file location")
    }
  }
}

#[derive(Deserialize, Debug)]
struct TagLocation {
  tags: TagSpec
}

impl TagLocation {
  fn read_value<S: StateRead + ?Sized>(&self, read: &S, prefix: &Option<String>) -> Result<String> {
    // TODO: restructure types to make it impossible to have a tags project w/out a tag_prefix
    let prefix = prefix.as_ref().ok_or_else(|| bad!("No tag prefix for tag location."))?;

    // TODO: use TagSpec default instead of Err
    Ok(read.latest_tag(prefix).ok_or_else(|| bad!("No tag found for {}", prefix))?.clone())
  }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum TagSpec {
  DefaultTag(DefaultTagSpec),
  MajorTag(MajorTagSpec)
}

#[derive(Deserialize, Debug)]
struct DefaultTagSpec {
  default: String
}

#[derive(Deserialize, Debug)]
struct MajorTagSpec {
  major: u32
}

#[derive(Clone, Deserialize, Debug)]
struct FileLocation {
  file: String,
  #[serde(flatten)]
  picker: Picker
}

impl FileLocation {
  pub fn write_value(&self, write: &mut StateWrite, root: &Option<String>, val: &str, id: ProjectId) -> Result<()> {
    let file = self.rooted(root);
    write.update_mark(PickPath::new(file, self.picker.clone()), val.to_string(), id)
  }

  pub fn read_value<S: StateRead + ?Sized>(&self, read: &S, root: &Option<String>) -> Result<String> {
    let file = self.rooted(root);
    let data: String = read.read_file(&file)?;
    self.picker.find(&data).map(|m| m.into_value())
  }

  pub fn rooted(&self, root: &Option<String>) -> PathBuf {
    match root {
      Some(root) => PathBuf::from(root).join(&self.file),
      None => PathBuf::from(&self.file)
    }
  }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
  Fail,
  Major,
  Minor,
  Patch,
  None
}

impl Size {
  fn is_size(v: &str) -> bool { Size::from_str(v).is_ok() }

  fn from_str(v: &str) -> Result<Size> {
    match v {
      "major" => Ok(Size::Major),
      "minor" => Ok(Size::Minor),
      "patch" => Ok(Size::Patch),
      "none" => Ok(Size::None),
      "fail" => Ok(Size::Fail),
      other => err!("Unknown size: {}", other)
    }
  }

  pub fn parts(v: &str) -> Result<[u32; 3]> {
    let parts: Vec<_> = v.split('.').map(|p| p.parse()).collect::<std::result::Result<_, _>>()?;
    if parts.len() != 3 {
      return err!("Not a 3-part version: {}", v);
    }
    Ok([parts[0], parts[1], parts[2]])
  }

  pub fn less_than(v1: &str, v2: &str) -> Result<bool> {
    let p1 = Size::parts(v1)?;
    let p2 = Size::parts(v2)?;

    Ok(p1[0] < p2[0] || (p1[0] == p2[0] && (p1[1] < p2[1] || (p1[1] == p2[1] && p1[2] < p2[2]))))
  }

  pub fn apply(self, v: &str) -> Result<String> {
    let parts = Size::parts(v)?;

    let newv = match self {
      Size::Major => format!("{}.{}.{}", parts[0] + 1, 0, 0),
      Size::Minor => format!("{}.{}.{}", parts[0], parts[1] + 1, 0),
      Size::Patch => format!("{}.{}.{}", parts[0], parts[1], parts[2] + 1),
      Size::None => format!("{}.{}.{}", parts[0], parts[1], parts[2]),
      Size::Fail => bail!("'fail' size encountered.")
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
      Size::None => write!(f, "none"),
      Size::Fail => write!(f, "fail")
    }
  }
}

impl PartialOrd for Size {
  fn partial_cmp(&self, other: &Size) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Size {
  fn cmp(&self, other: &Size) -> Ordering {
    match self {
      Size::Fail => match other {
        Size::Fail => Ordering::Equal,
        _ => Ordering::Greater
      },
      Size::Major => match other {
        Size::Fail => Ordering::Less,
        Size::Major => Ordering::Equal,
        _ => Ordering::Greater
      },
      Size::Minor => match other {
        Size::Major | Size::Fail => Ordering::Less,
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

fn deserialize_located<'de, D: Deserializer<'de>>(desr: D) -> std::result::Result<Location, D::Error> {
  struct VecPartSeed;

  impl<'de> DeserializeSeed<'de> for VecPartSeed {
    type Value = Vec<Part>;
    fn deserialize<D>(self, deslr: D) -> std::result::Result<Self::Value, D::Error>
    where
      D: Deserializer<'de>
    {
      deserialize_parts(deslr)
    }
  }

  struct LocatorVisitor;

  impl<'de> Visitor<'de> for LocatorVisitor {
    type Value = Location;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("a version location") }

    fn visit_map<V>(self, mut map: V) -> std::result::Result<Self::Value, V::Error>
    where
      V: MapAccess<'de>
    {
      let mut file: Option<String> = None;
      let mut pattern: Option<String> = None;
      let mut parts: Option<Vec<Part>> = None;
      let mut tags: Option<TagSpec> = None;
      let mut code: Option<String> = None;

      while let Some(key) = map.next_key::<String>()? {
        match key.as_str() {
          "file" => {
            file = Some(map.next_value()?);
          }
          "tags" => {
            tags = Some(map.next_value()?);
          }
          "json" | "yaml" | "toml" | "xml" => {
            code = Some(key);
            parts = Some(map.next_value_seed(VecPartSeed)?);
          }
          "pattern" => {
            pattern = Some(map.next_value()?);
          }
          other => return Err(de::Error::invalid_value(Unexpected::Str(other), &"a location key"))
        }
      }

      if let Some(file) = file {
        if tags.is_some() {
          Err(de::Error::custom("cant have both 'file' and 'tags' for location"))
        } else if pattern.is_none() && parts.is_none() {
          Ok(Location::File(FileLocation { file, picker: Picker::File(FilePicker {}) }))
        } else if let Some(pattern) = pattern {
          if parts.is_some() {
            Err(de::Error::custom("can't have both 'pattern' and parts field"))
          } else {
            Ok(Location::File(FileLocation { file, picker: Picker::Line(LinePicker::new(pattern)) }))
          }
        } else {
          let parts = parts.unwrap();
          let loc = match code.unwrap().as_str() {
            "json" => Location::File(FileLocation { file, picker: Picker::Json(ScanningPicker::new(parts)) }),
            "yaml" => Location::File(FileLocation { file, picker: Picker::Yaml(ScanningPicker::new(parts)) }),
            "toml" => Location::File(FileLocation { file, picker: Picker::Toml(ScanningPicker::new(parts)) }),
            "xml" => Location::File(FileLocation { file, picker: Picker::Xml(ScanningPicker::new(parts)) }),
            other => return Err(de::Error::custom(format!("unrecognized part {}", other)))
          };
          Ok(loc)
        }
      } else if let Some(tags) = tags {
        Ok(Location::Tag(TagLocation { tags }))
      } else {
        Err(de::Error::custom("must have 'file' or 'tags' for location"))
      }
    }
  }

  desr.deserialize_map(LocatorVisitor)
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
      let mut using_angular = false;

      while let Some(val) = map.next_key::<String>()? {
        match val.as_str() {
          val if Size::is_size(val) => {
            let size = Size::from_str(val).unwrap();
            let keys: Vec<String> = map.next_value()?;
            for key in keys {
              if result.contains_key(&key) {
                return Err(de::Error::custom(format!("Duplicated kind \"{}\".", key)));
              }
              result.insert(key, size);
            }
          }
          "use_angular" => {
            using_angular = map.next_value()?;
          }
          _ => return Err(de::Error::custom(format!("Unrecognized sizes key \"{}\".", val)))
        }
      }

      // Based on the angular standard:
      // https://github.com/angular/angular.js/blob/main/DEVELOPERS.md#-git-commit-guidelines
      if using_angular {
        insert_if_missing(&mut result, "feat", Size::Minor);
        insert_if_missing(&mut result, "fix", Size::Patch);
        insert_if_missing(&mut result, "docs", Size::None);
        insert_if_missing(&mut result, "style", Size::None);
        insert_if_missing(&mut result, "refactor", Size::None);
        insert_if_missing(&mut result, "perf", Size::None);
        insert_if_missing(&mut result, "test", Size::None);
        insert_if_missing(&mut result, "chore", Size::None);
        insert_if_missing(&mut result, "build", Size::None);
      }

      Ok(result)
    }
  }

  desr.deserialize_map(MapVisitor)
}

fn insert_if_missing(result: &mut HashMap<String, Size>, key: &str, val: Size) {
  if !result.contains_key(key) {
    result.insert(key.to_string(), val);
  }
}

fn match_opts() -> MatchOptions { MatchOptions { require_literal_separator: true, ..Default::default() } }

fn construct_change_log_html(cl: &ChangeLog) -> Result<String> {
  let mut output = String::new();
  output.push_str("<html>\n");
  output.push_str("<body>\n");

  output.push_str("<ul>\n");
  for (pr, size) in cl.entries() {
    if !pr.commits().iter().any(|c| c.included()) {
      continue;
    }
    if pr.number() == 0 {
      // "PR zero" is the top-level set of commits.
      output.push_str(&format!("  <li>Other commits : {} </li>\n", size));
    } else {
      output.push_str(&format!("  <li>PR {} : {} </li>\n", pr.number(), size));
    }
    output.push_str("  <ul>\n");
    for c /* (oid, msg, size, appl, dup) */ in pr.commits().iter().filter(|c| c.included()) {
      let symbol = if c.duplicate() {
        "(dup) "
      } else if c.applies() {
        ""
      } else {
        "(not appl) "
      };
      output.push_str(&format!("    <li>{}commit {} ({}) : {}</li>\n", symbol, &c.oid()[.. 7], c.size(), c.message()));
    }
    output.push_str("  </ul>\n");
  }
  output.push_str("</ul>\n");

  output.push_str("</body>\n");
  output.push_str("</html>\n");

  Ok(output)
}

#[cfg(test)]
mod test {
  use super::{ConfigFile, FileLocation, LinePicker, Location, Picker, Project, ScanningPicker, Size};
  use crate::scan::parts::Part;
  use std::marker::PhantomData;

  #[test]
  fn test_both_file_and_tags() {
    // TODO: more tests like this
    let data = r#"
projects:
  - name: everything
    id: 1
    includes: ["**/*"]
    located:
      tags:
        default: "1.0.0"
      file: "toplevel.json""#;

    assert!(ConfigFile::read(data).is_err())
  }

  #[test]
  fn test_scan() {
    let data = r#"
projects:
  - name: everything
    id: 1
    includes: ["**/*"]
    located:
      file: "toplevel.json"
      json: "version"

  - name: project1
    id: 2
    includes: ["project1/**/*"]
    located:
      file: "project1/Cargo.toml"
      toml: "version"

  - name: "combined a and b"
    id: 3
    includes: ["nested/project_a/**/*", "nested/project_b/**/*"]
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
    assert_eq!("line", config.projects[2].located.picker().picker_type());
  }

  #[test]
  fn test_validate() {
    let config = r#"
projects:
  - name: p1
    id: 1
    includes: ["**/*"]
    located: { file: f1 }

  - name: project1
    id: 1
    includes: ["**/*"]
    located: { file: f2 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_names() {
    let config = r#"
projects:
  - name: p1
    id: 1
    includes: ["**/*"]
    located: { file: f1 }

  - name: p1
    id: 2
    includes: ["**/*"]
    located: { file: f2 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_illegal_prefix() {
    let config = r#"
projects:
  - name: p1
    id: 1
    tag_prefix: "ixth*&o"
    includes: ["**/*"]
    located: { file: f1 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_unascii_prefix() {
    let config = r#"
projects:
  - name: p1
    id: 1
    tag_prefix: "ixthïo"
    includes: ["**/*"]
    located: { file: f1 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_prefix() {
    let config = r#"
projects:
  - name: p1
    id: 1
    tag_prefix: proj
    includes: ["**/*"]
    located: { file: f1 }

  - name: p2
    id: 2
    tag_prefix: proj
    includes: ["**/*"]
    located: { file: f2 }
    "#;

    assert!(ConfigFile::read(config).is_err());
  }

  #[test]
  fn test_validate_ok() {
    let config = r#"
projects:
  - name: p1
    id: 1
    tag_prefix: "_proj1-abc"
    includes: ["**/*"]
    located: { file: f1 }

  - name: p2
    id: 2
    tag_prefix: proj2
    includes: ["**/*"]
    located: { file: f2 }
    "#;

    assert!(ConfigFile::read(config).is_ok());
  }

  #[test]
  fn test_find_reg() {
    let data = r#"
This is text.
Current rev is "v1.2.3" because it is."#;

    let mark = LinePicker::find_reg_data(data, "v(\\d+\\.\\d+\\.\\d+)").unwrap();
    assert_eq!("1.2.3", mark.value());
    assert_eq!(32, mark.start());
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

  #[test]
  fn test_include_w_root() {
    let proj = Project {
      name: "test".into(),
      id: 1,
      root: Some("base".into()),
      includes: vec!["**/*".into()],
      excludes: Vec::new(),
      depends: Vec::new(),
      change_log: None,
      located: Location::File(FileLocation {
        file: "package.json".into(),
        picker: Picker::Json(ScanningPicker { _scan: PhantomData, parts: vec![Part::Map("version".into())] })
      }),
      tag_prefix: None
    };

    assert!(proj.does_cover("base/somefile.txt").unwrap());
    assert!(!proj.does_cover("outerfile.txt").unwrap());
  }

  #[test]
  fn test_exclude_w_root() {
    let proj = Project {
      name: "test".into(),
      id: 1,
      root: Some("base".into()),
      includes: vec!["**/*".into()],
      excludes: vec!["internal/**/*".into()],
      depends: Vec::new(),
      change_log: None,
      located: Location::File(FileLocation {
        file: "package.json".into(),
        picker: Picker::Json(ScanningPicker { _scan: PhantomData, parts: vec![Part::Map("version".into())] })
      }),
      tag_prefix: None
    };

    assert!(!proj.does_cover("base/internal/infile.txt").unwrap());
  }

  #[test]
  fn test_excludes_check() {
    let proj = Project {
      name: "test".into(),
      id: 1,
      root: Some("base".into()),
      includes: vec![],
      excludes: vec!["internal/**/*".into()],
      depends: Vec::new(),
      change_log: None,
      located: Location::File(FileLocation {
        file: "package.json".into(),
        picker: Picker::Json(ScanningPicker { _scan: PhantomData, parts: vec![Part::Map("version".into())] })
      }),
      tag_prefix: None
    };

    assert!(proj.check_excludes().is_err());
  }
}
