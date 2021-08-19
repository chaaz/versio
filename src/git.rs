//! Interactions with git.

use crate::config::CONFIG_FILENAME;
use crate::either::IterEither2 as E2;
use crate::errors::{Result, ResultExt};
use crate::vcs::{VcsLevel, VcsState};
use chrono::{DateTime, FixedOffset};
use error_chain::bail;
use git2::build::CheckoutBuilder;
use git2::string_array::StringArray;
use git2::{AnnotatedCommit, AutotagOption, Blob, Commit, Cred, CredentialType, Diff, DiffOptions, FetchOptions, Index,
           Object, ObjectType, Oid, PushOptions, Reference, ReferenceType, Remote, RemoteCallbacks, Repository,
           RepositoryOpenFlags, RepositoryState, ResetType, Revwalk, Signature, Sort, Status, StatusOptions, Time};
use gpgme::{Context, Protocol};
use log::{error, info, trace, warn};
use path_slash::PathBufExt as _;
use regex::Regex;
use serde::Deserialize;
use std::cell::RefCell;
use std::cmp::{min, Ord};
use std::collections::HashMap;
use std::env::var;
use std::ffi::OsStr;
use std::fmt;
use std::io::{stdout, Write};
use std::iter::empty;
use std::path::{Path, PathBuf};

pub struct Repo {
  vcs: GitVcsLevel,
  ignore_current: bool
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

    let branch_name = match find_branch_name(&repo) {
      Err(_) => return Ok(VcsLevel::None),
      Ok(branch_name) => branch_name
    };
    trace!("Detected branch name: {:?}.", branch_name);

