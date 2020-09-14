//! A monorepo can read and alter the current state of all projects.

use crate::analyze::{analyze, Analysis};
use crate::config::{Config, ConfigFile, FsConfig, Project, ProjectId, Size};
use crate::either::{IterEither2 as E2, IterEither3 as E3};
use crate::errors::Result;
use crate::git::{Auth, CommitInfoBuf, FromTag, FromTagBuf, FullPr, GithubInfo, Repo};
use crate::github::{changes, line_commits_head, Changes};
use crate::state::{CurrentState, OldTags, PrevFiles, PrevTagMessage, StateRead, StateWrite};
use crate::vcs::VcsLevel;
use chrono::{DateTime, FixedOffset};
use error_chain::bail;
use log::trace;
use serde::Deserialize;
use std::cmp::{max, Ordering};
use std::collections::{HashMap, HashSet, VecDeque};
use std::convert::identity;
use std::iter::{empty, once};
use std::path::{Path, PathBuf};

const USER_PREFS_DIR: &str = ".versio";
const USER_PREFS_FILE: &str = "prefs.toml";

pub struct Mono {
  current: Config<CurrentState>,
  next: StateWrite,
  last_commits: HashMap<ProjectId, String>,
  repo: Repo,
  user_prefs: UserPrefs
}

impl Mono {
  pub fn here(vcs: VcsLevel) -> Result<Mono> { Mono::open(".", vcs) }

  pub fn open<P: AsRef<Path>>(dir: P, vcs: VcsLevel) -> Result<Mono> {
    let repo = Repo::open(dir.as_ref(), vcs)?;
    let root = repo.working_dir()?;

    // A little dance to construct a state and config.
    let file = ConfigFile::from_dir(root)?;
    let projects = file.projects().iter();
    let old_tags = find_old_tags(projects, file.prev_tag(), &repo)?;
    let state = CurrentState::new(root.to_path_buf(), old_tags);
    let current = Config::new(state, file);

    let last_commits = find_last_commits(&current, &repo)?;
    let next = StateWrite::new();

    let user_prefs = read_env_prefs()?;

    Ok(Mono { user_prefs, current, next, last_commits, repo })
  }

  pub fn check_branch(&self) -> std::result::Result<(), (String, String)> {
    if let Ok(branch_name) = self.repo.branch_name() {
      if let Some(cfg_name) = self.current.branch() {
        if branch_name != cfg_name {
          Err((cfg_name.clone(), branch_name.clone()))
        } else {
          Ok(())
        }
      } else {
        Ok(())
      }
    } else {
      Ok(())
    }
  }

  pub fn commit(&mut self, advance_prev: bool) -> Result<()> {
    self.next.commit(
      &self.repo,
      self.current.prev_tag(),
      &self.last_commits,
      &self.current.old_tags().current(),
      advance_prev
    )
  }

  pub fn get_project(&self, id: &ProjectId) -> Result<&Project> {
    self.current.get_project(id).ok_or_else(|| bad!("No such project {}", id))
  }

  pub fn diff(&self) -> Result<Analysis> {
    let prev_config = self.current.slice_to_prev(&self.repo)?;

    let curt_annotate = self.current.annotate()?;
    let prev_annotate = prev_config.annotate()?;

    Ok(analyze(prev_annotate, curt_annotate))
  }

  pub fn config(&self) -> &Config<CurrentState> { &self.current }
  pub fn repo(&self) -> &Repo { &self.repo }

  pub fn set_by_id(&mut self, id: &ProjectId, val: &str) -> Result<()> {
    self.do_project_write(id, move |p, n| p.set_value(n, val))
  }

  pub fn set_by_name(&mut self, name: &str, val: &str) -> Result<()> {
    let id = self.current.find_unique(name)?.clone();
    self.set_by_id(&id, val)
  }

  pub fn set_by_only(&mut self, val: &str) -> Result<()> {
    if self.current.projects().len() != 1 {
      bail!("No solo project.");
    }
    let id = self.current.projects().get(0).unwrap().id().clone();
    self.set_by_id(&id, val)
  }

  pub fn forward_by_id(&mut self, id: &ProjectId, val: &str) -> Result<()> {
    self.do_project_write(id, move |p, n| p.forward_tag(n, val))
  }

