//! Interactions with git.

use crate::either::IterEither2 as E2;
use crate::error::Result;
use git2::build::CheckoutBuilder;
use git2::{
  AnnotatedCommit, AutotagOption, Blob, Commit, Cred, Diff, DiffOptions, FetchOptions, ObjectType, Oid, PushOptions,
  Reference, ReferenceType, Remote, RemoteCallbacks, Repository, RepositoryState, ResetType, Signature, Status,
  StatusOptions, Time, RepositoryOpenFlags, Object, Index
};
use std::cmp::min;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::ffi::OsStr;

const PREV_TAG_NAME: &str = "versio-prev";

pub struct GithubInfo {
  owner_name: String,
  repo_name: String
}

impl GithubInfo {
  pub fn new(owner_name: String, repo_name: String) -> GithubInfo { GithubInfo { owner_name, repo_name } }
  pub fn owner_name(&self) -> &str { &self.owner_name }
  pub fn repo_name(&self) -> &str { &self.repo_name }
}

pub struct Repo {
  repo: Repository,

  fetches: HashMap<String, Oid>,
  branch_name: String,
  remote_name: Option<String>
}

impl Repo {
  pub fn open<P: AsRef<Path>>(path: P) -> Result<Repo> {
    let flags = RepositoryOpenFlags::empty();
    let repo = Repository::open_ext(path, flags, std::iter::empty::<&OsStr>())?;
    let fetches = HashMap::new();

    let branch_name = {
      let head_ref = repo.find_reference("HEAD").map_err(|e| versio_error!("Couldn't resolve head: {:?}.", e))?;
      if head_ref.kind() != Some(ReferenceType::Symbolic) {
        return versio_err!("Not on a branch.");
      } else {
        let mut branch_name = head_ref.symbolic_target().ok_or_else(|| versio_error!("Branch is not named."))?;
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

  pub fn prev(&self) -> Slice { self.slice(PREV_TAG_NAME.to_string()) }
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

    path.map(|path| {
      let len = path.len();
      let path = if path.ends_with(".git") { &path[0 .. len - 4] } else { path };

      let slash = path.char_indices().find(|(_, c)| *c == '/').map(|(i, _)| i);
      let slash = slash.ok_or_else(|| versio_error!("No slash found in github path \"{}\".", path))?;

      Ok(GithubInfo::new(path[0 .. slash].to_string(), path[slash + 1 ..].to_string()))
    }).transpose()
  }

  pub fn fetch(&self) -> Result<Option<Oid>> {
    self.slice(self.branch_name.to_string()).fetch()
  }

  pub fn pull(&self) -> Result<()> {
    if let Some(oid) = self.fetch()? {
      self.merge_after_fetch(oid)?;
    }
    Ok(())
  }

  pub fn push_changes(&self) -> Result<()> {
    if let Some(index) = self.add_all_modified()? {
      let tree_oid = index.write_tree()?;
      self.commit_tree(tree_oid)?;
      self.update_tag()?;
      self.push()?;
    }
    // TODO: push the tag regardless

    Ok(())
  }

  /// Return all commits as if `git rev-list from_sha..to_sha`, along with the earliest time in that range.
  pub fn dated_revlist(&self, from_sha: &str, to_sha: &str) -> Result<(Vec<String>, Time)> {
    let mut revwalk = self.repo.revwalk()?;
    revwalk.hide(self.repo.revparse_single(from_sha)?.id())?;
    revwalk.push(self.repo.revparse_single(to_sha)?.id())?;

    revwalk
      .try_fold::<_, _, Result<Option<(Vec<String>, Time)>>>(None, |v, oid| {
        let oid = oid?;
        if let Some((mut oids, v)) = v {
          oids.push(oid.to_string());
          let t = min(v, self.repo.find_commit(oid)?.time());
          Ok(Some((oids, t)))
        } else {
          let oids = vec![oid.to_string()];
          let t = self.repo.find_commit(oid)?.time();
          Ok(Some((oids, t)))
        }
      })
      .transpose()
      .ok_or_else(|| versio_error!("No commits found in {}..{}", from_sha, to_sha))?
  }

  pub fn commits_between<'a>(
    &'a self, from_sha: &str, to_sha: &str
  ) -> Result<impl Iterator<Item = Result<CommitInfo<'a>>> + 'a> {
    let mut revwalk = self.repo.revwalk()?;
    revwalk.hide(self.repo.revparse_single(from_sha)?.id())?;
    revwalk.push(self.repo.revparse_single(to_sha)?.id())?;

    Ok(revwalk.map(move |id| Ok(CommitInfo::new(&self.repo, self.repo.find_commit(id?)?))))
  }

  fn merge_after_fetch(&self, fetch_oid: Oid) -> Result<()> {
    let fetch_commit = self.repo.find_annotated_commit(fetch_oid)?;

    let mut status_opts = StatusOptions::new();
    status_opts.include_ignored(false);
    status_opts.include_untracked(true);
    status_opts.exclude_submodules(false);
    if self.repo.statuses(Some(&mut status_opts))?.iter().any(|s| s.status() != Status::CURRENT) {
      return versio_err!("Can't pull: repository isn't current.");
    }

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

  fn update_tag(&self) -> Result<()> {
    let obj = self.repo.revparse_single("HEAD")?;
    self.repo.tag_lightweight(PREV_TAG_NAME, &obj, true)?;
    Ok(())
  }

  fn push(&self) -> Result<()> {
    if let Some(remote_name) = &self.remote_name {
      let mut remote = self.repo.find_remote(remote_name)?;
      let bchref = format!("refs/heads/{}", self.branch_name);
      let tagref = format!("refs/tags/{}", PREV_TAG_NAME);

      let mut cb = RemoteCallbacks::new();

      cb.credentials(|_url, username_from_url, _allowed_types| Cred::ssh_key_from_agent(username_from_url.unwrap()));

      // TODO: rollback the tag if the heads didn't succeed.
      cb.push_update_reference(|rref, status| {
        if let Some(status) = status {
          println!("Couldn't push reference {}: {}", rref, status);
          return Err(git2::Error::from_str(&format!("Couldn't push reference {}: {}", rref, status)));
        }
        Ok(())
      });

      let mut push_opts = PushOptions::new();
      push_opts.remote_callbacks(cb);

      remote.push(&[&bchref, &tagref], Some(&mut push_opts))?;
    }
    Ok(())
  }

  fn find_last_commit(&self) -> Result<Commit> {
    let obj = self.repo.head()?.resolve()?.peel(ObjectType::Commit)?;
    obj.into_commit().map_err(|o| versio_error!("Not a commit, somehow: {}", o.id()))
  }
}

struct Slice<'r> {
  repo: &'r Repo,
  refspec: String
}

impl<'r> Slice<'r> {
  pub fn reslice(&self, refspec: String) -> Slice { self.repo.slice(refspec) }