    match find_remote_name(&repo, &branch_name) {
      Ok(remote_name) => {
        trace!("Detected remote name: \"{}\".", remote_name);
        if find_github_info(&repo, &remote_name, &Default::default()).is_ok() {
          Ok(VcsLevel::Smart)
        } else {
          Ok(VcsLevel::Remote)
        }
      }
      Err(e) => {
        trace!("No remote name: {:?}.", e);
        Ok(VcsLevel::Local)
      }
    }
  }

  pub fn find_working_dir<P: AsRef<Path>>(path: P, vcs: VcsLevel, allow_cwd: bool) -> Result<PathBuf> {
    if vcs == VcsLevel::None {
      match find_root_blind(path.as_ref()) {
        Ok(path) => return Ok(path),
        Err(e) => {
          if allow_cwd {
            return Ok(path.as_ref().to_path_buf());
          } else {
            return Err(e);
          }
        }
      }
    }

    let flags = RepositoryOpenFlags::empty();
    let repo = Repository::open_ext(path, flags, empty::<&OsStr>())?;
    Ok(repo.workdir().ok_or_else(|| bad!("Repo has no working dir"))?.to_path_buf())
  }

  pub fn open<P: AsRef<Path>>(path: P, vcs: VcsState) -> Result<Repo> {
    let ignore_current = vcs.ignore_current();
    if vcs.level().is_none() {
      let root = find_root_blind(path)?;
      return Ok(Repo { ignore_current, vcs: GitVcsLevel::None { root } });
    }

    let flags = RepositoryOpenFlags::empty();
    let repo = Repository::open_ext(path, flags, empty::<&OsStr>())?;
    let branch_name = find_branch_name(&repo)?;

    if vcs.level().is_local() {
      return Ok(Repo { ignore_current, vcs: GitVcsLevel::Local { repo, branch_name } });
    }

    let remote_name = find_remote_name(&repo, &branch_name)?;
    let fetches = RefCell::new(HashMap::new());
    let root = repo.workdir().ok_or_else(|| bad!("Repo has no working dir."))?.to_path_buf();

    Ok(Repo { ignore_current, vcs: GitVcsLevel::from(vcs.level(), root, repo, branch_name, remote_name, fetches) })
  }

  pub fn working_dir(&self) -> Result<&Path> {
    match &self.vcs {
      GitVcsLevel::None { root } => Ok(root),
      GitVcsLevel::Local { repo, .. } | GitVcsLevel::Remote { repo, .. } | GitVcsLevel::Smart { repo, .. } => {
        repo.workdir().ok_or_else(|| bad!("Repo has no working dir"))
      }
    }
  }

  pub fn revparse_oid(&self, spec: FromTag) -> Result<String> {
    let repo = self.repo()?;
    if !self.ignore_current {
      verify_current(repo).chain_err(|| "Can't complete revparse.")?;
    }
    Ok(repo.revparse_single(spec.tag())?.id().to_string())
  }

  pub fn slice(&self, refspec: FromTagBuf) -> Slice { Slice { repo: self, refspec } }

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

  pub fn github_info(&self, auth: &Auth) -> Result<GithubInfo> {
    find_github_info(self.repo()?, self.remote_name()?, auth)
  }

  /// Return all commits as in `git rev-list from..to_sha`, along with the earliest time in that range.
  ///
  /// `from` may be any legal target of `rev-parse`.
  pub fn commits_between_buf(&self, from: FromTag, to_oid: Oid) -> Result<Option<(Vec<CommitInfoBuf>, Time)>> {
    let repo = self.repo()?;
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TOPOLOGICAL)?;
    hide_from(repo, &mut revwalk, from)?;
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

  /// Return all commits as in `git rev-list from..to_sha`.
  ///
  /// `from` may be any legal target of `rev-parse`.
  pub fn commits_between(
    &self, from: FromTag, to_oid: Oid, incl_from: bool
  ) -> Result<impl Iterator<Item = Result<CommitInfo>> + '_> {
    let repo = self.repo()?;
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TOPOLOGICAL)?;
    if incl_from {
      hide_from_parents(repo, &mut revwalk, from)?;
    } else {
      hide_from(repo, &mut revwalk, from)?;
    }
    revwalk.push(to_oid)?;

    Ok(revwalk.map(move |id| Ok(CommitInfo::new(repo, repo.find_commit(id?)?))))
  }

  /// Return all commits as in `git rev-list from_sha..HEAD`.
  ///
  /// `from` may be any legal target of `rev-parse`.
  pub fn commits_to_head<'r>(
    &'r self, from: FromTag, incl_from: bool
  ) -> Result<impl Iterator<Item = Result<CommitInfo<'r>>> + 'r> {
    let head_oid = match &self.vcs {
      GitVcsLevel::None { .. } => return Ok(E2::A(empty())),
      _ => self.get_oid_head()?.id()
    };

    Ok(E2::B(self.commits_between(from, head_oid, incl_from)?))
  }

  pub fn get_oid_head(&self) -> Result<AnnotatedCommit> {
    if let Some(branch_name) = self.branch_name()? {
      self.get_oid(branch_name)
    } else {
      self.get_oid("HEAD")
    }
  }

  pub fn get_oid(&self, spec: &str) -> Result<AnnotatedCommit> {
    match &self.vcs {
      GitVcsLevel::None { .. } => bail!("Can't get OID at `none`."),
      GitVcsLevel::Local { repo, .. } => {
        if !self.ignore_current {
          verify_current(repo).chain_err(|| "Can't complete get.")?;
        }
        get_oid_local(repo, spec)
      }
      GitVcsLevel::Remote { repo, branch_name, remote_name, fetches }
      | GitVcsLevel::Smart { repo, branch_name, remote_name, fetches } => {
        if spec == "HEAD" {
          if !self.ignore_current {
            verify_current(repo).chain_err(|| "Can't complete HEAD get.")?;
          }
          get_oid_local(repo, spec)
        } else {
          // get_oid_remote() will verify current
          get_oid_remote(repo, branch_name, spec, remote_name, fetches)
        }
      }
    }
  }

  pub fn annotation_of(&self, tag: &str) -> Option<String> {
    let repo = match &self.vcs {
      GitVcsLevel::None { .. } => return None,
      GitVcsLevel::Local { repo, .. } | GitVcsLevel::Remote { repo, .. } | GitVcsLevel::Smart { repo, .. } => repo
    };

    repo
      .refname_to_id(&format!("refs/tags/{}", tag))
      .and_then(|oid| repo.find_tag(oid))
      .ok()
      .and_then(|tag| tag.message().map(|m| m.to_string()))
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
    status_opts.exclude_submodules(true);

    let mut index = repo.index()?;
    let mut found = false;
    for s in repo.statuses(Some(&mut status_opts))?.iter().filter(|s| {
      let s = s.status();
      s.is_wt_modified() || s.is_wt_deleted() || s.is_wt_renamed() || s.is_wt_typechange() || s.is_wt_new()
    }) {
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
    let msg = "build(deploy): Versio update versions";

    let commit_oid = if repo.config()?.get_bool("commit.gpgSign").unwrap_or(false) {
      let mut ctx = Context::from_protocol(Protocol::OpenPgp)?;

      let signid = repo.config()?.get_string("user.signingKey").ok();
      if let Some(signid) = signid {
        let key = ctx
          .keys()?
          .find(|k| k.as_ref().map(|k| k.id().map(|id| id == signid).unwrap_or(false)).unwrap_or(false))
          .ok_or_else(|| bad!("No key found with ID: {}", signid))??;
        ctx.add_signer(&key)?;
      }

      let buf = repo.commit_create_buffer(&sig, &sig, msg, &tree, &[&parent_commit])?;

      let mut outbuf = Vec::new();
      ctx.set_armor(true);
      ctx.sign_detached(&*buf, &mut outbuf)?;

      let contents = buf.as_str().ok_or("Buffer was not valid UTF-8")?;
      let out = std::str::from_utf8(&outbuf)?;

      repo.commit_signed(contents, out, Some("gpgsig"))?
    } else {
      repo.commit(head, &sig, &sig, msg, &tree, &[&parent_commit])?
    };

    repo.reset(&repo.find_object(commit_oid, Some(ObjectType::Commit))?, ResetType::Mixed, None)?;

    Ok(())
  }

  fn find_last_commit(&self) -> Result<Commit> {
    let repo = self.repo()?;
    let obj = repo.head()?.resolve()?.peel(ObjectType::Commit)?;
    obj.into_commit().map_err(|o| bad!("Not a commit, somehow: {}", o.id()))
  }

  pub fn update_tag_head(&self, tag: &str) -> Result<()> { self.update_tag(tag, "HEAD") }

  pub fn update_tag_head_anno(&self, tag: &str, msg: &str) -> Result<()> { self.update_tag_anno(tag, "HEAD", msg) }

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

  pub fn update_tag_anno(&self, tag: &str, spec: &str, msg: &str) -> Result<()> {
    if let GitVcsLevel::None { .. } = self.vcs {
      return Ok(());
    }

    let repo = self.repo()?;
    let obj = repo.revparse_single(spec)?;
    let tagger = Signature::now("Versio", "github.com/chaaz/versio")?;

    let config = repo.config()?;
    let fsa = config.get_bool("tag.forceSignAnnotated").unwrap_or(false);
    let gsign = config.get_bool("tag.gpgSign").unwrap_or(false);

    if fsa || gsign {
      // There's no tag_create_buffer() in libgit2, so we'll do this:
      //   - tag it
      //   - read the raw tag data
      //   - sign that (`git tag -s` signs the entire buffer, so this should be good)
      //   - get the detached signature, and put that at the bottom of the msg
      //   - re-tag with everything the same, having changed only the message.
      //
      // This leaves an old tag lying around, but that should eventually be garbage collected.

      // make the msg end with a newline, so that the signature starts on a new line
      let msg_string = if msg.ends_with('\n') { msg.to_string() } else { format!("{}\n", msg) };

      let first_oid = repo.tag(tag, &obj, &tagger, &msg_string, true)?;
      let odb = repo.odb()?;
      let tag_obj = odb.read(first_oid)?;
      let raw = std::str::from_utf8(tag_obj.data())?;

      let mut ctx = Context::from_protocol(Protocol::OpenPgp)?;

      let signid = repo.config()?.get_string("user.signingKey").ok();
      if let Some(signid) = signid {
        let key = ctx
          .keys()?
          .find(|k| k.as_ref().map(|k| k.id().map(|id| id == signid).unwrap_or(false)).unwrap_or(false))
          .ok_or_else(|| bad!("No key found with ID: {}", signid))??;
        ctx.add_signer(&key)?;
      }

      let mut outbuf = Vec::new();
      ctx.set_armor(true);
      ctx.sign_detached(raw, &mut outbuf)?;

      let detached_sig = std::str::from_utf8(&outbuf)?;

      repo.tag(tag, &obj, &tagger, &format!("{}{}", msg_string, detached_sig), true)?;
    } else {
      repo.tag(tag, &obj, &tagger, msg, true)?;
    }
    self.push_tag(tag)?;
    Ok(())
  }

  fn push_head(&self, tags: &[String]) -> Result<()> {
    let (repo, branch_name, remote_name) = match &self.vcs {
      GitVcsLevel::None { .. } | GitVcsLevel::Local { .. } => return Ok(()),
      GitVcsLevel::Remote { repo, branch_name, remote_name, .. }
      | GitVcsLevel::Smart { repo, branch_name, remote_name, .. } => (repo, branch_name, remote_name)
    };

    let branch_name = branch_name.as_ref().ok_or_else(|| bad!("No branch name for push."))?;
    let mut refs = vec![format!("+refs/heads/{}", branch_name)];
    for tag in tags {
      refs.push(format!("+refs/tags/{}", tag));
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

    do_push(repo, remote_name, &[format!("+refs/tags/{}", tag)])
  }

  pub fn branch_name(&self) -> Result<&Option<String>> {
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
  refspec: FromTagBuf
}

impl<'r> Slice<'r> {
  pub fn has_blob(&self, path: &str) -> Result<bool> { Ok(self.object(path).is_ok()) }
  pub fn slice(&self, refspec: FromTagBuf) -> Slice<'r> { Slice { repo: self.repo, refspec } }

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
    Ok(tree.iter().filter_map(|entry| entry.name().map(|n| n.to_string())).filter(|n| filter.is_match(n)).collect())
  }

  #[cfg(not(target_family = "windows"))]
  fn object(&self, path: &str) -> Result<Object> {
    Ok(self.repo.repo()?.revparse_single(&format!("{}:{}", self.refspec.tag(), path))?)
  }

  // Always path issues on windows. See https://github.com/JuliaLang/julia/issues/18724
  #[cfg(target_family = "windows")]
  fn object(&self, path: &str) -> Result<Object> {
    let path = path.replace('\\', "/");
    Ok(self.repo.repo()?.revparse_single(&format!("{}:{}", self.refspec.tag(), &path))?)
  }

  pub fn date(&self) -> Result<Option<Time>> {
    let obj = match self.repo.repo()?.revparse_single(&format!("{}^{{}}", self.refspec.tag())) {
      Ok(obj) => obj,
      Err(e) => {
        if self.refspec.else_none {
          return Ok(None);
        } else {
          return Err(e.into());
        }
      }
    };
    let commit = obj.into_commit().map_err(|o| bad!("\"{}\" isn't a commit.", o.id()))?;
    Ok(Some(commit.time()))
  }
}

pub struct GithubInfo {
  owner_name: String,
  repo_name: String,
  token: Option<String>
}

impl GithubInfo {
  pub fn new(owner_name: String, repo_name: String, token: Option<String>) -> GithubInfo {
    GithubInfo { owner_name, repo_name, token }
  }

  pub fn owner_name(&self) -> &str { &self.owner_name }
  pub fn repo_name(&self) -> &str { &self.repo_name }
  pub fn token(&self) -> &Option<String> { &self.token }
}

#[derive(Clone)]
pub struct CommitInfoBuf {
  id: String,
  summary: String,
  message: String,
  kind: String,
  files: Vec<String>
}

impl CommitInfoBuf {
  pub fn new(id: String, kind: String, summary: String, message: String, files: Vec<String>) -> CommitInfoBuf {
    CommitInfoBuf { id, summary, message, kind, files }
  }

  pub fn guess(id: String) -> CommitInfoBuf { CommitInfoBuf::new(id, "-".into(), "-".into(), "".into(), Vec::new()) }

  pub fn extract<'a>(repo: &'a Repository, commit: &Commit<'a>) -> Result<CommitInfoBuf> {
    let id = commit.id().to_string();
    let summary = commit.summary().unwrap_or("-").to_string();
    let message = commit.message().unwrap_or("-").to_string();
    let kind = extract_kind(&message);
    let files = files_from_commit(repo, commit)?.collect();
    Ok(CommitInfoBuf::new(id, kind, summary, message, files))
  }

  pub fn id(&self) -> &str { &self.id }
  pub fn summary(&self) -> &str { &self.summary }
  pub fn message(&self) -> &str { &self.message }
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
  pub fn message(&self) -> &str { self.commit.message().unwrap_or("-") }
  pub fn kind(&self) -> String { extract_kind(self.message()) }
  pub fn files(&self) -> Result<impl Iterator<Item = String> + 'a> { files_from_commit(self.repo, &self.commit) }

  pub fn buffer(self) -> Result<CommitInfoBuf> {
    Ok(CommitInfoBuf::new(
      self.id(),
      self.kind(),
      self.summary().to_string(),
      self.message().to_string(),
      self.files()?.collect()
    ))
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
  title: String,
  head_ref: String,
  head_oid: Option<Oid>,
  base_oid: FromTagBuf,
  base_time: Time,
  commits: Vec<CommitInfoBuf>,
  excludes: Vec<String>,
  closed_at: DateTime<FixedOffset>,
  discovery_order: usize
}

impl FullPr {
  pub fn lookup(
    repo: &Repo, base: FromTagBuf, headref: String, number: u32, title: String, closed_at: DateTime<FixedOffset>,
    discovery_order: usize
  ) -> Result<FullPr> {
    let commit = repo.get_oid(&headref);
    match lookup_from_commit(repo, base.clone(), commit)? {
      Err(e) => {
        warn!("Couldn't fetch {}: using best-guess instead: {}", headref, e);
        Ok(FullPr {
          number,
          title,
          head_ref: headref,
          head_oid: None,
          base_oid: base,
          base_time: Time::new(0, 0),
          commits: Vec::new(),
          excludes: Vec::new(),
          closed_at,
          discovery_order
        })
      }
      Ok((commit, commits, base_time)) => Ok(FullPr {
        number,
        title,
        head_ref: headref,
        head_oid: Some(commit.id()),
        base_oid: base,
        base_time,
        commits,
        excludes: Vec::new(),
        closed_at,
        discovery_order
      })
    }
  }

  pub fn number(&self) -> u32 { self.number }
  pub fn title(&self) -> &str { &self.title }
  pub fn head_ref(&self) -> &str { &self.head_ref }
  pub fn head_oid(&self) -> &Option<Oid> { &self.head_oid }
  pub fn base_oid(&self) -> FromTag { self.base_oid.as_from_tag() }
  pub fn commits(&self) -> &[CommitInfoBuf] { &self.commits }
  pub fn excludes(&self) -> &[String] { &self.excludes }
  pub fn best_guess(&self) -> bool { self.head_oid.is_none() }
  pub fn has_exclude(&self, oid: &str) -> bool { self.excludes.iter().any(|c| c == oid) }
  pub fn closed_at(&self) -> &DateTime<FixedOffset> { &self.closed_at }
  pub fn discovery_order(&self) -> usize { self.discovery_order }

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
  begin: FromTagBuf
}

impl Span {
  pub fn new(number: u32, end: Oid, since: Time, begin: FromTagBuf) -> Span { Span { number, end, since, begin } }

  pub fn number(&self) -> u32 { self.number }
  pub fn end(&self) -> Oid { self.end }
  pub fn begin(&self) -> FromTag { self.begin.as_from_tag() }
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
  Local { repo: Repository, branch_name: Option<String> },
  Remote { repo: Repository, branch_name: Option<String>, remote_name: String, fetches: RefCell<HashMap<String, Oid>> },
  Smart { repo: Repository, branch_name: Option<String>, remote_name: String, fetches: RefCell<HashMap<String, Oid>> }
}

impl GitVcsLevel {
  fn from(
    level: &VcsLevel, root: PathBuf, repo: Repository, branch_name: Option<String>, remote_name: String,
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

/// A git commit hash-like (hash, branch, tag, etc) to revwalk "from" (a.k.a. "hide"), or none if the hash-like
/// couldn't be looked up.
#[derive(Clone)]
pub struct FromTag<'a> {
  tag: &'a str,
  else_none: bool
}

impl<'a> FromTag<'a> {
  pub fn new(tag: &'a str, else_none: bool) -> FromTag<'a> { FromTag { tag, else_none } }
  pub fn tag(&self) -> &'a str { self.tag }
  // pub fn is_else_none(&self) -> bool { self.else_none }
  // pub fn to_from_tag_buf(&self) -> FromTagBuf { FromTagBuf::new(self.tag.to_string(), self.else_none) }
}

impl<'a> From<&'a str> for FromTag<'a> {
  fn from(a: &'a str) -> FromTag<'a> { FromTag::new(a, false) }
}

// impl<'a> Into<FromTag<'a>> for &'a str {
//   fn into(self) -> FromTag<'a> { FromTag::new(self, false) }
// }

impl<'a> fmt::Display for FromTag<'a> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "[from {}{}]", self.tag, if self.else_none { " (else none)" } else { "" })
  }
}