  pub fn write_changelog(&mut self, id: &ProjectId, changelog: &Changelog) -> Result<Option<PathBuf>> {
    self.do_project_write(id, move |p, n| p.write_changelog(n, changelog))
  }

  fn do_project_write<F, T>(&mut self, id: &ProjectId, f: F) -> Result<T>
  where
    F: FnOnce(&Project, &mut StateWrite) -> Result<T>
  {
    let proj = self.current.get_project(id).ok_or_else(|| bad!("No such project {}", id))?;
    f(proj, &mut self.next)
  }

  pub fn check(&self) -> Result<()> {
    for project in self.current.projects() {
      project.check(self.current.state_read())?;
    }
    Ok(())
  }

  pub fn keyed_files<'a>(&'a self) -> Result<impl Iterator<Item = Result<(String, String)>> + 'a> {
    let changes = self.changes()?;
    let prs = changes.into_groups().into_iter().map(|(_, v)| v).filter(|pr| !pr.best_guess());

    let mut vec = Vec::new();
    for pr in prs {
      vec.push(pr_keyed_files(&self.repo, pr));
    }

    Ok(vec.into_iter().flatten())
  }

  pub fn build_plan(&self) -> Result<Plan> {
    let mut plan = PlanBuilder::create(&self.repo, self.current.file(), self.user_prefs.auth())?;

    // Consider the grouped, unsquashed commits to determine project sizing and changelogs.
    for pr in self.changes()?.groups().values() {
      plan.start_pr(pr)?;
      for commit in pr.included_commits() {
        plan.start_commit(commit.clone())?;
        for file in commit.files() {
          plan.start_file(file)?;
          plan.finish_file()?;
        }
        plan.finish_commit()?;
      }
      plan.finish_pr()?;
    }

    // Some projects might depend on other projects.
    plan.handle_deps()?;

    // Sort projects by earliest closed date, mark duplicate commits.
    plan.sort_and_dedup()?;

    Ok(plan.build())
  }

  pub fn changes(&self) -> Result<Changes> {
    let base = FromTagBuf::new(self.current.prev_tag().to_string(), true);
    let head = self.repo.branch_name()?.to_string();
    changes(&self.user_prefs.auth, &self.repo, base, head)
  }
}

/// Read the user preferences file, with some values override with environment variables.
fn read_env_prefs() -> Result<UserPrefs> {
  read_user_prefs().map(|mut prefs| {
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
      prefs.auth_mut().set_github_token(Some(token))
    }
    prefs
  })
}

fn read_user_prefs() -> Result<UserPrefs> {
  let homefile = dirs::home_dir().map(|h| h.join(USER_PREFS_DIR).join(USER_PREFS_FILE));
  let homefile = match homefile {
    Some(f) => f,
    None => return Ok(Default::default())
  };
  if !homefile.exists() {
    return Ok(Default::default());
  }

  let user_prefs: UserPrefs = toml::from_str(&std::fs::read_to_string(homefile)?)?;
  Ok(user_prefs)
}

#[derive(Deserialize, Debug)]
struct UserPrefs {
  auth: Auth
}

impl Default for UserPrefs {
  fn default() -> UserPrefs { UserPrefs { auth: Default::default() } }
}

impl UserPrefs {
  fn auth(&self) -> &Auth { &self.auth }
  fn auth_mut(&mut self) -> &mut Auth { &mut self.auth }
}

/// Find the last covering commit ID, if any, for each current project.
fn find_last_commits(current: &Config<CurrentState>, repo: &Repo) -> Result<HashMap<ProjectId, String>> {
  let prev_spec = current.prev_tag();
  let mut last_commits = LastCommitBuilder::create(repo, &current)?;

  // Consider the in-line commits to determine the last commit (if any) for each project.
  for commit in line_commits_head(repo, FromTag::new(prev_spec, true))? {
    last_commits.start_line_commit(&commit)?;
    for file in commit.files() {
      last_commits.start_line_file(file)?;
      last_commits.finish_line_file()?;
    }
    last_commits.finish_line_commit()?;
  }

  let result = last_commits.build();
  trace!("Found last commits: {:?}", result);
  result
}

