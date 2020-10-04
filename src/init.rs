//! Simple implementation of the `init` command.

use crate::config::CONFIG_FILENAME;
use crate::either::IterEither2 as E2;
use crate::errors::Result;
use crate::mark::Mark;
use crate::scan::{find_reg_data, JsonScanner, Scanner, TomlScanner, XmlScanner};
use error_chain::bail;
use log::trace;
use log::warn;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::iter::once;
use std::path::Path;

pub fn init(max_depth: u16) -> Result<()> {
  if Path::new(CONFIG_FILENAME).exists() {
    bail!("Versio is already initialized.");
  }

  let projs = find_projects(Path::new("."), 0, max_depth)?;
  if projs.is_empty() {
    println!("No projects found.");
  }
  write_yaml(&projs)?;
  append_ignore()?;
  Ok(())
}

fn append_ignore() -> Result<()> {
  let mut file = OpenOptions::new().create(true).append(true).open(".gitignore")?;
  Ok(file.write_all(b"/.versio-paused\n")?)
}

fn find_projects(dir: &Path, depth: u16, max_depth: u16) -> Result<Vec<ProjSummary>> {
  trace!("Finding projects in {}", dir.to_string_lossy());
  if depth > max_depth {
    return Ok(Vec::new());
  }

  let here = find_projects_in(dir);
  let there = find_projects_under(dir, depth, max_depth);

  here.chain(there).collect()
}

fn find_projects_under(dir: &Path, depth: u16, max_depth: u16) -> impl Iterator<Item = Result<ProjSummary>> {
  let dir = match dir.read_dir() {
    Ok(dir) => dir,
    Err(e) => return E2::A(once(Err(e.into())))
  };

  E2::B(
    dir
      .filter_map(|e| e.ok())
      .filter(|e| e.file_name().into_string().map(|n| !n.starts_with('.')).unwrap_or(false))
      .filter(|e| e.file_type().map(|f| f.is_dir()).unwrap_or(false))
      .map(|e| e.path())
      .flat_map(move |p| match find_projects(&p, depth + 1, max_depth) {
        Ok(proj) => E2::A(proj.into_iter().map(Ok)),
        Err(e) => E2::B(once(Err(e)))
      })
  )
}

fn extract_name<F: FnOnce(String) -> Result<Mark>>(dir: &Path, file: impl AsRef<Path>, find: F) -> Result<String> {
  std::fs::read_to_string(&dir.join(file.as_ref()))
    .map_err(|e| e.into())
    .and_then(find)
    .map(|mark| mark.value().to_string())
}

fn find_projects_in(dir: &Path) -> impl Iterator<Item = Result<ProjSummary>> {
  let mut summaries = Vec::new();

  if dir.join("package.json").exists() {
    let name = try_iter!(extract_name(dir, "package.json", |d| JsonScanner::new("name").find(&d)));
    summaries.push(Ok(ProjSummary::new_file(name, dir.to_string_lossy(), "package.json", "json", "version", &["npm"])));
  }

  if dir.join("Cargo.toml").exists() {
    let name = try_iter!(extract_name(dir, "Cargo.toml", |d| TomlScanner::new("package.name").find(&d)));
    let mut proj =
      ProjSummary::new_file(name, dir.to_string_lossy(), "Cargo.toml", "toml", "package.version", &["cargo"]);
    proj.hook("post_write", "cargo fetch");
    summaries.push(Ok(proj));
  }

  if dir.join("go.mod").exists() {
    let mut is_subdir = false;
    if let Some(parent) = dir.parent() {
      if parent.join("go.mod").exists() {
        is_subdir = true;
      }
    }
    if !is_subdir {
      let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("project");
      summaries.push(Ok(ProjSummary::new_tags(name, dir.to_string_lossy(), true, &["go"])));
    }
  }

  if dir.join("pom.xml").exists() {
    let name = try_iter!(extract_name(dir, "pom.xml", |d| XmlScanner::new("project.artifactId").find(&d)));
    summaries.push(Ok(ProjSummary::new_file(
      name,
      dir.to_string_lossy(),
      "pom.xml",
      "xml",
      "project.version",
      &["mvn"]
    )));
  }

  if dir.join("setup.py").exists() {
    let name_reg = r#"name *= *['"]([^'"]*)['"]"#;
    let version_reg = r#"version *= *['"](\d+\.\d+\.\d+)['"]"#;
    let name = try_iter!(extract_name(dir, "setup.py", |d| find_reg_data(&d, &name_reg)));
    summaries.push(Ok(ProjSummary::new_file(
      name,
      dir.to_string_lossy(),
      "setup.py",
      "pattern",
      version_reg,
      &["pip"]
    )));
  }

  if try_iter!(dir.read_dir())
    .filter_map(|e| e.ok().and_then(|e| e.file_name().into_string().ok()))
    .any(|n| n.ends_with("*.tf"))
  {
    summaries.push(Ok(ProjSummary::new_tags("terraform", dir.to_string_lossy(), false, &["terraform"])));
  }

  if dir.join("Dockerfile").exists() {
    summaries.push(Ok(ProjSummary::new_tags("docker", dir.to_string_lossy(), false, &["docker"])));
  }

  try_iter!(add_gemspecs(dir, &mut summaries));

  E2::B(summaries.into_iter())
}