#[derive(Clone)]
pub struct FromTagBuf {
  tag: String,
  else_none: bool
}

impl FromTagBuf {
  pub fn new(tag: String, else_none: bool) -> FromTagBuf { FromTagBuf { tag, else_none } }
  pub fn as_from_tag(&self) -> FromTag { FromTag::new(&self.tag, self.else_none) }
  pub fn tag(&self) -> &str { &self.tag }
  // pub fn is_else_none(&self) -> bool { self.else_none }
}

impl fmt::Display for FromTagBuf {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "[from {}{}]", self.tag, if self.else_none { " (else none)" } else { "" })
  }
}

#[derive(Deserialize, Debug)]
pub struct Auth {
  github_token: Option<String>
}

impl Default for Auth {
  fn default() -> Auth { Auth { github_token: None } }
}

impl Auth {
  pub fn github_token(&self) -> &Option<String> { &self.github_token }
  pub fn set_github_token(&mut self, token: Option<String>) { self.github_token = token; }
}

fn find_root_blind<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
  let path = path.as_ref();
  if path.join(CONFIG_FILENAME).exists() {
    Ok(path.to_path_buf())
  } else {
    path.parent().ok_or_else(|| bad!("Not found in path: {}", CONFIG_FILENAME)).and_then(find_root_blind)
  }
}

