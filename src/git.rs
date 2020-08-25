//! Interactions with git.

use crate::config::CONFIG_FILENAME;
use crate::either::IterEither2 as E2;
use crate::errors::{Result, ResultExt};
use crate::vcs::VcsLevel;
use chrono::{DateTime, FixedOffset};
use error_chain::bail;
use git2::build::CheckoutBuilder;
use git2::string_array::StringArray;
use git2::{
  AnnotatedCommit, AutotagOption, Blob, Commit, Cred, Diff, DiffOptions, FetchOptions, Index, Object, ObjectType, Oid,
  PushOptions, Reference, ReferenceType, Remote, RemoteCallbacks, Repository, RepositoryOpenFlags, RepositoryState,
  ResetType, Signature, Status, StatusOptions, Time
};
use log::{error, info, trace, warn};
use regex::Regex;
use std::cell::RefCell;
use std::cmp::min;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{stdout, Write};
use std::iter::empty;
use std::path::{Path, PathBuf};

pub struct Repo {
  vcs: GitVcsLevel
}

impl Repo {
  // All member methods of `Repo` should do the "best thing" for the current VCS level. For example,
  // `commits_to_head` will: if None, return an empty list; if Local, return all commits found from the given
  // spec to HEAD; if Remote or Smart, first fetch the spec from the remote (merging into the current directory
  // if on the current branch), then return everything from the fetched commit to HEAD.
  //
  // If a method deals with git OIDs in either in their argument or return, then that method will return an
  // `Err` if the VCS level is None: OIDs are an opaque artifact of git, and you really need some sort of
  // repository to do anything with them. `commits_between` and `get_oid_head` fall into this category. On the
  // other hand, methods that only deal with refspecs or other symbolic references such as `commits_to_head` or
  // `tag_names` must work at any VCS level.
  //
  // Methods that obviously only work at certain levels will return an `Err` if executed outside those levels.
  // For example, `find_github_info` returns the GitHub data that enables smart remote scanning, so it only
  // returns successfully at the Smart level.

  /// Return the vcs level that this repository can support.
  pub fn detect<P: AsRef<Path>>(path: P) -> Result<VcsLevel> {
    let flags = RepositoryOpenFlags::empty();
    let repo = Repository::open_ext(path, flags, empty::<&OsStr>());
    let repo = match repo {
      Err(_) => return Ok(VcsLevel::None),
      Ok(repo) => repo
    };

    let branch_name = find_branch_name(&repo)?;
    if let Ok(remote_name) = find_remote_name(&repo, &branch_name) {
      if find_github_info(&repo, &remote_name).is_ok() {
        Ok(VcsLevel::Smart)
      } else {
        Ok(VcsLevel::Remote)
      }
    } else {
      Ok(VcsLevel::Local)
    }
  }

  pub fn open<P: AsRef<Path>>(path: P, vcs: VcsLevel) -> Result<Repo> {
    if vcs == VcsLevel::None {
      let root = find_root_blind(path)?;
      return Ok(Repo { vcs: GitVcsLevel::None { root } });
    }

    let flags = RepositoryOpenFlags::empty();
    let repo = Repository::open_ext(path, flags, empty::<&OsStr>())?;
    let branch_name = find_branch_name(&repo)?;

    if vcs == VcsLevel::Local {
      return Ok(Repo { vcs: GitVcsLevel::Local { repo, branch_name } });
    }

    let remote_name = find_remote_name(&repo, &branch_name)?;
    let fetches = RefCell::new(HashMap::new());
    let root = repo.workdir().ok_or_else(|| bad!("Repo has no working dir."))?.to_path_buf();

    Ok(Repo { vcs: GitVcsLevel::from(vcs, root, repo, branch_name, remote_name, fetches) })
  }

  pub fn working_dir(&self) -> Result<&Path> {
    match &self.vcs {
      GitVcsLevel::None { root } => Ok(root),
      GitVcsLevel::Local { repo, .. } | GitVcsLevel::Remote { repo, .. } | GitVcsLevel::Smart { repo, .. } => {
        repo.workdir().ok_or_else(|| bad!("Repo has no working dir"))
      }
    }
  }

  pub fn revparse_oid(&self, spec: &str) -> Result<String> {
    let repo = self.repo()?;
    verify_current(repo).chain_err(|| "Can't complete get.")?;
    Ok(repo.revparse_single(spec)?.id().to_string())
  }

  pub fn slice(&self, refspec: String) -> Slice { Slice { repo: self, refspec } }

