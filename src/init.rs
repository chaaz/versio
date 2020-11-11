//! Simple implementation of the `init` command.

use crate::config::CONFIG_FILENAME;
use crate::errors::{Error, Result};
use crate::mark::Mark;
use crate::scan::{find_reg_data, JsonScanner, Scanner, TomlScanner, XmlScanner};
use error_chain::bail;
use ignore::WalkBuilder;
use log::warn;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

pub fn init(max_depth: u16) -> Result<()> {
  if Path::new(CONFIG_FILENAME).exists() {
    bail!("Versio is already initialized.");
  }

  let walk = WalkBuilder::new("./").max_depth(Some(max_depth as usize)).build();
  let projs: Vec<_> = walk
    .filter_map(|r| r.map_err(Error::from).and_then(|e| find_project(e.file_name(), e.path())).transpose())
    .collect::<Result<_>>()?;

  if projs.is_empty() {
    println!("No projects found.");
  }
  write_yaml(&projs)?;
  append_ignore()?;
  Ok(())
}

fn find_project(name: &OsStr, file: &Path) -> Result<Option<ProjSummary>> {
  let fname = match name.to_str() {
    Some(n) => n,
    None => return Ok(None)
  };

  if fname == "package.json" {
    let name = extract_name(file, |d| JsonScanner::new("name").find(&d))?;
    let dir = file.parent().unwrap();
    return Ok(Some(ProjSummary::new_file(name, dir.to_string_lossy(), "package.json", "json", "version", &["npm"])));
  }

  if fname == "Cargo.toml" {
    let name = extract_name(file, |d| TomlScanner::new("package.name").find(&d))?;
    let dir = file.parent().unwrap();
    let mut proj =
      ProjSummary::new_file(name, dir.to_string_lossy(), "Cargo.toml", "toml", "package.version", &["cargo"]);
    proj.hook("post_write", "cargo fetch");
    return Ok(Some(proj));
  }

  if fname == "go.mod" {
    let dir = file.parent().unwrap();
    let is_subdir = if let Some(parent) = dir.parent() { parent.join("go.mod").exists() } else { false };
    if !is_subdir {
      let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("project");
      return Ok(Some(ProjSummary::new_tags(name, dir.to_string_lossy(), true, &["go"])));
    }
  }

  if fname == "pom.xml" {
    let name = extract_name(file, |d| XmlScanner::new("project.artifactId").find(&d))?;
    let dir = file.parent().unwrap().to_string_lossy();
    return Ok(Some(ProjSummary::new_file(name, dir, "pom.xml", "xml", "project.version", &["mvn"])));
  }

  if fname == "setup.py" {
    let name_reg = r#"name *= *['"]([^'"]*)['"]"#;
    let version_reg = r#"version *= *['"](\d+\.\d+\.\d+)['"]"#;
    let name = extract_name(file, |d| find_reg_data(&d, &name_reg))?;
    let dir = file.parent().unwrap().to_string_lossy();
    return Ok(Some(ProjSummary::new_file(name, dir, "setup.py", "pattern", version_reg, &["pip"])));
  }

  if file.is_dir()
    && file
      .read_dir()?
      .filter_map(|e| e.ok().and_then(|e| e.file_name().into_string().ok()))
      .any(|n| n.ends_with("*.tf"))
  {
    return Ok(Some(ProjSummary::new_tags("terraform", file.to_string_lossy(), false, &["terraform"])));
  }

  if fname == "Dockerfile" {
    let dir = file.parent().unwrap();
    return Ok(Some(ProjSummary::new_tags("docker", dir.to_string_lossy(), false, &["docker"])));
  }

  if let Some(ps) = add_gemspec(fname, file)? {
    return Ok(Some(ps));
  }

  Ok(None)
}