fn find_remote_name(repo: &Repository, branch_name: &Option<String>) -> Result<String> {
  let configured = branch_name
    .as_ref()
    .and_then(|branch_name| {
      repo
        .config()
        .and_then(|mut config| config.snapshot())
        .map(|config| config.get_string(&format!("branch.{}.remote", branch_name)).ok())
        .transpose()
    })
    .transpose()?
    .ok_or_else(|| bad!("No configured remote found for {:?}.", branch_name));

  configured.or_else(|e| {
    let remotes = repo.remotes()?;
    if remotes.is_empty() {
      err!("No remotes in this repo: {}.", e)
    } else if remotes.len() == 1 {
      Ok(remotes.iter().next().unwrap().ok_or_else(|| bad!("Non-utf8 remote name."))?.to_string())
    } else {
      err!("Too many remotes in this repo: {}.", e)
    }
  })
}

fn find_branch_name(repo: &Repository) -> Result<Option<String>> {
  let head_ref = repo.find_reference("HEAD").map_err(|e| bad!("Couldn't resolve head: {:?}.", e))?;
  if head_ref.kind() != Some(ReferenceType::Symbolic) {
    Ok(None)
  } else {
    match head_ref.symbolic_target() {
      None => Ok(None),
      Some(branch_name) => {
        if let Some(bname_suff) = branch_name.strip_prefix("refs/heads/") {
          Ok(Some(bname_suff.to_string()))
        } else {
          return err!("Current {} is not a branch.", branch_name);
        }
      }
    }
  }
}