  pub fn tag_names(&self, pattern: Option<&str>) -> Result<IterString> {
    match &self.vcs {
      GitVcsLevel::None { .. } => Ok(IterString::Empty),
      GitVcsLevel::Local { repo, .. } => Ok(IterString::Git(repo.tag_names(pattern)?)),
      GitVcsLevel::Remote { repo, remote_name, .. } | GitVcsLevel::Smart { repo, remote_name, .. } => {
        let fetch_pat = if let Some(pat) = pattern { pat } else { "*" };
        let specs: &[&str] = &[&format!("refs/tags/{pat}:refs/tags/{pat}", pat = fetch_pat)];
        safe_fetch(repo, remote_name, specs, false).chain_err(|| format!("Can't fetch tags \"{}\"", fetch_pat))?;
        Ok(IterString::Git(repo.tag_names(pattern)?))
      }
    }
  }

  pub fn github_info(&self) -> Result<GithubInfo> { find_github_info(self.repo()?, self.remote_name()?) }

  /// Return all commits as in `git rev-list from_sha..to_sha`, along with the earliest time in that range.
  ///
  /// `from` may be any legal target of `rev-parse`.
  pub fn commits_between_buf(&self, from_sha: &str, to_oid: Oid) -> Result<Option<(Vec<CommitInfoBuf>, Time)>> {
    let repo = self.repo()?;
    let mut revwalk = repo.revwalk()?;
    revwalk.hide(repo.revparse_single(from_sha)?.id())?;
    revwalk.push(to_oid)?;

    revwalk.try_fold::<_, _, Result<Option<(Vec<CommitInfoBuf>, Time)>>>(None, |v, oid| {
      let oid = oid?;
      let commit = repo.find_commit(oid)?;
      let ctime = commit.time();
      if let Some((mut datas, time)) = v {
        datas.push(CommitInfoBuf::extract(repo, &commit)?);
        Ok(Some((datas, min(time, ctime))))
      } else {
        let datas = vec![CommitInfoBuf::extract(repo, &commit)?];
        Ok(Some((datas, ctime)))
      }
    })
  }

  /// Return all commits as in `git rev-list from_sha..to_sha`.
  ///
  /// `from` may be any legal target of `rev-parse`.
  pub fn commits_between(
    &self, from: &str, to_oid: Oid, incl_from: bool
  ) -> Result<impl Iterator<Item = Result<CommitInfo<'_>>> + '_> {
    let repo = self.repo()?;
    let mut revwalk = repo.revwalk()?;
    if incl_from {
      let commit = repo.revparse_single(from)?.peel_to_commit()?;
      for pid in commit.parent_ids() {
        revwalk.hide(pid)?;
      }
    } else {
      revwalk.hide(repo.revparse_single(from)?.id())?;
    }
    revwalk.push(to_oid)?;

    Ok(revwalk.map(move |id| Ok(CommitInfo::new(repo, repo.find_commit(id?)?))))
  }

  /// Return all commits as in `git rev-list from_sha..HEAD`.
  ///
  /// `from` may be any legal target of `rev-parse`.
  pub fn commits_to_head(
    &self, from: &str, incl_from: bool
  ) -> Result<impl Iterator<Item = Result<CommitInfo<'_>>> + '_> {
    let head_oid = match &self.vcs {
      GitVcsLevel::None { .. } => return Ok(E2::A(empty())),
      _ => self.get_oid_head()?.id()
    };

    Ok(E2::B(self.commits_between(from, head_oid, incl_from)?))
  }

  pub fn get_oid_head(&self) -> Result<AnnotatedCommit> { self.get_oid(self.branch_name()?) }

  pub fn get_oid(&self, spec: &str) -> Result<AnnotatedCommit> {
    match &self.vcs {
      GitVcsLevel::None { .. } => bail!("Can't get OID at `none`."),
      GitVcsLevel::Local { repo, .. } => {
        verify_current(repo).chain_err(|| "Can't complete get.")?;
        get_oid_local(repo, spec)
      }
      GitVcsLevel::Remote { repo, branch_name, remote_name, fetches }
      | GitVcsLevel::Smart { repo, branch_name, remote_name, fetches } => {
        verify_current(repo).chain_err(|| "Can't complete get.")?;
        get_oid_remote(repo, branch_name, spec, remote_name, fetches)
      }
    }
  }

  pub fn commit(&self) -> Result<bool> {
    if let GitVcsLevel::None { .. } = self.vcs {
      return Ok(false);
    }

    if let Some(mut index) = self.add_all_modified()? {
      let tree_oid = index.write_tree()?;
      self.commit_tree(tree_oid)?;
      self.push_head(&[])?;
      Ok(true)
    } else {
      Ok(false)
    }
  }

