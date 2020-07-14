//! Interactions with git.

use crate::either::IterEither2 as E2;
use crate::error::Result;
use chrono::{DateTime, FixedOffset};
use git2::build::CheckoutBuilder;
use git2::{
  AnnotatedCommit, AutotagOption, Blob, Commit, Cred, Diff, DiffOptions, FetchOptions, Index, Object, ObjectType, Oid,
  PushOptions, Reference, ReferenceType, Remote, RemoteCallbacks, Repository, RepositoryOpenFlags, RepositoryState,
  ResetType, Signature, Status, StatusOptions, Time
};
use std::cell::RefCell;
use std::cmp::min;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};

pub struct Repo {
  repo: Repository,
  fetches: RefCell<HashMap<String, Oid>>,
  branch_name: String,
  remote_name: Option<String>
}

impl Repo {
  pub fn root_dir<P: AsRef<Path>>(dir: P) -> Result<PathBuf> {
    let flags = RepositoryOpenFlags::empty();
    let root_dir = Repository::open_ext(dir, flags, std::iter::empty::<&OsStr>())?
      .workdir()
      .ok_or_else(|| versio_error!("No working directory."))?
      .to_path_buf();
    Ok(root_dir)
  }

  pub fn open<P: AsRef<Path>>(path: P) -> Result<Repo> {
    let flags = RepositoryOpenFlags::empty();
    let repo = Repository::open_ext(path, flags, std::iter::empty::<&OsStr>())?;
    let fetches = RefCell::new(HashMap::new());

    let branch_name = {
      let head_ref = repo.find_reference("HEAD").map_err(|e| versio_error!("Couldn't resolve head: {:?}.", e))?;
      if head_ref.kind() != Some(ReferenceType::Symbolic) {
        return versio_err!("Not on a branch.");
      } else {
        let branch_name = head_ref.symbolic_target().ok_or_else(|| versio_error!("Branch is not named."))?;
        if branch_name.starts_with("refs/heads/") {
          branch_name[11 ..].to_string()
        } else {
          return versio_err!("Current {} is not a branch.", branch_name);
        }
      }
    };

    let config = repo.config()?;
    let remote_name = config.get_str(&format!("branch.{}.remote", branch_name)).ok();

    let remote_name = remote_name.map(|s| Ok(Some(s.to_string()))).unwrap_or_else(|| {
      let remotes = repo.remotes()?;
      if remotes.is_empty() {
        Ok(None)
      } else if remotes.len() == 1 {
        Ok(Some(remotes.iter().next().unwrap().ok_or_else(|| versio_error!("Non-utf8 remote name."))?.to_string()))
      } else {
        versio_err!("Couldn't determine remote name.")
      }
    })?;

    Ok(Repo { repo, fetches, branch_name, remote_name })
  }

  pub fn working_dir(&self) -> Result<&Path> {
    self.repo.workdir().ok_or_else(|| versio_error!("Repo has no working dir"))
  }

  pub fn slice(&self, refspec: String) -> Slice { Slice { repo: self, refspec } }
  pub fn branch_name(&self) -> &str { &self.branch_name }
  pub fn remote_name(&self) -> &Option<String> { &self.remote_name }
  pub fn has_remote(&self) -> bool { self.remote_name.is_some() }

  pub fn github_info(&self) -> Result<Option<GithubInfo>> {
    let remote_name = match self.remote_name() {
      Some(remote_name) => remote_name,
      None => return Ok(None)
    };
    let remote = self.repo.find_remote(&remote_name)?;

    let url = remote.url().ok_or_else(|| versio_error!("Invalid utf8 remote url."))?;
    let path = if url.starts_with("https://github.com/") {
      Some(&url[19 ..])
    } else if url.starts_with("git@github.com:") {
      Some(&url[15 ..])
    } else {
      None
    };

    path
      .map(|path| {
        let len = path.len();
        let path = if path.ends_with(".git") { &path[0 .. len - 4] } else { path };

        let slash = path.char_indices().find(|(_, c)| *c == '/').map(|(i, _)| i);
        let slash = slash.ok_or_else(|| versio_error!("No slash found in github path \"{}\".", path))?;

        Ok(GithubInfo::new(path[0 .. slash].to_string(), path[slash + 1 ..].to_string()))
      })
      .transpose()
  }