fn pr_keyed_files<'a>(repo: &'a Repo, pr: FullPr) -> impl Iterator<Item = Result<(String, String)>> + 'a {
  let head_oid = match pr.head_oid() {
    Some(oid) => *oid,
    None => return E3::C(empty())
  };

  let iter = repo.commits_between(pr.base_oid(), head_oid, false).map(move |cmts| {
    cmts
      .filter_map(move |cmt| match cmt {
        Ok(cmt) => {
          if pr.has_exclude(&cmt.id()) {
            None
          } else {
            match cmt.files() {
              Ok(files) => {
                let kind = cmt.kind();
                Some(E2::A(files.map(move |f| Ok((kind.clone(), f)))))
              }
              Err(e) => Some(E2::B(once(Err(e))))
            }
          }
        }
        Err(e) => Some(E2::B(once(Err(e))))
      })
      .flatten()
  });

  match iter {
    Ok(iter) => E3::A(iter),
    Err(e) => E3::B(once(Err(e)))
  }
}

pub struct Plan {
  incrs: HashMap<ProjectId, (Size, Changelog)>, // proj ID, incr size, changelog
  ineffective: Vec<LoggedPr>                    // PRs that didn't apply to any project
}

impl Plan {
  pub fn incrs(&self) -> &HashMap<ProjectId, (Size, Changelog)> { &self.incrs }
  pub fn ineffective(&self) -> &[LoggedPr] { &self.ineffective }
}

pub struct Changelog {
  entries: Vec<(LoggedPr, Size)>
}

impl Changelog {
  pub fn empty() -> Changelog { Changelog { entries: Vec::new() } }
  pub fn entries(&self) -> &[(LoggedPr, Size)] { &self.entries }
  pub fn add_entry(&mut self, pr: LoggedPr, size: Size) { self.entries.push((pr, size)); }
  pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

pub struct LoggedPr {
  number: u32,
  title: String,
  closed_at: DateTime<FixedOffset>,
  commits: Vec<LoggedCommit>,
  url: Option<String>
}

impl LoggedPr {
  pub fn capture(pr: &FullPr, url: Option<String>) -> LoggedPr {
    LoggedPr {
      number: pr.number(),
      title: pr.title().to_string(),
      closed_at: *pr.closed_at(),
      commits: Vec::new(),
      url
    }
  }

  pub fn number(&self) -> u32 { self.number }
  pub fn title(&self) -> &str { &self.title }
  pub fn closed_at(&self) -> &DateTime<FixedOffset> { &self.closed_at }
  pub fn commits(&self) -> &[LoggedCommit] { &self.commits }
  pub fn url(&self) -> &Option<String> { &self.url }
}

pub struct LoggedCommit {
  oid: String,
  summary: String,
  message: String,
  size: Size,
  applies: bool,
  duplicate: bool,
  url: Option<String>
}

impl LoggedCommit {
  pub fn new(oid: String, summary: String, message: String, size: Size, url: Option<String>) -> LoggedCommit {
    LoggedCommit { oid, summary, message, size, applies: false, duplicate: false, url }
  }

  pub fn applies(&self) -> bool { self.applies }
  pub fn duplicate(&self) -> bool { self.duplicate }
  pub fn included(&self) -> bool { self.applies && !self.duplicate }
  pub fn oid(&self) -> &str { &self.oid }
  pub fn summary(&self) -> &str { &self.summary }
  pub fn message(&self) -> &str { &self.message }
  pub fn size(&self) -> Size { self.size }
  pub fn url(&self) -> &Option<String> { &self.url }
}

struct PlanBuilder<'s> {
  on_pr_sizes: HashMap<ProjectId, LoggedPr>,
  on_ineffective: Option<LoggedPr>,
  on_commit: Option<CommitInfoBuf>,
  prev: Slicer<'s>,
  current: &'s ConfigFile,
  incrs: HashMap<ProjectId, (Size, Changelog)>, // proj ID, incr size, changelog
  ineffective: Vec<LoggedPr>,                   // PRs that didn't apply to any project
  github_info: Option<GithubInfo>
}

