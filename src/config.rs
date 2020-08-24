//! The configuration and top-level commands for Versio.

use crate::analyze::AnnotatedMark;
use crate::either::IterEither2 as E2;
use crate::errors::{Result, ResultExt};
use crate::git::{Repo, Slice};
use crate::mark::{FilePicker, LinePicker, Picker, ScanningPicker};
use crate::mono::ChangeLog;
use crate::scan::parts::{deserialize_parts, Part};
use crate::state::{CurrentFiles, CurrentState, FilesRead, PickPath, PrevFiles, PrevState, StateRead, StateWrite};
use error_chain::bail;
use glob::{glob_with, MatchOptions, Pattern};
use log::trace;
use regex::{escape, Regex};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, Unexpected, Visitor};
use serde::Deserialize;
use std::borrow::Cow;
use std::cmp::{Ord, Ordering};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::iter::once;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub const CONFIG_FILENAME: &str = ".versio.yaml";

#[derive(Hash, Debug, Eq, PartialEq, Clone)]
pub struct ProjectId {
  id: u32,
  majors: Vec<u32>
}

impl ProjectId {
  pub fn from_id(id: u32) -> ProjectId { ProjectId { id, majors: Vec::new() } }

  fn expand(&self, sub: &SubExtent) -> ProjectId {
    assert!(self.majors.is_empty(), "ProjectId {} expanding.", self);
    ProjectId { id: self.id, majors: sub.majors().to_vec() }
  }
}

impl FromStr for ProjectId {
  type Err = crate::errors::Error;
  fn from_str(v: &str) -> Result<ProjectId> { Ok(ProjectId::from_id(v.parse()?)) }
}

impl fmt::Display for ProjectId {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "[{} {:?}]", self.id, self.majors) }
}

impl<'de> Deserialize<'de> for ProjectId {
  fn deserialize<D: Deserializer<'de>>(desr: D) -> std::result::Result<ProjectId, D::Error> {
    struct ProjectIdVisitor;

    type DeResult<E> = std::result::Result<ProjectId, E>;

    impl<'de> Visitor<'de> for ProjectIdVisitor {
      type Value = ProjectId;

      fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result { formatter.write_str("a project id") }

      fn visit_i8<E: de::Error>(self, v: i8) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_i16<E: de::Error>(self, v: i16) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_i32<E: de::Error>(self, v: i32) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_i64<E: de::Error>(self, v: i64) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_i128<E: de::Error>(self, v: i128) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_u8<E: de::Error>(self, v: u8) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_u16<E: de::Error>(self, v: u16) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_u32<E: de::Error>(self, v: u32) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_u64<E: de::Error>(self, v: u64) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_u128<E: de::Error>(self, v: u128) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_f32<E: de::Error>(self, v: f32) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }
      fn visit_f64<E: de::Error>(self, v: f64) -> DeResult<E> { Ok(ProjectId::from_id(v as u32)) }

      fn visit_str<E: de::Error>(self, v: &str) -> DeResult<E> {
        let v = v.parse().map_err(|_| E::invalid_value(Unexpected::Str(v), &self))?;
        Ok(ProjectId::from_id(v))
      }
    }

    desr.deserialize_any(ProjectIdVisitor)
  }
}

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

impl<S: StateRead> Config<S> {
  pub fn new(state: S, file: ConfigFile) -> Config<S> { Config { state, file } }

  pub fn from_state(state: S) -> Result<Config<S>> {
    let file = ConfigFile::from_read(&state)?;
    Ok(Config::new(state, file))
  }

  pub fn file(&self) -> &ConfigFile { &self.file }
  pub fn state_read(&self) -> &S { &self.state }
  pub fn projects(&self) -> &[Project] { &self.file.projects() }
  pub fn get_project(&self, id: &ProjectId) -> Option<&Project> { self.file.get_project(id) }
  pub fn is_configured(&self) -> Result<bool> { self.state.has_file(CONFIG_FILENAME.as_ref()) }

  pub fn find_unique(&self, name: &str) -> Result<&ProjectId> {
    let mut iter = self.file.projects.iter().filter(|p| p.name.contains(name)).map(|p| p.id());
    let id = iter.next().ok_or_else(|| bad!("No project named {}", name))?;
    if iter.next().is_some() {
      bail!("Multiple projects with name {}", name);
    }
    Ok(id)
  }

  pub fn annotate(&self) -> Result<Vec<AnnotatedMark>> {
    self.file.projects.iter().map(|p| p.annotate(&self.state)).collect()
  }

  pub fn get_value(&self, id: &ProjectId) -> Result<Option<String>> { self.do_project_read(id, |p, s| p.get_value(s)) }