fn find_github_info(repo: &Repository, remote_name: &str, auth: &Auth) -> Result<GithubInfo> {
  let remote = repo.find_remote(remote_name)?;

  let url = remote.url().ok_or_else(|| bad!("Invalid utf8 remote url."))?;
  let path = if let Some(url_suff) = url.strip_prefix("https://github.com/") {
    url_suff
  } else if let Some(url_suff) = url.strip_prefix("git@github.com:") {
    url_suff
  } else {
    return err!("Can't find github in remote url {}", url);
  };

  let len = path.len();
  let path = if path.ends_with(".git") { &path[0 .. len - 4] } else { path };
  let slash = path.char_indices().find(|(_, c)| *c == '/').map(|(i, _)| i);
  let slash = slash.ok_or_else(|| bad!("No slash found in github path \"{}\".", path))?;

  Ok(GithubInfo::new(path[0 .. slash].to_string(), path[slash + 1 ..].to_string(), auth.github_token().clone()))
}

/// Hide ancestors of `from` from the revwalk, but don't hide anything if the commit-ish can't be found and
/// `else_none` is true.
fn hide_from<'r>(repo: &'r Repository, revwalk: &mut Revwalk<'r>, from: FromTag) -> Result<()> {
  let FromTag { tag, else_none } = from;
  match repo.revparse_single(tag) {
    Ok(oid) => Ok(revwalk.hide(oid.id())?),
    Err(err) => {
      if !else_none {
        Err(err).chain_err(|| format!("Can't find commits start {}", tag))
      } else {
        Ok(())
      }
    }
  }
}