impl<'s> PlanBuilder<'s> {
  fn create(repo: &'s Repo, current: &'s ConfigFile, auth: &Auth) -> Result<PlanBuilder<'s>> {
    let prev = Slicer::init(repo);
    let github_info = repo.github_info(auth).ok();
    let builder = PlanBuilder {
      on_pr_sizes: HashMap::new(),
      on_ineffective: None,
      on_commit: None,
      prev,
      current,
      incrs: HashMap::new(),
      ineffective: Vec::new(),
      github_info
    };
    Ok(builder)
  }

  pub fn start_pr(&mut self, pr: &FullPr) -> Result<()> {
    trace!(
      "planning PR {} with {}.",
      pr.number(),
      self.github_info.as_ref().map(|gh| gh.repo_name()).unwrap_or("<no gh>")
    );
    let url = self
      .github_info
      .as_ref()
      .map(|gh| format!("https://github.com/{}/{}/pull/{}", gh.owner_name(), gh.repo_name(), pr.number()));
    self.on_pr_sizes =
      self.current.projects().iter().map(|p| (p.id().clone(), LoggedPr::capture(pr, url.clone()))).collect();
    self.on_ineffective = Some(LoggedPr::capture(pr, url));
    Ok(())
  }

  pub fn finish_pr(&mut self) -> Result<()> {
    trace!("planning PR done.");
    let mut found = false;
    for (proj_id, logged_pr) in self.on_pr_sizes.drain() {
      let (size, changelog) = self.incrs.entry(proj_id).or_insert((Size::Empty, Changelog::empty()));
      let pr_size = logged_pr.commits.iter().filter(|c| c.applies).map(|c| c.size).max();
      if let Some(pr_size) = pr_size {
        found = true;
        *size = max(*size, pr_size);
        changelog.add_entry(logged_pr, pr_size);
      }
    }

    let ineffective = self.on_ineffective.take().unwrap();
    if !found {
      self.ineffective.push(ineffective);
    }

    Ok(())
  }

  pub fn start_commit(&mut self, commit: CommitInfoBuf) -> Result<()> {
    let id = commit.id().to_string();
    let kind = commit.kind().to_string();
    let summary = commit.summary().to_string();
    let msg = commit.message().to_string();
    self.on_commit = Some(commit);
    self.prev.slice_to(FromTagBuf::new(id.clone(), false))?;

    let url = self
      .github_info
      .as_ref()
      .map(|gh| format!("https://github.com/{}/{}/commit/{}", gh.owner_name(), gh.repo_name(), id));
    trace!("  planning commit {} at {}.", id, url.as_deref().unwrap_or("<no url>"));

    for (proj_id, logged_pr) in &mut self.on_pr_sizes {
      if let Some(cur_project) = self.current.get_project(proj_id) {
        let size = cur_project.size(&self.current.sizes(), &kind)?;
        logged_pr.commits.push(LoggedCommit::new(id.clone(), summary.clone(), msg.clone(), size, url.clone()));
      }
    }

    Ok(())
  }

  pub fn finish_commit(&mut self) -> Result<()> {
    trace!("  planning commit done.");
    Ok(())
  }

  pub fn start_file(&mut self, path: &str) -> Result<()> {
    trace!("    planning file {}.", path);
    let commit = self.on_commit.as_ref().ok_or_else(|| bad!("Not on a commit"))?;
    let commit_id = commit.id();

    for prev_project in self.prev.file()?.projects() {
      if let Some(logged_pr) = self.on_pr_sizes.get_mut(&prev_project.id()) {
        trace!("      vs current project {}.", prev_project.id());
        if prev_project.does_cover(path)? {
          let LoggedCommit { applies, .. } = logged_pr.commits.iter_mut().find(|c| c.oid == commit_id).unwrap();
          *applies = true;
          trace!("        covered.");
        } else {
          trace!("        not covered.");
        }
      } else {
        trace!("      project {} doesn't currently exist.", prev_project.id());
      }
    }
    Ok(())
  }

  pub fn finish_file(&mut self) -> Result<()> { Ok(()) }

  pub fn handle_deps(&mut self) -> Result<()> {
    // Use a modified Kahn's algorithm to traverse deps in order.
    let mut queue: VecDeque<(ProjectId, Size)> = VecDeque::new();

    let mut dependents: HashMap<ProjectId, HashSet<ProjectId>> = HashMap::new();
    for project in self.current.projects() {
      for dep in project.depends() {
        dependents.entry(dep.clone()).or_insert_with(HashSet::new).insert(project.id().clone());
      }

      if project.depends().is_empty() {
        if let Some((size, ..)) = self.incrs.get(&project.id()) {
          queue.push_back((project.id().clone(), *size));
        } else {
          queue.push_back((project.id().clone(), Size::Empty))
        }
      }
    }

    while let Some((id, size)) = queue.pop_front() {
      let val = &mut self.incrs.entry(id.clone()).or_insert((Size::Empty, Changelog::empty())).0;
      *val = max(*val, size);

      let depds: Option<HashSet<ProjectId>> = dependents.get(&id).cloned();
      if let Some(depds) = depds {
        for depd in depds {
          dependents.get_mut(&id).unwrap().remove(&depd);
          let val = &mut self.incrs.entry(depd.clone()).or_insert((Size::Empty, Changelog::empty())).0;
          *val = max(*val, size);

          if dependents.values().all(|ds| !ds.contains(&depd)) {
            queue.push_back((depd, *val));
          }
        }
      }
    }

    Ok(())
  }

  pub fn sort_and_dedup(&mut self) -> Result<()> {
    for (.., changelog) in self.incrs.values_mut() {
      changelog.entries.sort_by_key(|(pr, _)| *pr.closed_at());

      let mut seen_commits = HashSet::new();
      for (pr, size) in &mut changelog.entries {
        for LoggedCommit { oid, duplicate, .. } in &mut pr.commits {
          if seen_commits.contains(oid) {
            *duplicate = true;
          }
          seen_commits.insert(oid.clone());
        }
        *size = pr.commits().iter().filter(|c| c.included()).map(|c| c.size).max().unwrap_or(Size::Empty);
      }
    }
    Ok(())
  }

  pub fn build(self) -> Plan { Plan { incrs: self.incrs, ineffective: self.ineffective } }
}

