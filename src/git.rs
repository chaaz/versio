//! Interactions with git.

use crate::either::IterEither2 as E2;
use crate::errors::Result;
use crate::vcs::VcsLevel;
use chrono::{DateTime, FixedOffset};
use git2::build::CheckoutBuilder;
use git2::string_array::StringArray;
use git2::{
  AnnotatedCommit, AutotagOption, Blob, Commit, Cred, Diff, DiffOptions, FetchOptions, Index, Object, ObjectType, Oid,
  Reference, ReferenceType, Remote, RemoteCallbacks, Repository, RepositoryOpenFlags, RepositoryState, ResetType,
  Signature, Status, StatusOptions, Time
};
use std::cell::RefCell;
use std::cmp::min;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};
use crate::config::CONFIG_FILENAME;
use log::{info, warn};

pub struct Repo {
  vcs: GitVcsLevel
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

  fn vcs_level(&self) -> VcsLevel {
    match self {
      GitVcsLevel::None { .. } => VcsLevel::None,
      GitVcsLevel::Local { .. } => VcsLevel::Local,
      GitVcsLevel::Remote { .. } => VcsLevel::Remote,
      GitVcsLevel::Smart { .. } => VcsLevel::Smart
    }
  }
}

impl Repo {
  /// Return the vcs level that this repository can support.
  pub fn negotiate<P: AsRef<Path>>(path: P) -> Result<VcsLevel> {
    let flags = RepositoryOpenFlags::empty();
    let repo = Repository::open_ext(path, flags, std::iter::empty::<&OsStr>());
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
    let repo = Repository::open_ext(path, flags, std::iter::empty::<&OsStr>())?;
    let branch_name = find_branch_name(&repo)?;

    if vcs == VcsLevel::Local {
      return Ok(Repo { vcs: GitVcsLevel::Local { repo, branch_name } });
    }

    let remote_name = find_remote_name(&repo, &branch_name)?;
    let fetches = RefCell::new(HashMap::new());
    let root = repo.workdir().ok_or_else(|| bad!("Repo has no working dir."))?.to_path_buf();

    Ok(Repo { vcs: GitVcsLevel::from(vcs, root, repo, branch_name, remote_name, fetches) })
  }

  pub fn vcs_level(&self) -> VcsLevel { self.vcs.vcs_level() }

  pub fn working_dir(&self) -> Result<&Path> {
    match &self.vcs {
      GitVcsLevel::None { root } => Ok(root),
      GitVcsLevel::Local { repo, .. } | GitVcsLevel::Remote { repo, .. } | GitVcsLevel::Smart { repo, .. } => {
        repo.workdir().ok_or_else(|| bad!("Repo has no working dir"))
      }
    }
  }

  pub fn revparse_oid(&self, spec: &str) -> Result<String> { Ok(self.repo()?.revparse_single(spec)?.id().to_string()) }
  pub fn slice(&self, refspec: String) -> Slice { Slice { repo: self, refspec } }
  pub fn tag_names(&self, pattern: Option<&str>) -> Result<StringArray> { Ok(self.repo()?.tag_names(pattern)?) }
  pub fn github_info(&self) -> Result<GithubInfo> { find_github_info(self.repo()?, self.remote_name()?) }

  /// Return all commits as if `git rev-list from_sha..to_sha`, along with the earliest time in that range.
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

  pub fn commits_between(&self, from: &str, to_oid: Oid) -> Result<impl Iterator<Item = Result<CommitInfo<'_>>> + '_> {
    let repo = self.repo()?;
    let mut revwalk = repo.revwalk()?;
    revwalk.hide(repo.revparse_single(from)?.id())?;
    revwalk.push(to_oid)?;

    Ok(revwalk.map(move |id| Ok(CommitInfo::new(repo, repo.find_commit(id?)?))))
  }

  pub fn commits_to_head(&self, from: &str) -> Result<impl Iterator<Item = Result<CommitInfo<'_>>> + '_> {
    let repo = self.repo()?;
    let mut revwalk = repo.revwalk()?;
    revwalk.hide(repo.revparse_single(from)?.id())?;
    revwalk.push_head()?;

    Ok(revwalk.map(move |id| Ok(CommitInfo::new(repo, repo.find_commit(id?)?))))
  }

  pub fn pull(&self) -> Result<()> {
    let fetch_commit = self.fetch_head()?;
    ff_merge(self.repo()?, self.branch_name()?, &fetch_commit)
  }