  fn add_all_modified(&self) -> Result<Option<Index>> {
    let repo = self.repo()?;
    let mut status_opts = StatusOptions::new();
    status_opts.include_ignored(false);
    status_opts.include_untracked(true);
    status_opts.exclude_submodules(false);

    let mut index = repo.index()?;
    let mut found = false;
    for s in repo.statuses(Some(&mut status_opts))?.iter().filter(|s| s.status().is_wt_modified()) {
      found = true;
      let path = s.path().ok_or_else(|| bad!("Bad path"))?;
      index.add_path(path.as_ref())?;
    }

    if found {
      Ok(Some(index))
    } else {
      Ok(None)
    }
  }

  fn commit_tree(&self, tree_oid: Oid) -> Result<()> {
    let repo = self.repo()?;
    let tree = repo.find_tree(tree_oid)?;
    let parent_commit = self.find_last_commit()?;
    let sig = Signature::now("Versio", "github.com/chaaz/versio")?;
    let head = Some("HEAD");
    let msg = "chore(deploy): update versions";

    let commit_oid = repo.commit(head, &sig, &sig, msg, &tree, &[&parent_commit])?;
    repo.reset(&repo.find_object(commit_oid, Some(ObjectType::Commit))?, ResetType::Mixed, None)?;

    Ok(())
  }

  fn find_last_commit(&self) -> Result<Commit> {
    let repo = self.repo()?;
    let obj = repo.head()?.resolve()?.peel(ObjectType::Commit)?;
    obj.into_commit().map_err(|o| bad!("Not a commit, somehow: {}", o.id()))
  }

  pub fn update_tag_head(&self, tag: &str) -> Result<()> { self.update_tag(tag, "HEAD") }

  pub fn update_tag(&self, tag: &str, spec: &str) -> Result<()> {
    if let GitVcsLevel::None { .. } = self.vcs {
      return Ok(());
    }

    let repo = self.repo()?;
    let obj = repo.revparse_single(spec)?;
    repo.tag_lightweight(tag, &obj, true)?;
    self.push_tag(tag)?;
    Ok(())
  }

  fn push_head(&self, tags: &[String]) -> Result<()> {
    let (repo, branch_name, remote_name) = match &self.vcs {
      GitVcsLevel::None { .. } | GitVcsLevel::Local { .. } => return Ok(()),
      GitVcsLevel::Remote { repo, branch_name, remote_name, .. }
      | GitVcsLevel::Smart { repo, branch_name, remote_name, .. } => (repo, branch_name, remote_name)
    };

    let mut refs = vec![format!("refs/heads/{}", branch_name)];
    for tag in tags {
      refs.push(format!("refs/tags/{}", tag));
    }

    do_push(repo, remote_name, &refs)
  }

  fn push_tag(&self, tag: &str) -> Result<()> {
    let (repo, remote_name) = match &self.vcs {
      GitVcsLevel::None { .. } | GitVcsLevel::Local { .. } => return Ok(()),
      GitVcsLevel::Remote { repo, remote_name, .. } | GitVcsLevel::Smart { repo, remote_name, .. } => {
        (repo, remote_name)
      }
    };

    do_push(repo, remote_name, &[format!("refs/tags/{}", tag)])
  }

  pub fn branch_name(&self) -> Result<&String> {
    match &self.vcs {
      GitVcsLevel::None { .. } => err!("No branch name at `none` level."),
      GitVcsLevel::Local { branch_name, .. }
      | GitVcsLevel::Remote { branch_name, .. }
      | GitVcsLevel::Smart { branch_name, .. } => Ok(branch_name)
    }
  }

  fn repo(&self) -> Result<&Repository> {
    match &self.vcs {
      GitVcsLevel::None { .. } => err!("No repo at `none` level."),
      GitVcsLevel::Local { repo, .. } | GitVcsLevel::Remote { repo, .. } | GitVcsLevel::Smart { repo, .. } => Ok(repo)
    }
  }

  fn remote_name(&self) -> Result<&String> {
    match &self.vcs {
      GitVcsLevel::None { .. } | GitVcsLevel::Local { .. } => err!("No remote at `none` or `local`."),
      GitVcsLevel::Remote { remote_name, .. } | GitVcsLevel::Smart { remote_name, .. } => Ok(remote_name)
    }
  }
}

#[derive(Clone)]
pub struct Slice<'r> {
  repo: &'r Repo,
  refspec: String
}

impl<'r> Slice<'r> {
  pub fn has_blob(&self, path: &str) -> Result<bool> { Ok(self.object(path).is_ok()) }
  pub fn slice(&self, refspec: String) -> Slice<'r> { Slice { repo: self.repo, refspec } }
  pub fn revparse_oid(&self) -> Result<String> { self.repo.revparse_oid(&self.refspec) }