struct LastCommitBuilder<'s, C: StateRead> {
  on_line_commit: Option<String>,
  last_commits: HashMap<ProjectId, String>,
  prev: Slicer<'s>,
  current: &'s Config<C>
}

impl<'s, C: StateRead> LastCommitBuilder<'s, C> {
  fn create(repo: &'s Repo, current: &'s Config<C>) -> Result<LastCommitBuilder<'s, C>> {
    let prev = Slicer::init(repo);
    let builder = LastCommitBuilder { on_line_commit: None, last_commits: HashMap::new(), prev, current };
    Ok(builder)
  }

  pub fn start_line_commit(&mut self, commit: &CommitInfoBuf) -> Result<()> {
    let id = commit.id().to_string();
    self.on_line_commit = Some(id.clone());
    self.prev.slice_to(FromTagBuf::new(id, false))?;
    Ok(())
  }

  pub fn finish_line_commit(&mut self) -> Result<()> { Ok(()) }

  pub fn start_line_file(&mut self, path: &str) -> Result<()> {
    let commit_id = self.on_line_commit.as_ref().ok_or_else(|| bad!("Not on a line commit"))?;

    for prev_project in self.prev.file()?.projects() {
      let proj_id = prev_project.id();
      if self.current.get_project(proj_id).is_some()
        && prev_project.does_cover(path)?
        && !self.last_commits.contains_key(proj_id)
      {
        self.last_commits.insert(proj_id.clone(), commit_id.clone());
      }
    }
    Ok(())
  }

  pub fn finish_line_file(&mut self) -> Result<()> { Ok(()) }

  pub fn build(self) -> Result<HashMap<ProjectId, String>> { Ok(self.last_commits) }
}