  pub fn fetch(&self) -> Result<Option<Oid>> { self.slice(self.branch_name.to_string()).fetch() }

  pub fn fetch_current(&self) -> Result<Option<Oid>> {
    if let Some(oid) = self.fetch()? {
      self.check_after_fetch(oid)?;
      Ok(Some(oid))
    } else {
      Ok(None)
    }
  }

  pub fn pull(&self) -> Result<()> {
    if let Some(oid) = self.fetch()? {
      self.merge_after_fetch(oid)?;
    }
    Ok(())
  }

  pub fn make_changes(&self, new_tags: &[String]) -> Result<bool> {
    if let Some(mut index) = self.add_all_modified()? {
      let tree_oid = index.write_tree()?;
      self.commit_tree(tree_oid)?;
      self.update_other_tags(new_tags)?;
      self.push(new_tags)?;
      Ok(true)
    } else {
      // TODO: push the tag regardless
      Ok(false)
    }
  }

  pub fn forward_tags(&self, changed_tags: &HashMap<String, String>) -> Result<bool> {
    if !changed_tags.is_empty() {
      self.forward_other_tags(changed_tags)?;
      self.push_forward_tags(changed_tags)?;
      Ok(true)
    } else {
      Ok(false)
    }
  }

  pub fn forward_prev_tag(&self, tag: &str) -> Result<()> {
    self.update_prev_tag(tag)?;
    self.push_prev_tag(tag)?;
    Ok(())
  }

  /// Return all commits as if `git rev-list from_sha..to_sha`, along with the earliest time in that range.
  pub fn dated_revlist(&self, from_sha: &str, to_oid: Oid) -> Result<Option<(Vec<CommitData>, Time)>> {
    let mut revwalk = self.repo.revwalk()?;
    revwalk.hide(self.repo.revparse_single(from_sha)?.id())?;
    revwalk.push(to_oid)?;

    revwalk.try_fold::<_, _, Result<Option<(Vec<CommitData>, Time)>>>(None, |v, oid| {
      let oid = oid?;
      let commit = self.repo.find_commit(oid)?;
      let ctime = commit.time();
      if let Some((mut datas, time)) = v {
        datas.push(CommitData::extract(&self.repo, &commit)?);
        Ok(Some((datas, min(time, ctime))))
      } else {
        let datas = vec![CommitData::extract(&self.repo, &commit)?];
        Ok(Some((datas, ctime)))
      }
    })
  }