  pub fn blob(&self, path: &str) -> Result<Blob> {
    let obj = self.object(path)?;
    obj.into_blob().map_err(|e| bad!("Not a blob: {} : {:?}", path, e))
  }

  pub fn subdirs(&self, path: Option<&String>, regex: &str) -> Result<Vec<String>> {
    trace!("Finding git subdirs at {:?}", path);

    let path = path.map(|s| s.as_str()).unwrap_or("");
    let obj = self.object(path)?;
    let tree = obj.into_tree().map_err(|_| bad!("Not a tree: {}", path))?;
    let filter = Regex::new(regex)?;
    Ok(tree.iter().filter_map(|entry| entry.name().map(|n| n.to_string())).filter(|n| filter.is_match(&n)).collect())
  }

  fn object(&self, path: &str) -> Result<Object> {
    Ok(self.repo.repo()?.revparse_single(&format!("{}:{}", &self.refspec, path))?)
  }

  pub fn date(&self) -> Result<Time> {
    let obj = self.repo.repo()?.revparse_single(&format!("{}^{{}}", self.refspec))?;
    let commit = obj.into_commit().map_err(|o| bad!("\"{}\" isn't a commit.", o.id()))?;
    Ok(commit.time())
  }
}

pub struct GithubInfo {
  owner_name: String,
  repo_name: String
}

impl GithubInfo {
  pub fn new(owner_name: String, repo_name: String) -> GithubInfo { GithubInfo { owner_name, repo_name } }
  pub fn owner_name(&self) -> &str { &self.owner_name }
  pub fn repo_name(&self) -> &str { &self.repo_name }
}

#[derive(Clone)]
pub struct CommitInfoBuf {
  id: String,
  summary: String,
  kind: String,
  files: Vec<String>
}

impl CommitInfoBuf {
  pub fn new(id: String, summary: String, files: Vec<String>) -> CommitInfoBuf {
    let kind = extract_kind(&summary);
    CommitInfoBuf { id, summary, kind, files }
  }

  pub fn guess(id: String) -> CommitInfoBuf { CommitInfoBuf::new(id, "".into(), Vec::new()) }

  pub fn extract<'a>(repo: &'a Repository, commit: &Commit<'a>) -> Result<CommitInfoBuf> {
    let id = commit.id().to_string();
    let summary = commit.summary().unwrap_or("-").to_string();
    let files = files_from_commit(repo, commit)?.collect();
    Ok(CommitInfoBuf::new(id, summary, files))
  }

  pub fn id(&self) -> &str { &self.id }
  pub fn summary(&self) -> &str { &self.summary }
  pub fn kind(&self) -> &str { &self.kind }
  pub fn files(&self) -> &[String] { &self.files }
}

pub struct CommitInfo<'a> {
  repo: &'a Repository,
  commit: Commit<'a>
}

impl<'a> CommitInfo<'a> {
  pub fn new(repo: &'a Repository, commit: Commit<'a>) -> CommitInfo<'a> { CommitInfo { repo, commit } }

  pub fn id(&self) -> String { self.commit.id().to_string() }
  pub fn summary(&self) -> &str { self.commit.summary().unwrap_or("-") }
  pub fn kind(&self) -> String { extract_kind(self.summary()) }
  pub fn files(&self) -> Result<impl Iterator<Item = String> + 'a> { files_from_commit(&self.repo, &self.commit) }

  pub fn buffer(self) -> Result<CommitInfoBuf> {
    Ok(CommitInfoBuf::new(self.id(), self.summary().to_string(), self.files()?.collect()))
  }
}

struct DeltaIter<'repo> {
  diff: Diff<'repo>,
  len: usize,
  on: usize,
  on_new: bool
}

impl<'repo> Iterator for DeltaIter<'repo> {
  type Item = PathBuf;

  fn next(&mut self) -> Option<PathBuf> {
    while self.current().is_none() {
      if self.advance() {
        break;
      }
    }

    let current = self.current().map(|p| p.to_path_buf());
    self.advance();
    current
  }
}