enum Slicer<'r> {
  Orig(&'r Repo),
  Slice(FsConfig<PrevFiles<'r>>)
}

impl<'r> Slicer<'r> {
  pub fn init(repo: &'r Repo) -> Slicer<'r> { Slicer::Orig(repo) }

  pub fn file(&self) -> Result<&ConfigFile> {
    match self {
      Slicer::Slice(fsc) => Ok(fsc.file()),
      _ => err!("Slicer not sliced")
    }
  }

  pub fn slice_to(&mut self, id: FromTagBuf) -> Result<()> {
    *self = Slicer::Slice(match self {
      Slicer::Orig(repo) => FsConfig::from_slice(repo.slice(id))?,
      Slicer::Slice(fsc) => fsc.slice_to(id)?
    });
    Ok(())
  }
}

fn find_old_tags<'s, I: Iterator<Item = &'s Project>>(projects: I, prev_tag: &str, repo: &Repo) -> Result<OldTags> {
  let mut by_proj_oid = HashMap::new(); // Map<proj_id, Map<oid, Vec<tag>>>
  let mut proj_ids = HashSet::new();

  for proj in projects {
    proj_ids.insert(proj.id().clone());
    for fnmatch in tag_fnmatches(proj) {
      trace!("Searching tags for proj {} matching \"{}\".", proj.id(), fnmatch);
      for tag in repo.tag_names(Some(fnmatch.as_str()))?.iter().filter_map(identity) {
        let oid = repo.revparse_oid(FromTag::new(&format!("{}^{{}}", tag), false))?;
        trace!("Found proj {} tag {} at {}.", proj.id(), tag, oid);
        let by_id = by_proj_oid.entry(proj.id().clone()).or_insert_with(HashMap::new);
        by_id.entry(oid).or_insert_with(Vec::new).push(tag.to_string());
      }
    }
  }

  let mut current = HashMap::new();
  for commit_oid in repo.commits_to_head(FromTag::new(prev_tag, true), false)?.map(|c| c.map(|c| c.id())) {
    let commit_oid = commit_oid?;
    by_proj_oid.retain(|proj_id, by_id| {
      if let Some(tags) = by_id.remove(&commit_oid) {
        let mut versions = tags_to_versions(&tags);
        versions.sort_unstable_by(version_sort);
        current.insert(proj_id.clone(), versions[0].clone());
        false
      } else {
        true
      }
    });
  }

  let prev = pull_from_annotation(repo, prev_tag)?;
  fill_from_prev(&prev, &proj_ids, &mut current)?;

  let old_tags = OldTags::new(current, prev);
  trace!("Found old tags: {:?}", old_tags);
  Ok(old_tags)
}

fn pull_from_annotation(repo: &Repo, prev_tag: &str) -> Result<HashMap<ProjectId, String>> {
  repo
    .annotation_of(prev_tag)
    .map(|anno| serde_json::from_str::<PrevTagMessage>(&anno))
    .transpose()
    .map_err(|e| e.into())
    .map(|o| o.unwrap_or_default().into_versions())
}

fn fill_from_prev(
  prev: &HashMap<ProjectId, String>, proj_ids: &HashSet<ProjectId>, current: &mut HashMap<ProjectId, String>
) -> Result<()> {
  for id in proj_ids {
    if !current.contains_key(id) {
      if let Some(tag) = prev.get(id) {
        current.insert(id.clone(), tag.clone());
      }
    }
  }
  Ok(())
}

/// Construct a fnmatch pattern for a project that can be used to retrieve the project's tags.
///
/// This will return an empty iterator if the project doesn't have a tag_prefix. The resulting patterns are
/// usable by both `Repository::tag_names` and as a git fetch refspec `refs/tags/{pattern}`.
fn tag_fnmatches(proj: &Project) -> impl Iterator<Item = String> + '_ {
  let majors = proj.tag_majors();

  let majors_v = if let Some(majors) = majors {
    E2::A(majors.iter().map(|major| format!("v{}.*", major)))
  } else {
    E2::B(once("v*".to_string()))
  };

  let tag_prefix = proj.tag_prefix().as_ref().map(|p| p.as_str());
  match tag_prefix {
    None => E3::A(empty()),
    Some("") => E3::B(majors_v),
    Some(pref) => E3::C(majors_v.map(move |major_v| format!("{}-{}", pref, major_v)))
  }
}

fn tags_to_versions(tags: &[String]) -> Vec<String> {
  tags
    .iter()
    .map(|tag| {
      let v = tag.rfind('-').map(|d| d + 1).unwrap_or(0);
      tag[v + 1 ..].to_string()
    })
    .filter(|v| Size::parts(v).is_ok())
    .collect()
}

#[allow(clippy::ptr_arg)]
fn version_sort(a: &String, b: &String) -> Ordering {
  let p1 = Size::parts(a);
  let p2 = Size::parts(b);

  if let Ok(p1) = p1 {
    if let Ok(p2) = p2 {
      if p1[0] < p2[0] {
        Ordering::Greater
      } else if p1[0] > p2[0] {
        Ordering::Less
      } else if p1[1] < p2[1] {
        Ordering::Greater
      } else if p1[1] > p2[1] {
        Ordering::Less
      } else if p1[2] < p2[2] {
        Ordering::Greater
      } else if p1[2] > p2[2] {
        Ordering::Less
      } else {
        Ordering::Equal
      }
    } else {
      Ordering::Greater
    }
  } else if p2.is_ok() {
    Ordering::Less
  } else {
    Ordering::Equal
  }
}