  fn do_project_read<F, T>(&self, id: &ProjectId, f: F) -> Result<Option<T>>
  where
    F: FnOnce(&Project, &S) -> Result<T>
  {
    self.get_project(id).map(|proj| f(proj, &self.state)).transpose()
  }
}

pub struct FsConfig<F: FilesRead> {
  files: F,
  file: ConfigFile
}

impl<'r> FsConfig<PrevFiles<'r>> {
  pub fn slice_to(&self, spec: String) -> Result<FsConfig<PrevFiles<'r>>> {
    FsConfig::from_read(self.files.slice_to(spec)?)
  }

  pub fn from_slice(slice: Slice<'r>) -> Result<FsConfig<PrevFiles<'r>>> {
    FsConfig::from_read(PrevFiles::from_slice(slice)?)
  }
}

impl<F: FilesRead> FsConfig<F> {
  pub fn new(files: F, file: ConfigFile) -> FsConfig<F> { FsConfig { files, file } }

  pub fn from_read(files: F) -> Result<FsConfig<F>> {
    let file = ConfigFile::from_read(&files)?;
    Ok(FsConfig::new(files, file))
  }

  pub fn file(&self) -> &ConfigFile { &self.file }
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
  pub fn from_read<R: FilesRead>(read: &R) -> Result<ConfigFile> {
    ConfigFile::read(&read.read_file(CONFIG_FILENAME.as_ref())?)?.expand(read)
  }

  pub fn from_dir<P: AsRef<Path>>(p: P) -> Result<ConfigFile> {
    let files = CurrentFiles::new(p.as_ref().to_path_buf());
    ConfigFile::from_read(&files)
  }

  fn read(data: &str) -> Result<ConfigFile> {
    let file: ConfigFile = serde_yaml::from_str(data)?;
    file.validate()?;
    Ok(file)
  }

  fn expand<R: FilesRead>(self, read: &R) -> Result<ConfigFile> {
    let iters: Vec<_> = self.projects.into_iter().map(move |p| p.expand(read)).collect::<Result<_>>()?;
    let projects = iters.into_iter().flatten().collect();

    Ok(ConfigFile { projects, ..self })
  }

  pub fn prev_tag(&self) -> &str { self.options.prev_tag() }
  pub fn projects(&self) -> &[Project] { &self.projects }
  pub fn get_project(&self, id: &ProjectId) -> Option<&Project> { self.projects.iter().find(|p| p.id() == id) }
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
      ids.insert(p.id.clone());

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
  tag_prefix: Option<String>,
  #[serde(default)]
  subs: Option<Subs>
}

impl Project {
  pub fn id(&self) -> &ProjectId { &self.id }
  pub fn name(&self) -> &str { &self.name }

  pub fn depends(&self) -> &[ProjectId] { &self.depends }

  pub fn root(&self) -> Option<&String> { self.root.as_ref().and_then(|r| if r == "." { None } else { Some(r) }) }

  fn annotate<S: StateRead>(&self, state: &S) -> Result<AnnotatedMark> {
    Ok(AnnotatedMark::new(self.id.clone(), self.name.clone(), self.get_value(state)?))
  }

  pub fn verify_restrictions(&self, vers: &str) -> Result<()> {
    let major = Size::parts(vers)?[0];
    if let Some(tag_majors) = self.tag_majors() {
      if !tag_majors.contains(&major) {
        bail!("Illegal version {} for restricted project \"{}\" with majors {:?}.", vers, self.id, tag_majors);
      }
    }
    Ok(())
  }

  pub fn change_log(&self) -> Option<Cow<str>> {
    self.change_log.as_ref().map(|change_log| {
      if let Some(root) = self.root() {
        Cow::Owned(PathBuf::from(root).join(change_log).to_string_lossy().to_string())
      } else {
        Cow::Borrowed(change_log.as_str())
      }
    })
  }