impl<'repo> DeltaIter<'repo> {
  pub fn new(diff: Diff<'repo>) -> DeltaIter<'repo> {
    let len = diff.deltas().len();
    DeltaIter { diff, len, on: 0, on_new: false }
  }

  fn current(&self) -> Option<&Path> {
    let delta = self.diff.get_delta(self.on);
    if self.on_new {
      delta.and_then(|delta| delta.new_file().path())
    } else {
      delta.and_then(|delta| delta.old_file().path())
    }
  }

  fn advance(&mut self) -> bool {
    if self.on >= self.len {
      return true;
    }

    if self.on_new {
      self.on_new = false;
      self.on += 1;
    } else {
      let old_path = self.diff.get_delta(self.on).and_then(|dl| dl.old_file().path());
      let new_path = self.diff.get_delta(self.on).and_then(|dl| dl.new_file().path());
      if old_path == new_path {
        self.on += 1;
      } else {
        self.on_new = true;
      }
    }

    self.on >= self.len
  }
}

pub struct FullPr {
  number: u32,
  head_ref: String,
  head_oid: Option<Oid>,
  base_oid: String,
  base_time: Time,
  commits: Vec<CommitInfoBuf>,
  excludes: Vec<String>,
  closed_at: DateTime<FixedOffset>
}

impl FullPr {
  pub fn lookup(
    repo: &Repo, base: String, headref: String, number: u32, closed_at: DateTime<FixedOffset>
  ) -> Result<FullPr> {
    let commit = repo.get_oid(&headref);
    match lookup_from_commit(repo, base.clone(), commit)? {
      Err(e) => {
        warn!("Couldn't fetch {}: using best-guess instead: {}", headref, e);
        Ok(FullPr {
          number,
          head_ref: headref,
          head_oid: None,
          base_oid: base,
          base_time: Time::new(0, 0),
          commits: Vec::new(),
          excludes: Vec::new(),
          closed_at
        })
      }
      Ok((commit, commits, base_time)) => Ok(FullPr {
        number,
        head_ref: headref,
        head_oid: Some(commit.id()),
        base_oid: base,
        base_time,
        commits,
        excludes: Vec::new(),
        closed_at
      })
    }
  }

  pub fn number(&self) -> u32 { self.number }
  pub fn head_ref(&self) -> &str { &self.head_ref }
  pub fn head_oid(&self) -> &Option<Oid> { &self.head_oid }
  pub fn base_oid(&self) -> &str { &self.base_oid }
  pub fn commits(&self) -> &[CommitInfoBuf] { &self.commits }
  pub fn excludes(&self) -> &[String] { &self.excludes }
  pub fn best_guess(&self) -> bool { self.head_oid.is_none() }
  pub fn has_exclude(&self, oid: &str) -> bool { self.excludes.iter().any(|c| c == oid) }
  pub fn closed_at(&self) -> &DateTime<FixedOffset> { &self.closed_at }

  pub fn included_commits(&self) -> impl Iterator<Item = &CommitInfoBuf> + '_ {
    self.commits.iter().filter(move |c| !self.has_exclude(c.id()))
  }

  pub fn span(&self) -> Option<Span> {
    self.head_oid.map(|hoid| Span::new(self.number, hoid, self.base_time, self.base_oid.clone()))
  }

  pub fn add_commit(&mut self, data: CommitInfoBuf) {
    if !self.commits.iter().any(|c| c.id() == data.id()) {
      self.commits.push(data)
    }
  }

  pub fn add_exclude(&mut self, commit_oid: &str) {
    if !self.excludes.iter().any(|c| c == commit_oid) {
      self.excludes.push(commit_oid.to_string());
    }
  }

  pub fn contains(&self, commit_oid: &str) -> bool { self.commits.iter().any(|c| c.id() == commit_oid) }
}

pub struct Span {
  number: u32,
  end: Oid,
  since: Time,
  begin: String
}

impl Span {
  pub fn new(number: u32, end: Oid, since: Time, begin: String) -> Span { Span { number, end, since, begin } }

  pub fn number(&self) -> u32 { self.number }
  pub fn end(&self) -> Oid { self.end }
  pub fn begin(&self) -> &str { &self.begin }
  pub fn since(&self) -> &Time { &self.since }
}

pub enum IterString {
  Git(StringArray),
  Empty
}

impl IterString {
  pub fn iter(&self) -> impl Iterator<Item = Option<&str>> {
    match self {
      IterString::Git(array) => E2::A(array.iter()),
      IterString::Empty => E2::B(empty())
    }
  }
}

enum GitVcsLevel {
  None { root: PathBuf },
  Local { repo: Repository, branch_name: String },
  Remote { repo: Repository, branch_name: String, remote_name: String, fetches: RefCell<HashMap<String, Oid>> },
  Smart { repo: Repository, branch_name: String, remote_name: String, fetches: RefCell<HashMap<String, Oid>> }
}

impl GitVcsLevel {
  fn from(
    level: VcsLevel, root: PathBuf, repo: Repository, branch_name: String, remote_name: String,
    fetches: RefCell<HashMap<String, Oid>>
  ) -> GitVcsLevel {
    match level {
      VcsLevel::None => GitVcsLevel::None { root },
      VcsLevel::Local => GitVcsLevel::Local { repo, branch_name },
      VcsLevel::Remote => GitVcsLevel::Remote { repo, branch_name, remote_name, fetches },
      VcsLevel::Smart => GitVcsLevel::Smart { repo, branch_name, remote_name, fetches }
    }
  }
}