  pub fn commits_between<'a>(
    &'a self, from_sha: &str, to_oid: Oid
  ) -> Result<impl Iterator<Item = Result<CommitInfo<'a>>> + 'a> {
    let mut revwalk = self.repo.revwalk()?;
    revwalk.hide(self.repo.revparse_single(from_sha)?.id())?;
    revwalk.push(to_oid)?;

    Ok(revwalk.map(move |id| Ok(CommitInfo::new(&self.repo, self.repo.find_commit(id?)?))))
  }

  fn check_after_fetch(&self, fetch_oid: Oid) -> Result<AnnotatedCommit> {
    let fetch_commit = self.repo.find_annotated_commit(fetch_oid)?;

    let mut status_opts = StatusOptions::new();
    status_opts.include_ignored(false);
    status_opts.include_untracked(true);
    status_opts.exclude_submodules(false);
    if self.repo.statuses(Some(&mut status_opts))?.iter().any(|s| s.status() != Status::CURRENT) {
      return versio_err!("Can't complete fetch: repository isn't current.");
    }

    Ok(fetch_commit)
  }

  fn merge_after_fetch(&self, fetch_oid: Oid) -> Result<()> {
    let fetch_commit = self.check_after_fetch(fetch_oid)?;
    do_merge(&self.repo, &self.branch_name, fetch_commit)?;
    Ok(())
  }

  fn add_all_modified(&self) -> Result<Option<Index>> {
    let mut status_opts = StatusOptions::new();
    status_opts.include_ignored(false);
    status_opts.include_untracked(true);
    status_opts.exclude_submodules(false);

    let mut index = self.repo.index()?;
    let mut found = false;
    for s in self.repo.statuses(Some(&mut status_opts))?.iter().filter(|s| s.status().is_wt_modified()) {
      found = true;
      let path = s.path().ok_or_else(|| versio_error!("Bad path"))?;
      index.add_path(path.as_ref())?;
    }

    if found {
      Ok(Some(index))
    } else {
      Ok(None)
    }
  }

  fn commit_tree(&self, tree_oid: Oid) -> Result<()> {
    let tree = self.repo.find_tree(tree_oid)?;
    let parent_commit = self.find_last_commit()?;
    let sig = Signature::now("Versio", "github.com/chaaz/versio")?;
    let head = Some("HEAD");
    let msg = "Updated versions by versio";

    let commit_oid = self.repo.commit(head, &sig, &sig, msg, &tree, &[&parent_commit])?;
    self.repo.reset(&self.repo.find_object(commit_oid, Some(ObjectType::Commit))?, ResetType::Mixed, None)?;

    Ok(())
  }

  fn update_prev_tag(&self, tag: &str) -> Result<()> {
    let obj = self.repo.revparse_single("HEAD")?;
    self.repo.tag_lightweight(tag, &obj, true)?;
    Ok(())
  }

  fn update_other_tags(&self, new_tags: &[String]) -> Result<()> {
    let obj = self.repo.revparse_single("HEAD")?;
    for tag in new_tags {
      self.repo.tag_lightweight(tag, &obj, true)?;
    }
    Ok(())
  }

  fn forward_other_tags(&self, changed_tags: &HashMap<String, String>) -> Result<()> {
    for (tag, commit) in changed_tags {
      let obj = self.repo.revparse_single(commit)?;
      self.repo.tag_lightweight(tag, &obj, true)?;
    }
    Ok(())
  }

  fn push_forward_tags(&self, changed_tags: &HashMap<String, String>) -> Result<()> {
    if let Some(remote_name) = &self.remote_name {
      let mut remote = self.repo.find_remote(remote_name)?;
      let refs: Vec<_> = changed_tags.keys().map(|tag| format!("refs/tags/{}", tag)).collect();
      // TODO: do we need to do something here to force-push tag refs if they already exist on the remote?
      //   (see also force-push comment in `fn push`)

      let mut cb = RemoteCallbacks::new();
      cb.credentials(|_url, username_from_url, _allowed_types| Cred::ssh_key_from_agent(username_from_url.unwrap()));

      let mut push_opts = PushOptions::new();
      push_opts.remote_callbacks(cb);

      remote.push(&refs, Some(&mut push_opts))?;
    }

    Ok(())
  }

  fn push_prev_tag(&self, tag: &str) -> Result<()> {
    if let Some(remote_name) = &self.remote_name {
      let mut remote = self.repo.find_remote(remote_name)?;
      let refs = &[format!("refs/tags/{}", tag)];
      // TODO: do we need to do something here to force-push tag refs if they already exist on the remote?
      //   (see also force-push comment in `fn push`)

      // TODO: collapse this duplicated callbacs/push_opts/push dance
      let mut cb = RemoteCallbacks::new();
      cb.credentials(|_url, username_from_url, _allowed_types| Cred::ssh_key_from_agent(username_from_url.unwrap()));

      let mut push_opts = PushOptions::new();
      push_opts.remote_callbacks(cb);

      remote.push(refs, Some(&mut push_opts))?;
    }

    Ok(())
  }

  fn push(&self, new_tags: &[String]) -> Result<()> {
    if let Some(remote_name) = &self.remote_name {
      let mut remote = self.repo.find_remote(remote_name)?;
      let bchref = format!("refs/heads/{}", self.branch_name);

      let mut refs = vec![bchref];
      for tag in new_tags {
        refs.push(format!("refs/tags/{}", tag));
      }
      // TODO: do we need to do something here to force-push tag refs if they already exist on the remote?
      //   (see also force-push comment in `fn push_forward_tags`)

      let mut cb = RemoteCallbacks::new();
      cb.credentials(|_url, username_from_url, _allowed_types| Cred::ssh_key_from_agent(username_from_url.unwrap()));

      // TODO: do we have to rollback the tag if the heads didn't succeed.
      cb.push_update_reference(|rref, status| {
        if let Some(status) = status {
          println!("Couldn't push reference {}: {}", rref, status);
          return Err(git2::Error::from_str(&format!("Couldn't push reference {}: {}", rref, status)));
        }
        Ok(())
      });

      let mut push_opts = PushOptions::new();
      push_opts.remote_callbacks(cb);

      remote.push(&refs, Some(&mut push_opts))?;
    }
    Ok(())
  }

  fn find_last_commit(&self) -> Result<Commit> {
    let obj = self.repo.head()?.resolve()?.peel(ObjectType::Commit)?;
    obj.into_commit().map_err(|o| versio_error!("Not a commit, somehow: {}", o.id()))
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

  pub fn blob<P: AsRef<Path>>(&self, path: P) -> Result<Blob> {
    let obj = self.object(path.as_ref())?;
    obj.into_blob().map_err(|e| versio_error!("Not a blob: {} : {:?}", path.as_ref().to_string_lossy(), e))
  }

  pub fn object<P: AsRef<Path>>(&self, path: P) -> Result<Object> {
    let path_string = path.as_ref().to_string_lossy();
    Ok(self.repo.repo.revparse_single(&format!("{}:{}", &self.refspec, &path_string))?)
  }

  pub fn date(&self) -> Result<Time> {
    let obj = self.repo.repo.revparse_single(&format!("{}^{{}}", self.refspec))?;
    let commit = obj.into_commit().map_err(|o| versio_error!("\"{}\" isn't a commit.", o.id()))?;
    Ok(commit.time())
  }

  /// Fetch this slice from the remote, if there is one.
  ///
  /// This will only work if the slice is actually a refspec: fetching by sha is currently unsupported (and will
  /// result in an `Err`).
  pub fn fetch(&self) -> Result<Option<Oid>> {
    if let Some(oid) = self.repo.fetches.borrow().get(&self.refspec).cloned() {
      return Ok(Some(oid));
    }

    let state = self.repo.repo.state();
    if state != RepositoryState::Clean {
      // Don't bother if we're in the middle of a merge, rebase, etc.
      return versio_err!("Can't pull: repository {:?} isn't clean.", state);
    }

    if let Some(remote_name) = &self.repo.remote_name {
      let mut remote = self.repo.repo.find_remote(remote_name)?;
      do_fetch(&mut remote, &[&self.refspec])?;

      // Assume a standard git config `remote.<remote_name>.fetch` layout; if not we can force the tracking
      // branch (change the refspec to "{refspec}:refs/remotes/{remote_name}/{refspec}"), or parse the config
      // layout to see where it landed.
      let local_spec = format!("remotes/{}/{}^{{}}", remote_name, self.refspec);

      let obj = self.repo.repo.revparse_single(&local_spec)?;
      let oid = obj.id();

      // Make sure that the remote and workspace refer to the same ID
      let workspace_spec = format!("{}^{{}}", self.refspec);
      let ws_obj = self.repo.repo.revparse_single(&workspace_spec)?;
      let ws_oid = ws_obj.id();
      if ws_oid != oid {
        println!("Warning: remote {} doesn't match local; repo may be out-of-date", remote_name);
      }

      self.repo.fetches.borrow_mut().insert(self.refspec.to_string(), oid);
      Ok(Some(oid))
    } else {
      Ok(None)
    }
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
pub struct CommitData {
  id: String,
  summary: String,
  kind: String,
  files: Vec<String>
}

impl CommitData {
  pub fn new(id: String, summary: String, files: Vec<String>) -> CommitData {
    let kind = extract_kind(&summary);
    CommitData { id, summary, kind, files }
  }

  pub fn guess(id: String) -> CommitData { CommitData::new(id, "".into(), Vec::new()) }

  pub fn extract<'a>(repo: &'a Repository, commit: &Commit<'a>) -> Result<CommitData> {
    let id = commit.id().to_string();
    let summary = commit.summary().unwrap_or("-").to_string();
    let files = files_from_commit(repo, commit)?.collect();
    Ok(CommitData::new(id, summary, files))
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
  commits: Vec<CommitData>,
  excludes: Vec<String>,
  closed_at: DateTime<FixedOffset>
}

impl FullPr {
  pub fn lookup(
    repo: &Repo, headref: String, base: String, number: u32, closed_at: DateTime<FixedOffset>
  ) -> Result<FullPr> {
    match repo.slice(headref.clone()).fetch() {
      Err(e) => {
        println!("Couldn't fetch {}: using best-guess instead: {:?}", headref, e);
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
      Ok(None) => versio_err!("No fetched oid for {} somehow.", headref),
      Ok(Some(oid)) => {
        let base_time = repo.slice(base.clone()).date()?;

        let (commits, base_time) = repo
          .dated_revlist(&base, oid)?
          .map(|(commits, early)| (commits, min(base_time, early)))
          .unwrap_or_else(|| (Vec::new(), base_time));

        Ok(FullPr {
          number,
          head_ref: headref,
          head_oid: Some(oid),
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
  pub fn commits(&self) -> &[CommitData] { &self.commits }
  pub fn excludes(&self) -> &[String] { &self.excludes }
  pub fn best_guess(&self) -> bool { self.head_oid.is_none() }
  pub fn has_exclude(&self, oid: &str) -> bool { self.excludes.iter().any(|c| c == oid) }
  pub fn closed_at(&self) -> &DateTime<FixedOffset> { &self.closed_at }
  pub fn into_commits(self) -> Vec<CommitData> { self.commits }

  pub fn included_commits(&self) -> impl Iterator<Item = &CommitData> + '_ {
    self.commits.iter().filter(move |c| !self.has_exclude(c.id()))
  }

  pub fn span(&self) -> Option<Span> {
    self.head_oid.map(|hoid| Span::new(self.number, hoid, self.base_time, self.base_oid.clone()))
  }

  pub fn add_commit(&mut self, data: CommitData) {
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
      print!("Resolving deltas {}/{}\r", stats.indexed_deltas(), stats.total_deltas());
    } else if stats.total_objects() > 0 {
      print!(
        "Received {}/{} objects ({}) in {} bytes\r",
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
    println!(
      "\rReceived {}/{} objects in {} bytes (used {} local objects)",
      stats.indexed_objects(),
      stats.total_objects(),
      stats.received_bytes(),
      stats.local_objects()
    );
  } else {
    println!(
      "\rReceived {}/{} objects in {} bytes",
      stats.indexed_objects(),
      stats.total_objects(),
      stats.received_bytes()
    );
  }

  Ok(())
}

/// Merge the given commit into the working directory, but only if it's fast-forward-able.
fn do_merge<'a>(repo: &'a Repository, branch_name: &str, commit: AnnotatedCommit<'a>) -> Result<()> {
  let analysis = repo.merge_analysis(&[&commit])?;

  if analysis.0.is_fast_forward() {
    let refname = format!("refs/heads/{}", branch_name);
    match repo.find_reference(&refname) {
      Ok(mut rfrnc) => Ok(fast_forward(repo, &mut rfrnc, &commit)?),
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
    versio_err!("Can't pull: would not be a fast-forward.")
  } else {
    println!("Up to date.");
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
  println!("{}", msg);

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

//  pub struct FetchResults {
//    pub remote_name: Option<String>,
//    pub fetch_branch: String,
//    pub commit_oid: Option<Oid>
//  }