  pub fn tag_prefix(&self) -> &Option<String> { &self.tag_prefix }
  pub fn tag_majors(&self) -> Option<&[u32]> { self.located.tag_majors() }

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
      Ok(
        val || {
          let rooted = self.rooted_pattern(cov);
          let result = Pattern::new(&rooted)?.matches_with(path, match_opts());
          trace!("exclude {} match {} vs {}: {}", self.id(), rooted, path, result);
          result
        }
      )
    })?;

    if excludes {
      return Ok(false);
    }

    self.includes.iter().try_fold(false, |val, cov| {
      Ok(
        val || {
          let rooted = self.rooted_pattern(cov);
          let result = Pattern::new(&rooted)?.matches_with(path, match_opts());
          trace!("include {} match {} vs {}: {}", self.id(), rooted, path, result);
          result
        }
      )
    })
  }

  pub fn check<S: StateRead>(&self, state: &S) -> Result<()> {
    // Check that we can find the given mark.
    self.get_value(state)?;

    self.check_excludes()?;

    self.check_prefix()?;

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
      bail!("Proj {} has excludes, but no includes.", self.id);
    }
    Ok(())
  }

  /// Ensure that we don't have excludes without includes.
  fn check_prefix(&self) -> Result<()> {
    if self.located.is_tag() && self.tag_prefix.is_none() {
      bail!("Proj {} has located: tag without tag_prefix, self.id");
    }
    Ok(())
  }

  pub fn get_value<S: StateRead>(&self, read: &S) -> Result<String> {
    self.located.read_value(read, self.root(), self.id())
  }

  pub fn set_value(&self, write: &mut StateWrite, val: &str) -> Result<()> {
    self.located.write_value(write, self.root(), val, &self.id)?;
    self.forward_tag(write, val)
  }

  pub fn forward_tag(&self, write: &mut StateWrite, val: &str) -> Result<()> {
    if let Some(tag_prefix) = &self.tag_prefix {
      let tag = if tag_prefix.is_empty() { format!("v{}", val) } else { format!("{}-v{}", tag_prefix, val) };
      write.tag_head_or_last(tag, &self.id)?;
    }
    Ok(())
  }

  fn rooted_pattern(&self, pat: &str) -> String {
    if let Some(root) = self.root() {
      if root == "." {
        pat.to_string()
      } else {
        PathBuf::from(root).join(pat).to_string_lossy().to_string()
      }
    } else {
      pat.to_string()
    }
  }

  fn expand<R: FilesRead>(self, read: &R) -> Result<impl Iterator<Item = Project>> {
    if let Some(subs) = self.read_subs(read)? {
      Ok(E2::A(subs.into_iter().map(move |sub| Project {
        name: expand_name(&self.name, &sub),
        id: self.id.expand(&sub),
        root: expand_root(self.root(), &sub),
        includes: self.includes.clone(),
        excludes: expand_excludes(&self.excludes, &sub),
        depends: expand_depends(&self.depends, &sub),
        change_log: self.change_log.clone(),
        located: expand_located(&self.located, &sub),
        tag_prefix: self.tag_prefix.clone(),
        subs: None
      })))
    } else {
      Ok(E2::B(once(self)))
    }
  }

  fn read_subs<R: FilesRead>(&self, read: &R) -> Result<Option<Vec<SubExtent>>> {
    if let Some(subs) = &self.subs {
      let pattern = format!("^{}$", escape(subs.dirs()).replace("<>", "([0-9]+)"));
      let dirs = read.subdirs(self.root(), &pattern)?;
      let regex = Regex::new(&pattern)?;
      let extents: Vec<_> = dirs
        .iter()
        .cloned()
        .map(|dir| {
          let caps = regex.captures(&dir).ok_or_else(|| bad!("Unable to capture major from {}", dir))?;
          let major: u32 = caps[1].parse().chain_err(|| format!("Can't parse dir {} as major.", dir))?;
          Ok((dir, major))
        })
        .collect::<Result<_>>()?;
      let largest = extents.iter().map(|(_, m)| *m).max();
      let excludes = dirs.iter().map(|d| format!("{}/**/*", d)).collect();
      let majors = subs.tops().to_vec();

      let list = once(SubExtent { dir: ".".to_string(), majors, largest: dirs.is_empty(), excludes })
        .chain(extents.into_iter().map(|(dir, major)| SubExtent {
          dir,
          majors: vec![major],
          largest: major == *largest.as_ref().unwrap(),
          excludes: Vec::new()
        }))
        .collect::<Vec<_>>();

      Ok(Some(list))
    } else {
      Ok(None)
    }
  }
}

fn expand_name(name: &str, sub: &SubExtent) -> String { format!("{}/{}", name, sub.dir()) }

fn expand_root(root: Option<&String>, sub: &SubExtent) -> Option<String> {
  match root {
    Some(root) => Some(Path::new(root).join(sub.dir()).to_string_lossy().into_owned()),
    None => match sub.dir() {
      "." => None,
      other => Some(Path::new(other).to_string_lossy().into_owned())
    }
  }
}

fn expand_excludes(excludes: &[String], sub: &SubExtent) -> Vec<String> {
  let mut result = excludes.to_vec();
  result.extend_from_slice(sub.excludes());
  result
}