fn find_root_blind<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
  let path = path.as_ref();
  if path.join(CONFIG_FILENAME).exists() {
    Ok(path.to_path_buf())
  } else {
    path.parent().ok_or_else(|| bad!("Not found in path: {}", CONFIG_FILENAME)).and_then(find_root_blind)
  }
}

fn find_remote_name(repo: &Repository, branch_name: &str) -> Result<String> {
  repo.config()?.get_str(&format!("branch.{}.remote", branch_name)).map(|s| s.to_string()).or_else(|_| {
    let remotes = repo.remotes()?;
    if remotes.is_empty() {
      err!("No remotes in this repo.")
    } else if remotes.len() == 1 {
      Ok(remotes.iter().next().unwrap().ok_or_else(|| bad!("Non-utf8 remote name."))?.to_string())
    } else {
      err!("Too many remotes in this repo.")
    }
  })
}

fn find_branch_name(repo: &Repository) -> Result<String> {
  let head_ref = repo.find_reference("HEAD").map_err(|e| bad!("Couldn't resolve head: {:?}.", e))?;
  if head_ref.kind() != Some(ReferenceType::Symbolic) {
    return err!("Not on a branch.");
  } else {
    let branch_name = head_ref.symbolic_target().ok_or_else(|| bad!("Branch is not named."))?;
    if branch_name.starts_with("refs/heads/") {
      Ok(branch_name[11 ..].to_string())
    } else {
      return err!("Current {} is not a branch.", branch_name);
    }
  }
}

fn find_github_info(repo: &Repository, remote_name: &str) -> Result<GithubInfo> {
  let remote = repo.find_remote(remote_name)?;

  let url = remote.url().ok_or_else(|| bad!("Invalid utf8 remote url."))?;
  let path = if url.starts_with("https://github.com/") {
    &url[19 ..]
  } else if url.starts_with("git@github.com:") {
    &url[15 ..]
  } else {
    return err!("Can't find github in remote url {}", url);
  };

  let len = path.len();
  let path = if path.ends_with(".git") { &path[0 .. len - 4] } else { path };
  let slash = path.char_indices().find(|(_, c)| *c == '/').map(|(i, _)| i);
  let slash = slash.ok_or_else(|| bad!("No slash found in github path \"{}\".", path))?;

  Ok(GithubInfo::new(path[0 .. slash].to_string(), path[slash + 1 ..].to_string()))
}

/// Merge the given commit into the working directory, but only if it's fast-forward-able.
fn ff_merge<'a>(repo: &'a Repository, branch_name: &str, commit: &AnnotatedCommit<'a>) -> Result<()> {
  let analysis = repo.merge_analysis(&[commit])?;

  if analysis.0.is_fast_forward() {
    let refname = format!("refs/heads/{}", branch_name);
    match repo.find_reference(&refname) {
      Ok(mut rfrnc) => Ok(fast_forward(repo, &mut rfrnc, commit)?),
      Err(_) => {
        // Probably pulling in an empty repo; just set the reference to the commit directly.
        let message = format!("Setting {} to {}", branch_name, commit.id());
        repo.reference(&refname, commit.id(), true, &message)?;
        repo.set_head(&refname)?;
        Ok(
          repo.checkout_head(Some(
            CheckoutBuilder::default().allow_conflicts(false).conflict_style_merge(false).force()
          ))?
        )
      }
    }
  } else if analysis.0.is_normal() {
    err!("Can't pull: would not be a fast-forward.")
  } else {
    info!("No merge: already up to date.");
    Ok(())
  }
}

/// Fast-forward the working directory head to the reference at the commit.
fn fast_forward(repo: &Repository, rfrnc: &mut Reference, rc: &AnnotatedCommit) -> Result<()> {
  let name = match rfrnc.name() {
    Some(s) => s.to_string(),
    None => String::from_utf8_lossy(rfrnc.name_bytes()).to_string()
  };

  let msg = format!("Fast-forward: {} -> {:.7}", name, rc.id());
  info!("{}", msg);

  rfrnc.set_target(rc.id(), &msg)?;
  repo.set_head(&name)?;

  // 'force' required to update the working directory; safe becaused we checked that it's clean.
  repo.checkout_head(Some(CheckoutBuilder::default().force()))?;

  Ok(())
}