fn add_gemspecs(dir: &Path, summaries: &mut Vec<Result<ProjSummary>>) -> Result<()> {
  let spec_suffix = ".gemspec";
  for spec_file in dir
    .read_dir()?
    .filter_map(|e| e.ok().and_then(|e| e.file_name().into_string().ok()))
    .filter(|n| n.ends_with(spec_suffix))
  {
    let name_reg = r#"spec\.name *= *['"]([^'"]*)['"]"#;
    let version_reg = r#"spec\.version *= *(\S*)"#;
    let name = extract_name(dir, &spec_file, |d| find_reg_data(&d, &name_reg))?;
    let mut vers = extract_name(dir, &spec_file, |d| find_reg_data(&d, &version_reg))?;

    if (vers.starts_with('"') && vers.ends_with('"')) || (vers.starts_with('\'') && vers.ends_with('\'')) {
      vers = vers[1 .. vers.len() - 1].to_string();
    }

    if Mark::new(vers.clone(), 0).validate_version().is_ok() {
      // Sometimes, the version is in the specfile.
      let version_reg = r#"spec\.version *= *['"](\d+\.\d+\.\d+)['"]"#;
      summaries.push(Ok(ProjSummary::new_file(
        name,
        dir.to_string_lossy(),
        spec_file,
        "pattern",
        version_reg,
        &["gem"]
      )));
    } else if vers.ends_with("::VERSION") {
      // But other times, the version is in the gem itself i.e. 'MyGem::VERSION'. Search the standard place.
      let vers_file = Path::new("lib").join(&spec_file[.. spec_file.len() - spec_suffix.len()]).join("version.rb");
      if dir.join(&vers_file).exists() {
        let version_reg = r#"VERSION *= *['"](\d+\.\d+\.\d+)['"]"#;
        summaries.push(Ok(ProjSummary::new_file(
          name,
          dir.to_string_lossy(),
          vers_file.to_string_lossy(),
          "pattern",
          version_reg,
          &["gem"]
        )));
      } else {
        warn!("Couldn't find VERSION file \"{}\". Please edit the .versio.yaml file.", vers_file.to_string_lossy());
        summaries.push(Ok(ProjSummary::new_file(
          name,
          dir.to_string_lossy(),
          "EDIT_ME",
          "pattern",
          "EDIT_ME",
          &["gem"]
        )));
      }
    } else {
      // Still other times, it's too tough to find.
      warn!("Couldn't find version in \"{}\" from \"{}\". Please edit the .versio.yaml file.", spec_file, vers);
      summaries.push(Ok(ProjSummary::new_file(name, dir.to_string_lossy(), "EDIT_ME", "pattern", "EDIT_ME", &["gem"])));
    }
  }

  Ok(())
}

fn write_yaml(projs: &[ProjSummary]) -> Result<()> {
  let yaml = generate_yaml(projs);
  Ok(std::fs::write(CONFIG_FILENAME, &yaml)?)
}

fn generate_yaml(projs: &[ProjSummary]) -> String {
  let mut yaml = String::new();
  yaml.push_str("options:\n");
  yaml.push_str("  prev_tag: \"versio-prev\"\n");
  yaml.push_str("\n");

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
    yaml.push_str("\n");
  }

  yaml.push_str("sizes:\n");
  yaml.push_str("  use_angular: true\n");
  yaml.push_str("  fail: [\"*\"]\n");

  yaml
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