  pub fn has_blob<P: AsRef<Path>>(&self, path: P) -> bool { self.object(path).is_ok() }

  pub fn blob<P: AsRef<Path>>(&self, path: P) -> Result<Blob> {
    let obj = self.object(path)?;
    obj.into_blob().map_err(|e| versio_error!("Not a blob: {} : {:?}", path.as_ref().to_string_lossy(), e))
  }

  pub fn object<P: AsRef<Path>>(&self, path: P) -> Result<Object> {
    let path_string = path.as_ref().to_string_lossy();
    Ok(self.repo.repo.revparse_single(&format!("{}:{}", &self.refspec, &path_string))?)
  }

  pub fn oid(&self) -> Result<String> {
    let obj = self.repo.repo.revparse_single(&format!("{}^{{}}", self.refspec))?;
    Ok(obj.id().to_string())
  }

  pub fn date(&self) -> Result<Time> {
    let obj = self.repo.repo.revparse_single(&format!("{}^{{}}", self.refspec))?;
    let commit = obj.into_commit().map_err(|o| versio_error!("\"{}\" isn't a commit.", o.id()))?;
    Ok(commit.time())
  }

  pub fn fetch(&self) -> Result<Option<Oid>> {
    if let Some(oid) = self.repo.fetches.get(&self.refspec).cloned() {
      return Ok(Some(oid));
    }

    let state = self.repo.repo.state();
    if state != RepositoryState::Clean {
      // Don't bother if we're in the middle of a merge, rebase, etc.
      return versio_err!("Can't pull: repository {:?} isn't clean.", state);
    }

    if let Some(remote_name) = &self.repo.remote_name {
      let mut remote = self.repo.repo.find_remote(remote_name)?;
      let oid = do_fetch(&self.repo.repo, &mut remote, &[&self.refspec])?;
      self.repo.fetches.insert(self.refspec.to_string(), oid);
      Ok(Some(oid))
    } else {
      Ok(None)
    }
  }
}

pub struct CommitInfo<'a> {
  repo: &'a Repository,
  commit: Commit<'a>
}

impl<'a> CommitInfo<'a> {
  pub fn new(repo: &'a Repository, commit: Commit<'a>) -> CommitInfo<'a> { CommitInfo { repo, commit } }