fn expand_depends(depends: &[ProjectId], sub: &SubExtent) -> Vec<ProjectId> {
  if sub.is_largest() {
    depends.to_vec()
  } else {
    Vec::new()
  }
}

fn expand_located(located: &Location, sub: &SubExtent) -> Location {
  if located.is_tags() {
    Location::Tag(TagLocation { tags: TagSpec::MajorTag(MajorTagSpec { majors: sub.majors().to_vec() }) })
  } else {
    located.clone()
  }
}

struct SubExtent {
  dir: String,
  majors: Vec<u32>,
  largest: bool,
  excludes: Vec<String>
}

impl SubExtent {
  pub fn dir(&self) -> &str { &self.dir }
  pub fn excludes(&self) -> &[String] { &self.excludes }
  pub fn is_largest(&self) -> bool { self.largest }
  pub fn majors(&self) -> &[u32] { &self.majors }
}

#[derive(Clone, Deserialize, Debug)]
#[serde(untagged)]
enum Location {
  File(FileLocation),
  Tag(TagLocation)
}

impl Location {
  pub fn is_tags(&self) -> bool {
    match self {
      Location::Tag(_) => true,
      _ => false
    }
  }

  pub fn tag_majors(&self) -> Option<&[u32]> {
    match self {
      Location::File(_) => None,
      Location::Tag(tagl) => tagl.majors()
    }
  }

  pub fn write_value(&self, write: &mut StateWrite, root: Option<&String>, val: &str, id: &ProjectId) -> Result<()> {
    match self {
      Location::File(l) => l.write_value(write, root, val, id),
      Location::Tag(_) => Ok(())
    }
  }

  pub fn read_value<S: StateRead>(&self, read: &S, root: Option<&String>, proj: &ProjectId) -> Result<String> {
    match self {
      Location::File(l) => l.read_value(read, root),
      Location::Tag(l) => l.read_value(read, proj)
    }
  }