fn extract_kind(summary: &str) -> String {
  match summary.char_indices().find(|(_, c)| *c == ':' || *c == '\n') {
    Some((i, c)) if c == ':' => {
      let kind = &summary[0 .. i].trim();
      let bang = kind.ends_with('!');
      match kind.char_indices().find(|(_, c)| *c == '(').map(|(i, _)| i) {
        Some(i) => {
          let kind = &kind[0 .. i].trim();
          if bang && !kind.ends_with('!') {
            format!("{}!", kind)
          } else {
            (*kind).to_string()
          }
        }
        None => (*kind).to_string()
      }
    }
    _ => "-".to_string()
  }
}

fn files_from_commit<'a>(repo: &'a Repository, commit: &Commit<'a>) -> Result<impl Iterator<Item = String> + 'a> {
  if commit.parents().len() == 1 {
    let parent = commit.parent(0)?;
    let ptree = parent.tree()?;
    let ctree = commit.tree()?;
    let diff = repo.diff_tree_to_tree(Some(&ptree), Some(&ctree), Some(&mut DiffOptions::new()))?;
    let iter = DeltaIter::new(diff);
    Ok(E2::A(iter.map(move |path| path.to_string_lossy().into_owned())))
  } else {
    Ok(E2::B(empty()))
  }
}

fn lookup_from_commit<'a>(
  repo: &Repo, base: String, commit: Result<AnnotatedCommit<'a>>
) -> Result<Result<(AnnotatedCommit<'a>, Vec<CommitInfoBuf>, Time)>> {
  let commit_id = commit.as_ref().map(|c| c.id().to_string()).unwrap_or_else(|_| "<err>".to_string());
  let result = match commit {
    Err(e) => Ok(Err(e)),
    Ok(commit) => {
      let base_time = repo.slice(base.clone()).date()?;
      let (commits, base_time) = repo
        .commits_between_buf(&base, commit.id())?
        .map(|(commits, early)| (commits, min(base_time, early)))
        .unwrap_or_else(|| (Vec::new(), base_time));
      Ok(Ok((commit, commits, base_time)))
    }
  };
  trace!(
    "lookup from {} to {:?}: {:?}",
    base,
    commit_id,
    result.as_ref().map(|r| r.as_ref().map(|(_, list, _)| list.iter().map(|c| c.id().to_string()).collect::<Vec<_>>()))
  );
  result
}

fn get_oid_local<'r>(repo: &'r Repository, spec: &str) -> Result<AnnotatedCommit<'r>> {
  let local_spec = format!("{}^{{}}", spec);
  let obj = repo.revparse_single(&local_spec)?;
  Ok(repo.find_annotated_commit(obj.id())?)
}

fn get_oid_remote<'r>(
  repo: &'r Repository, branch_name: &str, spec: &str, remote_name: &str, fetches: &RefCell<HashMap<String, Oid>>
) -> Result<AnnotatedCommit<'r>> {
  let (commit, cached) = verified_fetch(repo, remote_name, fetches, spec)?;

  if !cached && (spec == branch_name || spec == "HEAD") {
    info!("Merging to \"{}\" on local.", spec);
    ff_merge(repo, branch_name, &commit)?;
  }
  Ok(commit)
}

fn verified_fetch<'r>(
  repo: &'r Repository, remote_name: &str, fetches: &RefCell<HashMap<String, Oid>>, spec: &str
) -> Result<(AnnotatedCommit<'r>, bool)> {
  verify_current(repo).chain_err(|| "Can't start fetch.")?;

  if let Some(oid) = fetches.borrow().get(spec).cloned() {
    info!("No fetch for \"{}\": already fetched.", spec);
    let fetch_commit = repo.find_annotated_commit(oid)?;
    return Ok((fetch_commit, true));
  }

  safe_fetch(repo, remote_name, &[spec], true)?;

  // Assume a standard git config `remote.<remote_name>.fetch` layout; if not we can force the tracking
  // branch (change the refspec to "{refspec}:refs/remotes/{remote_name}/{refspec}"), or parse the config
  // layout to see where it landed. Or maybe just use FETCH_HEAD?
  let local_spec = format!("remotes/{}/{}^{{}}", remote_name, spec);
  let obj = repo.revparse_single(&local_spec)?;
  let oid = obj.id();

  let workspace_spec = format!("{}^{{}}", spec);
  let ws_oid = repo.revparse_single(&workspace_spec)?.id();
  if ws_oid != oid {
    warn!("`remotes/{}/{}` doesn't match local after fetch.", remote_name, spec);
  }

  fetches.borrow_mut().insert(spec.to_string(), oid);

  let fetch_commit = repo.find_annotated_commit(oid)?;
  assert!(fetch_commit.id() == oid);

  verify_current(repo).chain_err(|| "Can't complete fetch.")?;

  Ok((fetch_commit, false))
}