  pub fn fetch_head(&self) -> Result<AnnotatedCommit> {
    self.fetch(self.branch_name()?)
  }

  pub fn fetch(&self, spec: &str) -> Result<AnnotatedCommit> {
    let oid = self.fetch_start(spec)?;
    self.check_after_fetch(oid)
  }

  fn fetch_start(&self, spec: &str) -> Result<Oid> {
    let repo = self.repo()?;
    let fetches = self.fetches()?;
    let remote_name = self.remote_name()?;

    if let Some(oid) = fetches.borrow().get(spec).cloned() {
      return Ok(oid);
    }

    let state = self.repo()?.state();
    if state != RepositoryState::Clean {
      // Don't bother if we're in the middle of a merge, rebase, etc.
      bail!("Can't pull: repository {:?} isn't clean.", state);
    }

    let mut remote = repo.find_remote(remote_name)?;
    do_fetch(&mut remote, &[spec])?;

    // Assume a standard git config `remote.<remote_name>.fetch` layout; if not we can force the tracking
    // branch (change the refspec to "{refspec}:refs/remotes/{remote_name}/{refspec}"), or parse the config
    // layout to see where it landed.
    let local_spec = format!("remotes/{}/{}^{{}}", remote_name, spec);
    let obj = repo.revparse_single(&local_spec)?;
    let oid = obj.id();

    let workspace_spec = format!("{}^{{}}", spec);
    let ws_oid = repo.revparse_single(&workspace_spec)?.id();
    if ws_oid != oid {
      warn!("Remotes/{}/{} doesn't match local.", remote_name, spec);
    }

    fetches.borrow_mut().insert(spec.to_string(), oid);
    Ok(oid)
  }

  fn check_after_fetch(&self, fetch_oid: Oid) -> Result<AnnotatedCommit> {
    let repo = self.repo()?;
    let fetch_commit = repo.find_annotated_commit(fetch_oid)?;
    assert!(fetch_commit.id() == fetch_oid);

    let mut status_opts = StatusOptions::new();
    status_opts.include_ignored(false);
    status_opts.include_untracked(true);
    status_opts.exclude_submodules(false);
    if repo.statuses(Some(&mut status_opts))?.iter().any(|s| s.status() != Status::CURRENT) {
      bail!("Can't complete fetch: repository isn't current.");
    }

    Ok(fetch_commit)
  }