  pub fn is_tag(&self) -> bool {
    match self {
      Location::Tag(..) => true,
      _ => false
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

#[derive(Clone, Deserialize, Debug)]
struct TagLocation {
  tags: TagSpec
}

impl TagLocation {
  pub fn majors(&self) -> Option<&[u32]> { self.tags.majors() }

  fn read_value<S: StateRead>(&self, read: &S, proj: &ProjectId) -> Result<String> {
    Ok(read.latest_tag(proj).cloned().unwrap_or_else(|| self.tags.default_value()))
  }
}

#[derive(Clone, Deserialize, Debug)]
#[serde(untagged)]
enum TagSpec {
  DefaultTag(DefaultTagSpec),
  MajorTag(MajorTagSpec)
}

impl TagSpec {
  pub fn majors(&self) -> Option<&[u32]> {
    match self {
      TagSpec::DefaultTag(_) => None,
      TagSpec::MajorTag(mtag) => Some(mtag.majors())
    }
  }

  pub fn default_value(&self) -> String {
    match self {
      TagSpec::DefaultTag(spec) => spec.default.clone(),
      TagSpec::MajorTag(MajorTagSpec { majors }) => {
        let small = majors.iter().min().copied().unwrap_or(0);
        format!("{}.0.0", small)
      }
    }
  }
}

#[derive(Clone, Deserialize, Debug)]
struct DefaultTagSpec {
  default: String
}

#[derive(Clone, Deserialize, Debug)]
struct MajorTagSpec {
  majors: Vec<u32>
}

impl MajorTagSpec {
  pub fn majors(&self) -> &[u32] { &self.majors }
}

#[derive(Clone, Deserialize, Debug)]
struct FileLocation {
  file: String,
  #[serde(flatten)]
  picker: Picker
}

impl FileLocation {
  pub fn write_value(&self, write: &mut StateWrite, root: Option<&String>, val: &str, id: &ProjectId) -> Result<()> {
    let file = self.rooted(root);
    write.update_mark(PickPath::new(file, self.picker.clone()), val.to_string(), id)
  }

  pub fn read_value<S: StateRead>(&self, read: &S, root: Option<&String>) -> Result<String> {
    let file = self.rooted(root);
    let data: String = read.read_file(&file)?;
    self.picker.find(&data).map(|m| m.into_value())
  }

  pub fn rooted(&self, root: Option<&String>) -> PathBuf {
    match root {
      Some(root) => PathBuf::from(root).join(&self.file),
      None => PathBuf::from(&self.file)
    }
  }
}

#[derive(Deserialize, Debug)]
struct Subs {
  #[serde(default)]
  dirs: Option<String>,
  #[serde(default)]
  tops: Option<Vec<u32>>
}

impl Subs {
  fn dirs(&self) -> &str { self.dirs.as_deref().unwrap_or("v<>") }
  fn tops(&self) -> &[u32] { self.tops.as_deref().unwrap_or(&[0, 1]) }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
/// The "size" of the commit is a measure of "how much" to increment a project's version number based on the
/// significance of its changes. There are currently six sizes from smallest to largest:
///
/// - **Empty**: The project was untouched, so the version will not change.
/// - **None**: Non-altering / cosmetic changes were made. The new version of the project is operationally
/// identical to the old version, or close enough to make no difference. The version number will not change.
/// - **Patch**: Bugs were fixed and/or slightly-more-than-cosmetic changes were made; the new version of the
/// project is fully backwards-compatible with the old, and probably operationally similar. The "patch" part of
/// the version number will increment.
/// - **Minor**: New features were added and/or other significant changes were made; the new version of the
/// project is backwards-compatible with the old, but possibly expanded or operationally dissimilar. The "minor"
/// part of the version number will be incremented, and the "patch" part will be reset.
/// - **Major**: Breaking changes were made: anything from pruning APIs to a full restructuring of the code; the
/// new version of the project is incompatible with the the old version, and can't be expected to act as a
/// drop-in replacement. The "major" part of the version number will be incremented, and other parts reset.
/// - **Fail**: A change occured to the project that could not be understood. No changes will be made to any
/// version numbers; in fact, the entire process is prematurely halted.
pub enum Size {
  Fail,
  Major,
  Minor,
  Patch,
  None,
  Empty
}

impl Size {
  fn is_size(v: &str) -> bool { Size::from_str(v).is_ok() }

  fn from_str(v: &str) -> Result<Size> {
    match v {
      "major" => Ok(Size::Major),
      "minor" => Ok(Size::Minor),
      "patch" => Ok(Size::Patch),
      "none" => Ok(Size::None),
      "empty" => Ok(Size::Empty),
      "fail" => Ok(Size::Fail),
      other => err!("Unknown size: {}", other)
    }
  }

  pub fn parts(v: &str) -> Result<[u32; 3]> {
    let parts: Vec<_> = v
      .split('.')
      .map(|p| p.parse())
      .collect::<std::result::Result<_, _>>()
      .chain_err(|| format!("Couldn't split {} into parts", v))?;
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
      Size::Empty => format!("{}.{}.{}", parts[0], parts[1], parts[2]),
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
      Size::Fail => write!(f, "fail"),
      Size::Empty => write!(f, "empty")
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
        Size::None | Size::Empty => Ordering::Greater,
        Size::Patch => Ordering::Equal,
        _ => Ordering::Less
      },
      Size::None => match other {
        Size::Empty => Ordering::Greater,
        Size::None => Ordering::Equal,
        _ => Ordering::Less
      },
      Size::Empty => match other {
        Size::Empty => Ordering::Equal,
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
  use super::{ConfigFile, FileLocation, Location, Picker, Project, ProjectId, ScanningPicker, Size};
  use crate::scan::parts::Part;

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

    assert_eq!(config.projects[0].id, ProjectId::from_id(1));
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
      id: ProjectId::from_id(1),
      root: Some("base".into()),
      includes: vec!["**/*".into()],
      excludes: Vec::new(),
      depends: Vec::new(),
      change_log: None,
      located: Location::File(FileLocation {
        file: "package.json".into(),
        picker: Picker::Json(ScanningPicker::new(vec![Part::Map("version".into())]))
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
      id: ProjectId::from_id(1),
      root: Some("base".into()),
      includes: vec!["**/*".into()],
      excludes: vec!["internal/**/*".into()],
      depends: Vec::new(),
      change_log: None,
      located: Location::File(FileLocation {
        file: "package.json".into(),
        picker: Picker::Json(ScanningPicker::new(vec![Part::Map("version".into())]))
      }),
      tag_prefix: None
    };

    assert!(!proj.does_cover("base/internal/infile.txt").unwrap());
  }

  #[test]
  fn test_excludes_check() {
    let proj = Project {
      name: "test".into(),
      id: ProjectId::from_id(1),
      root: Some("base".into()),
      includes: vec![],
      excludes: vec!["internal/**/*".into()],
      depends: Vec::new(),
      change_log: None,
      located: Location::File(FileLocation {
        file: "package.json".into(),
        picker: Picker::Json(ScanningPicker::new(vec![Part::Map("version".into())]))
      }),
      tag_prefix: None
    };

    assert!(proj.check_excludes().is_err());
  }
}