fn add_gemspec(fname: &str, file: &Path) -> Result<Option<ProjSummary>> {
  let spec_suffix = ".gemspec";
  if let Some(fname_pref) = fname.strip_suffix(spec_suffix) {
    let name_reg = r#"spec\.name *= *['"]([^'"]*)['"]"#;
    let version_reg = r#"spec\.version *= *(\S*)"#;
    let name = extract_name(file, |d| find_reg_data(&d, &name_reg))?;
    let mut vers = extract_name(file, |d| find_reg_data(&d, &version_reg))?;
    let dir = file.parent().unwrap();
    let dirn = dir.to_string_lossy();

    if (vers.starts_with('"') && vers.ends_with('"')) || (vers.starts_with('\'') && vers.ends_with('\'')) {
      vers = vers[1 .. vers.len() - 1].to_string();
    }

    if Mark::new(vers.clone(), 0).validate_version().is_ok() {
      // Sometimes, the version is in the specfile.
      let version_reg = r#"spec\.version *= *['"](\d+\.\d+\.\d+)['"]"#;
      return Ok(Some(ProjSummary::new_file(name, dirn, fname, "pattern", version_reg, &["gem"])));
    } else if vers.ends_with("::VERSION") {
      // But other times, the version is in the gem itself i.e. 'MyGem::VERSION'. Search the standard place.
      let vers_file = Path::new("lib").join(fname_pref).join("version.rb");
      if dir.join(&vers_file).exists() {
        let version_reg = r#"VERSION *= *['"](\d+\.\d+\.\d+)['"]"#;
        let vfn = vers_file.to_string_lossy();
        return Ok(Some(ProjSummary::new_file(name, dirn, vfn, "pattern", version_reg, &["gem"])));
      } else {
        warn!("Couldn't find VERSION file \"{}\". Please edit the .versio.yaml file.", vers_file.to_string_lossy());
        return Ok(Some(ProjSummary::new_file(name, dirn, "EDIT_ME", "pattern", "EDIT_ME", &["gem"])));
      }
    } else {
      // Still other times, it's too tough to find.
      warn!("Couldn't find version in \"{}\" from \"{}\". Please edit the .versio.yaml file.", fname, vers);
      return Ok(Some(ProjSummary::new_file(name, dir.to_string_lossy(), "EDIT_ME", "pattern", "EDIT_ME", &["gem"])));
    }
  }

  Ok(None)
}

fn extract_name<F: FnOnce(String) -> Result<Mark>>(file: &Path, find: F) -> Result<String> {
  std::fs::read_to_string(file).map_err(|e| e.into()).and_then(find).map(|mark| mark.value().to_string())
}

fn write_yaml(projs: &[ProjSummary]) -> Result<()> {
  let yaml = generate_yaml(projs);
  Ok(std::fs::write(CONFIG_FILENAME, &yaml)?)
}

fn generate_yaml(projs: &[ProjSummary]) -> String {
  let mut yaml = String::new();
  yaml.push_str("options:\n");
  yaml.push_str("  prev_tag: \"versio-prev\"\n");
  yaml.push('\n');

  if !projs.is_empty() {
    yaml.push_str("projects:\n");
  }

  let mut prefixes = HashSet::new();
  for (id, proj) in projs.iter().enumerate() {
    yaml.push_str(&format!("  - name: \"{}\"\n", proj.name()));
    if let Some(root) = proj.root() {
      yaml.push_str(&format!("    root: \"{}\"\n", root));
    }
    yaml.push_str(&format!("    id: {}\n", id + 1));
    yaml.push_str(&format!("    tag_prefix: \"{}\"\n", proj.tag_prefix(projs.len(), &mut prefixes)));
    if !proj.labels().is_empty() {
      if proj.labels().len() == 1 {
        yaml.push_str(&format!("    labels: {}\n", &proj.labels()[0]));
      } else {
        yaml.push_str("    labels:\n");
        for l in proj.labels() {
          yaml.push_str(&format!("      - {}\n", l));
        }
      }
    }
    yaml.push_str("    version:\n");
    proj.append_version(&mut yaml);

    if !proj.hooks().is_empty() {
      let mut hooks: Vec<_> = proj.hooks().iter().collect();
      hooks.sort_by(|(k1, _), (k2, _)| k1.cmp(k2));
      yaml.push_str("    hooks:\n");
      for (k, v) in hooks {
        yaml.push_str(&format!("      {}: '{}'\n", k, yaml_escape_single(v)));
      }
    }

    if proj.subs() {
      yaml.push_str("    subs: {}\n");
    }
    yaml.push('\n');
  }

  yaml.push_str("sizes:\n");
  yaml.push_str("  use_angular: true\n");
  yaml.push_str("  fail: [\"*\"]\n");

  yaml
}

fn append_ignore() -> Result<()> {
  let mut file = OpenOptions::new().create(true).append(true).open(".gitignore")?;
  Ok(file.write_all(b"/.versio-paused\n")?)
}