  pub fn commit(&self) -> Result<bool> {
    if let Some(mut index) = self.add_all_modified()? {
      let tree_oid = index.write_tree()?;
      self.commit_tree(tree_oid)?;
      // TODO: update tags? self.push(tags)?
      Ok(true)
    } else {
      // TODO: push the tags regardless
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
    let msg = "Updated versions by versio";

    let commit_oid = repo.commit(head, &sig, &sig, msg, &tree, &[&parent_commit])?;
    repo.reset(&repo.find_object(commit_oid, Some(ObjectType::Commit))?, ResetType::Mixed, None)?;

    Ok(())
  }

  fn find_last_commit(&self) -> Result<Commit> {
    let repo = self.repo()?;
    let obj = repo.head()?.resolve()?.peel(ObjectType::Commit)?;
    obj.into_commit().map_err(|o| bad!("Not a commit, somehow: {}", o.id()))
  }

  pub fn update_tag_head(&self, tag: &str) -> Result<()> {
    let repo = self.repo()?;
    let obj = repo.revparse_single("HEAD")?;
    repo.tag_lightweight(tag, &obj, true)?;
    Ok(())
  }

  pub fn update_tags_head(&self, new_tags: &[String]) -> Result<()> {
    let repo = self.repo()?;
    let obj = repo.revparse_single("HEAD")?;
    for tag in new_tags {
      repo.tag_lightweight(tag, &obj, true)?;
    }
    Ok(())
  }

  pub fn update_tag(&self, tag: &str, spec: &str) -> Result<()> {
    let repo = self.repo()?;
    let obj = repo.revparse_single(spec)?;
    repo.tag_lightweight(tag, &obj, true)?;
    Ok(())
  }

  pub fn update_tags(&self, changed_tags: &HashMap<String, String>) -> Result<()> {
    let repo = self.repo()?;
    for (tag, commit) in changed_tags {
      let obj = repo.revparse_single(commit)?;
      repo.tag_lightweight(tag, &obj, true)?;
    }
    Ok(())
  }

  // pub fn forward_tags(&self, changed_tags: &HashMap<String, String>) -> Result<bool> {
  //   if !changed_tags.is_empty() {
  //     self.forward_other_tags(changed_tags)?;
  //     self.push_forward_tags(changed_tags)?;
  //     Ok(true)
  //   } else {
  //     Ok(false)
  //   }
  // }

  // pub fn forward_prev_tag(&self, tag: &str) -> Result<()> {
  //   self.update_prev_tag(tag)?;
  //   self.push_prev_tag(tag)?;
  //   Ok(())
  // }

  // fn update_prev_tag(&self, tag: &str) -> Result<()> {
  //   let obj = self.repo.revparse_single("HEAD")?;
  //   self.repo.tag_lightweight(tag, &obj, true)?;
  //   Ok(())
  // }

  // fn push_forward_tags(&self, changed_tags: &HashMap<String, String>) -> Result<()> {
  //   if let Some(remote_name) = &self.remote_name {
  //     let mut remote = self.repo.find_remote(remote_name)?;
  //     let refs: Vec<_> = changed_tags.keys().map(|tag| format!("refs/tags/{}", tag)).collect();
  //     // TODO: do we need to do something here to force-push tag refs if they already exist on the remote?
  //     //   (see also force-push comment in `fn push`)

  //     let mut cb = RemoteCallbacks::new();
  //     cb.credentials(|_url, username_from_url, _allowed_types| Cred::ssh_key_from_agent(username_from_url.unwrap()));

  //     let mut push_opts = PushOptions::new();
  //     push_opts.remote_callbacks(cb);

  //     remote.push(&refs, Some(&mut push_opts))?;
  //   }

  //   Ok(())
  // }

  // fn push_prev_tag(&self, tag: &str) -> Result<()> {
  //   if let Some(remote_name) = &self.remote_name {
  //     let mut remote = self.repo.find_remote(remote_name)?;
  //     let refs = &[format!("refs/tags/{}", tag)];
  //     // TODO: do we need to do something here to force-push tag refs if they already exist on the remote?
  //     //   (see also force-push comment in `fn push`)

  //     // TODO: collapse this duplicated callbacs/push_opts/push dance
  //     let mut cb = RemoteCallbacks::new();
  //     cb.credentials(|_url, username_from_url, _allowed_types| Cred::ssh_key_from_agent(username_from_url.unwrap()));

  //     let mut push_opts = PushOptions::new();
  //     push_opts.remote_callbacks(cb);

  //     remote.push(refs, Some(&mut push_opts))?;
  //   }

  //   Ok(())
  // }

  // fn push(&self, new_tags: &[String]) -> Result<()> {
  //   if let Some(remote_name) = &self.remote_name {
  //     let mut remote = self.repo.find_remote(remote_name)?;
  //     let bchref = format!("refs/heads/{}", self.branch_name);

  //     let mut refs = vec![bchref];
  //     for tag in new_tags {
  //       refs.push(format!("refs/tags/{}", tag));
  //     }
  //     // TODO: do we need to do something here to force-push tag refs if they already exist on the remote?
  //     //   (see also force-push comment in `fn push_forward_tags`)

  //     let mut cb = RemoteCallbacks::new();
  //     cb.credentials(|_url, username_from_url, _allowed_types| Cred::ssh_key_from_agent(username_from_url.unwrap()));

  //     // TODO: do we have to rollback the tag if the heads didn't succeed.
  //     cb.push_update_reference(|rref, status| {
  //       if let Some(status) = status {
  //         println!("Couldn't push reference {}: {}", rref, status);
  //         return Err(git2::Error::from_str(&format!("Couldn't push reference {}: {}", rref, status)));
  //       }
  //       Ok(())
  //     });

  //     let mut push_opts = PushOptions::new();
  //     push_opts.remote_callbacks(cb);

  //     remote.push(&refs, Some(&mut push_opts))?;
  //   }
  //   Ok(())
  // }

  fn repo(&self) -> Result<&Repository> {
    match &self.vcs {
      GitVcsLevel::None { .. } => err!("No repo at `none` level."),
      GitVcsLevel::Local { repo, .. } | GitVcsLevel::Remote { repo, .. } | GitVcsLevel::Smart { repo, .. } => {
        Ok(repo)
      }
    }
  }

  pub fn branch_name(&self) -> Result<&String> {
    match &self.vcs {
      GitVcsLevel::None { .. } => err!("No branch name at `none` level."),
      GitVcsLevel::Local { branch_name, .. } | GitVcsLevel::Remote { branch_name, .. }
          | GitVcsLevel::Smart { branch_name, .. } => {
        Ok(branch_name)
      }
    }
  }

  fn remote_name(&self) -> Result<&String> {
    match &self.vcs {
      GitVcsLevel::None { .. } | GitVcsLevel::Local { .. } => err!("No remote at `none` or `local`."),
      GitVcsLevel::Remote { remote_name, .. } | GitVcsLevel::Smart { remote_name, .. } => Ok(remote_name)
    }
  }

  fn fetches(&self) -> Result<&RefCell<HashMap<String, Oid>>> {
    match &self.vcs {
      GitVcsLevel::None { .. } | GitVcsLevel::Local { .. } => err!("No fetches at `none` or `local`."),
      GitVcsLevel::Remote { fetches, .. } | GitVcsLevel::Smart { fetches, .. } => Ok(fetches)
    }
  }
}

pub struct Slice<'r> {
  repo: &'r Repo,
  refspec: String
}