fn verify_current(repo: &Repository) -> Result<()> {
  let state = repo.state();
  if state != RepositoryState::Clean {
    // Don't bother if we're in the middle of a merge, rebase, etc.
    bail!("Can't pull: repository {:?} isn't clean.", state);
  }

  let mut status_opts = StatusOptions::new();
  status_opts.include_ignored(false);
  status_opts.include_untracked(true);
  status_opts.exclude_submodules(false);
  if repo.statuses(Some(&mut status_opts))?.iter().any(|s| s.status() != Status::CURRENT) {
    bail!("Repository is not current");
  }
  Ok(())
}

fn safe_fetch(repo: &Repository, remote_name: &str, specs: &[&str], all_tags: bool) -> Result<()> {
  let state = repo.state();
  if state != RepositoryState::Clean {
    // Don't bother if we're in the middle of a merge, rebase, etc.
    bail!("Can't pull: repository {:?} isn't clean.", state);
  }

  let mut remote = repo.find_remote(remote_name)?;

  // As of git server 2.6, you can fetch `refs/tags/xyz*`
  do_fetch(&mut remote, specs, all_tags)
}

/// Fetch the given refspecs (and maybe all tags) from the remote.
fn do_fetch(remote: &mut Remote, refs: &[&str], all_tags: bool) -> Result<()> {
  // WARNING: Currently not supporting fetching via sha:
  //
  // git has supported `git fetch <remote> <sha>` for a while, but it has to work a bit differently (since sha's
  // are not technically refspecs).

  info!("Fetching {:?}{}", refs, if all_tags { " and all tags." } else { "." });

  let mut cb = RemoteCallbacks::new();

  cb.credentials(|_url, username_from_url, _allowed_types| Cred::ssh_key_from_agent(username_from_url.unwrap()));

  cb.transfer_progress(|stats| {
    if stats.received_objects() == stats.total_objects() {
      info!("Resolving deltas {}/{}", stats.indexed_deltas(), stats.total_deltas());
    } else if stats.total_objects() > 0 {
      info!(
        "Received {}/{} objects ({}) in {} bytes",
        stats.received_objects(),
        stats.total_objects(),
        stats.indexed_objects(),
        stats.received_bytes()
      );
    }
    stdout().flush().unwrap();
    true
  });
  cb.sideband_progress(|bytes| {
    info!("Fetch progress: {}", String::from_utf8_lossy(bytes));
    true
  });
  cb.update_tips(|tip, _old, _new| {
    info!("Fetch update: {}", tip);
    true
  });

  let mut fo = FetchOptions::new();
  fo.remote_callbacks(cb);

  if all_tags {
    fo.download_tags(AutotagOption::All);
  }
  remote.fetch(refs, Some(&mut fo), None)?;

  let stats = remote.stats();
  if stats.local_objects() > 0 {
    info!(
      "Received {}/{} objects in {} bytes (used {} local objects)",
      stats.indexed_objects(),
      stats.total_objects(),
      stats.received_bytes(),
      stats.local_objects()
    );
  } else {
    info!("Received {}/{} objects in {} bytes", stats.indexed_objects(), stats.total_objects(), stats.received_bytes());
  }

  Ok(())
}

pub fn do_push(repo: &Repository, remote_name: &str, specs: &[String]) -> Result<()> {
  let mut cb = RemoteCallbacks::new();
  cb.credentials(|_url, username_from_url, _allowed_types| Cred::ssh_key_from_agent(username_from_url.unwrap()));

  info!("Pushing specs {:?} to remote {}", specs, remote_name);

  cb.push_update_reference(|rref, status| {
    if let Some(status) = status {
      error!("Couldn't push reference {}: {}", rref, status);
      return Err(git2::Error::from_str(&format!("Couldn't push reference {}: {}", rref, status)));
    }
    Ok(())
  });

  let mut push_opts = PushOptions::new();
  push_opts.remote_callbacks(cb);

  let mut remote = repo.find_remote(remote_name)?;
  remote.push(specs, Some(&mut push_opts))?;
  Ok(())
}

#[cfg(test)]
mod test {
  use super::extract_kind;

  #[test]
  fn test_kind_simple() {
    assert_eq!(&extract_kind("thing: this is thing"), "thing");
  }

  #[test]
  fn test_kind_bang() {
    assert_eq!(&extract_kind("thing! : this is thing"), "thing!");
  }

  #[test]
  fn test_kind_paren() {
    assert_eq!(&extract_kind("thing(scope): this is thing"), "thing");
  }

  #[test]
  fn test_kind_complex() {
    assert_eq!(&extract_kind("thing(scope)!: this is thing"), "thing!");
  }

  #[test]
  fn test_kind_backwards() {
    assert_eq!(&extract_kind("thing!(scope): this is thing"), "thing!");
  }
}