fn hide_from_parents<'r>(repo: &'r Repository, revwalk: &mut Revwalk<'r>, from: FromTag) -> Result<()> {
  let FromTag { tag, else_none } = from;
  match repo.revparse_single(tag).and_then(|obj| obj.peel_to_commit()) {
    Ok(commit) => {
      for pid in commit.parent_ids() {
        revwalk.hide(pid)?;
      }
      Ok(())
    }
    Err(err) => {
      if !else_none {
        Err(err).chain_err(|| format!("Can't find inclusive commits start {}", tag))
      } else {
        Ok(())
      }
    }
  }
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

/// Finds a conventional commit "type" from a commit message.
///
/// The type can be one of the special characters "-" (no type found) or "!" ("BREAKING CHANGE:" or
/// "BREAKING-CHANGE:" starting footer, or "!" after type/scope)
fn extract_kind(message: &str) -> String {
  let breaking_pattern =
    Regex::new("^(?s).*?\\n\\n((BREAKING CHANGE|BREAKING-CHANGE):|.*\n(BREAKING CHANGE|BREAKING-CHANGE):)").unwrap();
  if breaking_pattern.is_match(message) {
    return "!".into();
  }

  match message.char_indices().find(|(_, c)| *c == ':' || *c == '\n') {
    Some((i, c)) if c == ':' => {
      let kind = &message[.. i].trim();
      if kind.ends_with('!') {
        return "!".into();
      }
      match kind.char_indices().find(|(_, c)| *c == '(').map(|(i, _)| i) {
        Some(i) => {
          let kind = &kind[0 .. i].trim();
          if kind.ends_with('!') {
            "!".into()
          } else {
            (*kind).to_lowercase()
          }
        }
        None => (*kind).to_lowercase()
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
    Ok(E2::A(iter.map(move |path| path.to_slash_lossy())))
  } else {
    Ok(E2::B(empty()))
  }
}

fn lookup_from_commit<'a>(
  repo: &Repo, base: FromTagBuf, commit: Result<AnnotatedCommit<'a>>
) -> Result<Result<(AnnotatedCommit<'a>, Vec<CommitInfoBuf>, Time)>> {
  let commit_id = commit.as_ref().map(|c| c.id().to_string()).unwrap_or_else(|_| "<err>".to_string());
  let result = match commit {
    Err(e) => Ok(Err(e)),
    Ok(commit) => {
      let base_time = repo.slice(base.clone()).date()?;
      let (commits, base_time) = repo
        .commits_between_buf(base.as_from_tag(), commit.id())?
        .map(|(commits, early)| (commits, min_ok(base_time, early)))
        .unwrap_or_else(|| (Vec::new(), base_time.unwrap_or_else(|| Time::new(0, 0))));
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

fn min_ok<C: Ord>(c1: Option<C>, c0: C) -> C {
  match c1 {
    Some(c1) => min(c0, c1),
    None => c0
  }
}

fn get_oid_local<'r>(repo: &'r Repository, spec: &str) -> Result<AnnotatedCommit<'r>> {
  let local_spec = format!("{}^{{}}", spec);
  let obj = repo.revparse_single(&local_spec)?;
  Ok(repo.find_annotated_commit(obj.id())?)
}

fn get_oid_remote<'r>(
  repo: &'r Repository, branch_name: &Option<String>, spec: &str, remote_name: &str,
  fetches: &RefCell<HashMap<String, Oid>>
) -> Result<AnnotatedCommit<'r>> {
  let (commit, cached) = verified_fetch(repo, remote_name, fetches, spec)?;

  if let Some(branch_name) = branch_name {
    if !cached && spec == branch_name {
      info!("Merging to \"{}\" on local.", spec);
      ff_merge(repo, branch_name, &commit)?;
    }
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
  let remote_spec = format!("remotes/{}/{}^{{}}", remote_name, spec);
  let obj = repo.revparse_single(&remote_spec)?;
  let oid = obj.id();

  // We don't need the revspec to be in our local database. But if it is there, it should match.
  let local_spec = format!("{}^{{}}", spec);
  if let Ok(loc_oid) = repo.revparse_single(&local_spec).map(|loc| loc.id()) {
    if loc_oid != oid {
      bail!("`remotes/{}/{}` doesn't match local after fetch.", remote_name, spec);
    }
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

  let statuses = repo.statuses(Some(&mut status_opts))?;
  let bad_status = statuses.iter().find(|s| s.status() != Status::CURRENT);
  if let Some(bad_status) = bad_status {
    bail!("Repository is not current: {} = {:?}", bad_status.path().unwrap_or("<none>"), bad_status.status());
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

  cb.credentials(find_creds);
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

fn find_creds(
  _url: &str, username_from_url: Option<&str>, _allowed_types: CredentialType
) -> std::result::Result<Cred, git2::Error> {
  if let Some(username_from_url) = username_from_url {
    if let Ok(v) = Cred::ssh_key_from_agent(username_from_url) {
      return Ok(v);
    }
  }

  if let Ok((user, token)) = var("GITHUB_TOKEN").and_then(|token| var("GITHUB_USER").map(|user| (user, token))) {
    if let Ok(v) = Cred::userpass_plaintext(&user, &token) {
      return Ok(v);
    }
  }

  Err(git2::Error::from_str("Unable to authenticate"))
}

pub fn do_push(repo: &Repository, remote_name: &str, specs: &[String]) -> Result<()> {
  info!("Pushing specs {:?} to remote {}", specs, remote_name);
  let mut cb = RemoteCallbacks::new();

  cb.credentials(find_creds);
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
    assert_eq!(&extract_kind("thing! : this is thing"), "!");
  }

  #[test]
  fn test_kind_paren() {
    assert_eq!(&extract_kind("thing(scope): this is thing"), "thing");
  }

  #[test]
  fn test_kind_complex() {
    assert_eq!(&extract_kind("thing(scope)!: this is thing"), "!");
  }

  #[test]
  fn test_kind_backwards() {
    assert_eq!(&extract_kind("thing!(scope): this is thing"), "!");
  }

  #[test]
  fn test_kind_breaking() {
    assert_eq!(&extract_kind("thing(scope): this is thing\n\nbody\n\nBREAKING CHANGE: yup"), "!");
  }

  #[test]
  fn test_kind_breaking_no_body() {
    assert_eq!(&extract_kind("thing(scope): this is thing\n\nBREAKING CHANGE: yup"), "!");
  }

  #[test]
  fn test_kind_breaking_later() {
    assert_eq!(&extract_kind("thing(scope): this is thing\n\nbody\n\nfoot: 1\nBREAKING CHANGE: yup"), "!");
  }

  #[test]
  fn test_kind_breaking_both() {
    assert_eq!(&extract_kind("thing(scope)!: this is thing\n\nbody\n\nBREAKING CHANGE: yup"), "!");
  }

  #[test]
  fn test_kind_breaking_dash() {
    assert_eq!(&extract_kind("thing(scope): this is thing\n\nbody\n\nBREAKING-CHANGE: yup"), "!");
  }

  #[test]
  fn test_empty() {
    assert_eq!(&extract_kind(""), "-");
  }

  #[test]
  fn test_unconventional() {
    assert_eq!(&extract_kind("-"), "-");
  }

  #[test]
  fn test_uncertain() {
    assert_eq!(&extract_kind("ENG-123: I forgot to conventinal commit"), "eng-123");
  }
}