  pub fn kind(&self) -> String { extract_kind(self.commit.summary().unwrap_or("-")) }

  pub fn id(&self) -> String { self.commit.id().to_string() }

  pub fn files(&self) -> Result<impl Iterator<Item = String> + 'a> {
    if self.commit.parents().len() == 1 {
      let parent = self.commit.parent(0)?;
      let ptree = parent.tree()?;
      let ctree = self.commit.tree()?;
      let diff = self.repo.diff_tree_to_tree(Some(&ptree), Some(&ctree), Some(&mut DiffOptions::new()))?;
      let iter = DeltaIter::new(diff);
      Ok(E2::A(iter.map(move |path| path.to_string_lossy().into_owned())))
    } else {
      Ok(E2::B(std::iter::empty()))
    }
  }
}

/// Fetch the given refspecs (and all tags) from the remote.
fn do_fetch<'a>(repo: &'a Repository, remote: &'a mut Remote, refs: &[&str]) -> Result<Oid> {
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
  println!("Fetching {:?} from {}", refs, remote.name().unwrap());
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

  let fetch_head = repo.find_reference("FETCH_HEAD")?;
  Ok(repo.reference_to_annotated_commit(&fetch_head)?.id())
}

/// Merge the given commit into the working directory, but only if it's fast-forward-able.
fn do_merge<'a>(repo: &'a Repository, branch_name: &str, commit: AnnotatedCommit<'a>) -> Result<()> {
  let analysis = repo.merge_analysis(&[&commit])?;

  if analysis.0.is_fast_forward() {
    println!("Updating branch (fast forward)");
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

struct DeltaIter<'repo> {
  diff: Diff<'repo>,
  len: usize,
  on: usize,
  on_new: bool
}

impl<'repo> Iterator for DeltaIter<'repo> {
  type Item = PathBuf;

  fn next(&mut self) -> Option<PathBuf> {
    while let None = self.current() {
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
  head_oid: String,
  base_oid: String,
  base_time: Time,
  commits: Vec<String>,
  excludes: Vec<String>,
  best_guess: bool
}

impl FullPr {
  pub fn lookup(repo: &Repo, head: String, base: String, number: u32) -> Result<FullPr> {
    let full_pr = match repo.slice(head.clone()).fetch() {
      Err(e) => {
        println!("Couldn't fetch {}: using best-guess instead: {:?}", head, e);
        FullPr {
          number,
          head_oid: head,
          base_oid: base,
          base_time: Time::new(0, 0),
          commits: Vec::new(),
          excludes: Vec::new(),
          best_guess: true
        }
      }

      Ok(_) => {
        let base_time = repo.slice(base.clone()).date()?;
        let (commits, early) = repo.dated_revlist(&base, &head)?;

        FullPr {
          number,
          head_oid: head,
          base_oid: base,
          base_time: min(base_time, early),
          commits,
          excludes: Vec::new(),
          best_guess: false
        }
      }
    };

    Ok(full_pr)
  }

  pub fn number(&self) -> u32 { self.number }
  pub fn head_oid(&self) -> &str { &self.head_oid }
  pub fn base_oid(&self) -> &str { &self.base_oid }
  pub fn commits(&self) -> &[String] { &self.commits }
  pub fn excludes(&self) -> &[String] { &self.excludes }
  pub fn best_guess(&self) -> bool { self.best_guess }
  pub fn has_exclude(&self, oid: &str) -> bool { self.excludes.iter().any(|c| c == oid) }

  pub fn span(&self) -> Span { Span::new(self.number, self.head_oid.clone(), self.base_time, self.base_oid.clone()) }

  pub fn add_commit(&mut self, commit_oid: &str) {
    if !self.commits.iter().any(|c| c == commit_oid) {
      self.commits.push(commit_oid.to_string());
    }
  }

  pub fn add_exclude(&mut self, commit_oid: &str) {
    if !self.excludes.iter().any(|c| c == commit_oid) {
      self.excludes.push(commit_oid.to_string());
    }
  }

  pub fn contains(&self, commit_oid: &str) -> bool { self.commits.iter().any(|c| c == commit_oid) }
}

pub struct Span {
  number: u32,
  end: String,
  since: Time,
  begin: String
}

impl Span {
  pub fn new(number: u32, end: String, since: Time, begin: String) -> Span { Span { number, end, since, begin } }

  pub fn number(&self) -> u32 { self.number }
  pub fn end(&self) -> &str { &self.end }
  pub fn begin(&self) -> &str { &self.begin }
  pub fn since(&self) -> &Time { &self.since }
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