struct ProjSummary {
  name: String,
  labels: Vec<String>,
  root: String,
  subs: bool,
  version: VersionSummary,
  hooks: HashMap<String, String>
}

impl ProjSummary {
  pub fn new_file(
    name: impl ToString, root: impl ToString, file: impl ToString, file_type: impl ToString, parts: impl ToString,
    labels: &[impl ToString]
  ) -> ProjSummary {
    ProjSummary {
      name: name.to_string(),
      root: root.to_string(),
      subs: false,
      labels: labels.iter().map(|s| s.to_string()).collect(),
      version: VersionSummary::File(FileVersionSummary::new(
        file.to_string(),
        file_type.to_string(),
        parts.to_string()
      )),
      hooks: HashMap::new()
    }
  }

  pub fn hook(&mut self, key: &str, val: &str) -> &mut ProjSummary {
    self.hooks.insert(key.into(), val.into());
    self
  }

  pub fn new_tags(name: impl ToString, root: impl ToString, subs: bool, labels: &[impl ToString]) -> ProjSummary {
    ProjSummary {
      name: name.to_string(),
      root: root.to_string(),
      subs,
      labels: labels.iter().map(|s| s.to_string()).collect(),
      version: VersionSummary::Tag(TagVersionSummary::new()),
      hooks: HashMap::new()
    }
  }

  fn name(&self) -> &str { &self.name }
  fn labels(&self) -> &[String] { &self.labels }
  fn hooks(&self) -> &HashMap<String, String> { &self.hooks }

  fn root(&self) -> Option<&str> {
    if &self.root == "." {
      None
    } else if self.root.starts_with("./") {
      Some(&self.root[2 ..])
    } else {
      Some(&self.root)
    }
  }

  fn subs(&self) -> bool { self.subs }

  fn tag_prefix(&self, projs_len: usize, prefixes: &mut HashSet<String>) -> String {
    let prefix = if projs_len == 1 { "".into() } else { tag_sanitize(&self.name) };

    let prefix = if prefixes.contains(&prefix) {
      let best = (2 .. 1000).map(|d| format!("{}_{}", prefix, d)).find(|v| !prefixes.contains(v.as_str()));
      best.ok_or_else(|| bad!("All prefixes {}_2 - {}_1000 are taken", prefix, prefix)).unwrap()
    } else {
      prefix
    };

    prefixes.insert(prefix.clone());
    prefix
  }

  fn append_version(&self, yaml: &mut String) {
    match &self.version {
      VersionSummary::File(f) => f.append(yaml),
      VersionSummary::Tag(t) => t.append(yaml)
    }
  }
}

enum VersionSummary {
  File(FileVersionSummary),
  Tag(TagVersionSummary)
}

struct FileVersionSummary {
  file: String,
  file_type: String,
  parts: String
}

impl FileVersionSummary {
  pub fn new(file: String, file_type: String, parts: String) -> FileVersionSummary {
    FileVersionSummary { file, file_type, parts }
  }

  pub fn append(&self, yaml: &mut String) {
    yaml.push_str(&format!("      file: \"{}\"\n", self.file));
    if self.file_type == "pattern" {
      yaml.push_str(&format!("      {}: '{}'\n", self.file_type, yaml_escape_single(&self.parts)));
    } else {
      yaml.push_str(&format!("      {}: \"{}\"\n", self.file_type, self.parts));
    }
  }
}

fn yaml_escape_single(val: &str) -> String { val.replace("'", "''") }

struct TagVersionSummary {}

impl TagVersionSummary {
  pub fn new() -> TagVersionSummary { TagVersionSummary {} }

  pub fn append(&self, yaml: &mut String) {
    yaml.push_str("      tags:\n");
    yaml.push_str("        default: \"0.0.0\"\n");
  }
}

fn tag_sanitize(name: &str) -> String {
  // match the logic of `config::legal_tag`
  let mut prefix: String =
    name.chars().filter(|c| c.is_ascii() && (*c == '_' || *c == '-' || c.is_alphanumeric())).collect();

  if prefix.is_empty() {
    return "_".into();
  }

  let char0 = prefix.chars().next().unwrap();
  if char0 != '_' && !char0.is_alphabetic() {
    prefix = format!("_{}", prefix);
  }

  prefix
}