impl<'r> Slice<'r> {
  pub fn refspec(&self) -> &str { &self.refspec }
  pub fn has_blob<P: AsRef<Path>>(&self, path: P) -> Result<bool> { Ok(self.object(path).is_ok()) }
  pub fn slice(&self, refspec: String) -> Slice<'r> { Slice { repo: self.repo, refspec } }
  pub fn repo(&self) -> &Repo { &self.repo }

  pub fn blob<P: AsRef<Path>>(&self, path: P) -> Result<Blob> {
    let obj = self.object(path.as_ref())?;
    obj.into_blob().map_err(|e| bad!("Not a blob: {} : {:?}", path.as_ref().to_string_lossy(), e))
  }

  fn object<P: AsRef<Path>>(&self, path: P) -> Result<Object> {
    let path_string = path.as_ref().to_string_lossy();
    Ok(self.repo.repo()?.revparse_single(&format!("{}:{}", &self.refspec, &path_string))?)
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
    repo: &Repo, headref: String, base: String, number: u32, closed_at: DateTime<FixedOffset>
  ) -> Result<FullPr> {
    match repo.fetch(&headref) {
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
      Ok(commit) => {
        let base_time = repo.slice(base.clone()).date()?;

        let (commits, base_time) = repo
          .commits_between_buf(&base, commit.id())?
          .map(|(commits, early)| (commits, min(base_time, early)))
          .unwrap_or_else(|| (Vec::new(), base_time));

        Ok(FullPr {
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
  pub fn into_commits(self) -> Vec<CommitInfoBuf> { self.commits }

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

fn find_root_blind<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
  let path = path.as_ref();
  if path.join(CONFIG_FILENAME).exists() {
    Ok(path.to_path_buf())
  } else {
    path.parent().ok_or_else(|| bad!("Not found in path: {}", CONFIG_FILENAME)).and_then(find_root_blind)
  }
}

fn find_remote_name(repo: &Repository, branch_name: &str) -> Result<String> {
  repo
    .config()?
    .get_str(&format!("branch.{}.remote", branch_name))
    .map(|s| s.to_string())
    .or_else(|_| {
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

/// Fetch the given refspecs (and all tags) from the remote.
fn do_fetch<'a>(remote: &'a mut Remote, refs: &[&str]) -> Result<()> {
  // WARNING: Currently not supporting fetching via sha:
  //
  // git has supported `git fetch <remote> <sha>` for a while, but it has to work a bit differently (since sha's
  // are not technically refspecs).

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

  let mut fo = FetchOptions::new();
  fo.remote_callbacks(cb);

  fo.download_tags(AutotagOption::All);
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
    info!(
      "Received {}/{} objects in {} bytes",
      stats.indexed_objects(),
      stats.total_objects(),
      stats.received_bytes()
    );
  }

  Ok(())
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
  // TODO: only search as far as newline ?
  match summary.char_indices().find(|(_, c)| *c == ':').map(|(i, _)| i) {
    Some(i) => {
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
    None => "-".to_string()
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
    Ok(E2::B(std::iter::empty()))
  }
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

//  pub struct FetchResults {
//    pub remote_name: Option<String>,
//    pub fetch_branch: String,
//    pub commit_oid: Option<Oid>
//  }
