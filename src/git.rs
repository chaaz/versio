//! Interactions with git.

use crate::either::IterEither2 as E2;
use crate::error::Result;
use git2::build::CheckoutBuilder;
use git2::{
  AnnotatedCommit, AutotagOption, Blob, Commit, Cred, Diff, DiffOptions, FetchOptions, ObjectType, Oid, PushOptions,
  Reference, ReferenceType, Remote, RemoteCallbacks, Repository, RepositoryState, ResetType, Signature, Status,
  StatusOptions, Time
};
use std::cmp::min;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};

const PREV_TAG_NAME: &str = "versio-prev";

pub struct FetchResults {
  pub remote_name: Option<String>,
  pub fetch_branch: String,
  pub commit_oid: Option<Oid>
}

pub fn has_prev_blob<P: AsRef<Path>>(repo: &Repository, path: P) -> Result<bool> {
  let path_string = path.as_ref().to_string_lossy();
  let obj = repo.revparse_single(&format!("{}:{}", PREV_TAG_NAME, &path_string)).ok();
  Ok(obj.is_some())
}

pub fn prev_blob<P: AsRef<Path>>(repo: &Repository, path: P) -> Result<Option<Blob>> {
  let path_string = path.as_ref().to_string_lossy();
  let obj = repo.revparse_single(&format!("{}:{}", PREV_TAG_NAME, &path_string)).ok();
  obj.map(|obj| obj.into_blob().map_err(|e| versio_error!("Not a file: {} : {:?}", path_string, e))).transpose()
}

pub fn github_owner_name_branch(repo: &Repository) -> Result<Option<(String, String, String)>> {
  let (remote_name, branch) = get_name_and_branch(repo, None, None)?;
  let remote_name = match remote_name {
    Some(remote_name) => remote_name,
    None => return Ok(None)
  };
  let remote = repo.find_remote(&remote_name)?;

  let url = remote.url().ok_or_else(|| versio_error!("Invalid utf8 remote url."))?;
  let owner_name = if url.starts_with("https://github.com/") {
    Some(&url[19 ..])
  } else if url.starts_with("git@github.com:") {
    Some(&url[15 ..])
  } else {
    None
  };

  let owner_name = owner_name.and_then(|owner_name| {
    let len = owner_name.len();
    let owner_name = if owner_name.ends_with(".git") { &owner_name[0 .. len - 4] } else { owner_name };

    let slash = owner_name.char_indices().find(|(_, c)| *c == '/').map(|(i, _)| i);
    slash.map(|slash| (owner_name[0 .. slash].to_string(), owner_name[slash + 1 ..].to_string()))
  });

  Ok(owner_name.map(|owner_name| (owner_name.0, owner_name.1, branch)))
}

pub fn prev_tag_oid(repo: &Repository) -> Result<String> {
  let obj = repo.revparse_single(&format!("{}^{{}}", PREV_TAG_NAME)).ok();
  Ok(obj.map(|obj| obj.id().to_string()).unwrap_or_else(|| "^".to_string()))
}

pub fn get_date(repo: &Repository, sha: &str) -> Result<Time> {
  let obj = repo.revparse_single(&format!("{}^{{}}", sha))?;
  let commit = obj.into_commit().map_err(|o| versio_error!("Object {} isn't a commit.", o.id()))?;
  Ok(commit.time())
}

pub fn fetch(repo: &Repository, remote_name: Option<&str>, remote_branch: Option<&str>) -> Result<FetchResults> {
  let (remote_name, fetch_branch) = get_name_and_branch(repo, remote_name, remote_branch)?;

  let state = repo.state();
  if state != RepositoryState::Clean {
    return versio_err!("Can't pull: repository {:?} isn't clean.", state);
  }

  let commit_oid = match &remote_name {
    Some(remote_name) => {
      let mut remote = repo.find_remote(remote_name)?;
      let fetch_commit = do_fetch(&repo, &[&fetch_branch], &mut remote)?;
      fetch_commit.map(|c| c.id())
    }
    None => None
  };

  Ok(FetchResults { remote_name, fetch_branch, commit_oid })
}

pub fn merge_after_fetch(repo: &Repository, fetch_results: &FetchResults) -> Result<()> {
  if let Some(fetch_commit_oid) = &fetch_results.commit_oid {
    let fetch_commit = repo.find_annotated_commit(*fetch_commit_oid)?;

    let mut status_opts = StatusOptions::new();
    status_opts.include_ignored(false);
    status_opts.include_untracked(true);
    status_opts.exclude_submodules(false);
    if repo.statuses(Some(&mut status_opts))?.iter().any(|s| s.status() != Status::CURRENT) {
      return versio_err!("Can't pull: repository isn't current.");
    }

    do_merge(repo, &fetch_results.fetch_branch, &fetch_commit)?;
  }

  Ok(())
}

pub fn add_and_commit(repo: &Repository, fetch_results: &FetchResults) -> Result<Option<Oid>> {
  let mut status_opts = StatusOptions::new();
  status_opts.include_ignored(false);
  status_opts.include_untracked(true);
  status_opts.exclude_submodules(false);

  let mut index = repo.index()?;
  let mut found = false;
  for s in repo.statuses(Some(&mut status_opts))?.iter().filter(|s| s.status().is_wt_modified()) {
    found = true;
    let path = s.path().ok_or_else(|| versio_error!("Bad path"))?;
    index.add_path(path.as_ref())?;
  }

  if found {
    // commit ...
    let add_oid = index.write_tree()?;
    let sig = Signature::now("Versio", "github.com/chaaz/versio")?;
    let tree = repo.find_tree(add_oid)?;
    let parent_commit = find_last_commit(&repo)?;

    let commit_oid = repo.commit(Some("HEAD"), &sig, &sig, "Updated versions by versio", &tree, &[&parent_commit])?;
    repo.reset(&repo.find_object(commit_oid, Some(ObjectType::Commit))?, ResetType::Mixed, None)?;

    // ... tag ...
    let obj = repo.revparse_single("HEAD")?;
    repo.tag_lightweight(PREV_TAG_NAME, &obj, true)?;

    // ... and push
    if let Some(remote_name) = &fetch_results.remote_name {
      let fetch_branch = &fetch_results.fetch_branch;
      let mut remote = repo.find_remote(remote_name)?;
      let bchref = format!("refs/heads/{}", fetch_branch);
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

    Ok(Some(commit_oid))
  } else {
    // TOOD: still push the new tag
    Ok(None)
  }
}

fn find_last_commit(repo: &Repository) -> Result<Commit> {
  let obj = repo.head()?.resolve()?.peel(ObjectType::Commit)?;
  obj.into_commit().map_err(|o| versio_error!("Not a commit, somehow: {}", o.id()))
}

fn do_fetch<'a>(repo: &'a Repository, refs: &[&str], remote: &'a mut Remote) -> Result<Option<AnnotatedCommit<'a>>> {
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

  let fetch_head = repo.find_reference("FETCH_HEAD").ok();
  Ok(fetch_head.map(|fetch_head| repo.reference_to_annotated_commit(&fetch_head)).transpose()?)
}

fn do_merge<'a>(repo: &'a Repository, remote_branch: &str, fetch_commit: &AnnotatedCommit<'a>) -> Result<()> {
  let analysis = repo.merge_analysis(&[fetch_commit])?;

  if analysis.0.is_fast_forward() {
    println!("Updating branch (fast forward)");
    let refname = format!("refs/heads/{}", remote_branch);
    match repo.find_reference(&refname) {
      Ok(mut r) => Ok(fast_forward(repo, &mut r, fetch_commit)?),
      Err(_) => {
        // Probably pulling in an empty repo; just set the reference to the commit directly.
        let message = format!("Setting {} to {}", remote_branch, fetch_commit.id());
        repo.reference(&refname, fetch_commit.id(), true, &message)?;
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

fn fast_forward(repo: &Repository, lb: &mut Reference, rc: &AnnotatedCommit) -> Result<()> {
  let name = match lb.name() {
    Some(s) => s.to_string(),
    None => String::from_utf8_lossy(lb.name_bytes()).to_string()
  };

  let msg = format!("Fast-forward: {} -> {:.7}", name, rc.id());
  println!("{}", msg);

  lb.set_target(rc.id(), &msg)?;
  repo.set_head(&name)?;
  // 'force' required to update the working directory; safe becaused we checked that it's clean.
  repo.checkout_head(Some(CheckoutBuilder::default().force()))?;
  Ok(())
}

pub fn get_name_and_branch(
  repo: &Repository, name: Option<&str>, branch: Option<&str>
) -> Result<(Option<String>, String)> {
  let remote_name = name.map(|s| Ok(Some(s.to_string()))).unwrap_or_else(|| {
    let remotes = repo.remotes()?;
    if remotes.is_empty() {
      Ok(None)
    } else if remotes.len() == 1 {
      Ok(Some(remotes.iter().next().unwrap().ok_or_else(|| versio_error!("Non-utf8 remote name."))?.to_string()))
    } else if remotes.iter().any(|s| s == Some("origin")) {
      Ok(Some("origin".to_string()))
    } else {
      versio_err!("Couldn't determine remote name.")
    }
  })?;

  let remote_branch = branch.map(|b| Ok(b.to_string())).unwrap_or_else(|| {
    let head_ref = repo.find_reference("HEAD").map_err(|e| versio_error!("Couldn't resolve head: {:?}.", e))?;
    if head_ref.kind() != Some(ReferenceType::Symbolic) {
      return versio_err!("Not on a branch.");
    } else {
      let mut branch_name = head_ref.symbolic_target().ok_or_else(|| versio_error!("Branch is not named."))?;
      if branch_name.starts_with("refs/heads/") {
        branch_name = &branch_name[11 ..];
      } else {
        return versio_err!("Current {} is not a branch.", branch_name);
      }
      Ok(branch_name.to_string())
    }
  })?;

  Ok((remote_name, remote_branch))
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

/// Return all commits as if `git rev-list from_sha..to_sha`, along with the earliest time in that range.
pub fn dated_shas_between(repo: &Repository, from_sha: &str, to_sha: &str) -> Result<(Vec<String>, Time)> {
  let mut revwalk = repo.revwalk()?;
  if let Ok(prev_spec) = repo.revparse_single(from_sha) {
    revwalk.hide(prev_spec.id())?;
  } else {
    println!("\"{}\" not found, searching all history.", PREV_TAG_NAME);
  }
  let head_spec = repo.revparse_single(to_sha)?;
  revwalk.push(head_spec.id())?;

  revwalk
    .try_fold::<_, _, Result<Option<(Vec<String>, Time)>>>(None, |v, oid| {
      let oid = oid?;
      if let Some((mut oids, v)) = v {
        let t = min(v, repo.find_commit(oid)?.time());
        oids.push(oid.to_string());
        Ok(Some((oids, t)))
      } else {
        let oids = vec![oid.to_string()];
        let t = repo.find_commit(oid)?.time();
        Ok(Some((oids, t)))
      }
    })
    .transpose()
    .ok_or_else(|| versio_error!("No commits found in {}..{}", from_sha, to_sha))?
}

pub fn commits_between<'a>(
  repo: &'a Repository, from_sha: &str, to_sha: &str
) -> Result<impl Iterator<Item = Result<CommitInfo<'a>>> + 'a> {
  let mut revwalk = repo.revwalk()?;
  let from_spec = repo.revparse_single(from_sha)?;
  revwalk.hide(from_spec.id())?;
  let head_spec = repo.revparse_single(to_sha)?;
  revwalk.push(head_spec.id())?;

  Ok(revwalk.map(move |id| Ok(CommitInfo::new(repo, repo.find_commit(id?)?))))
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
  pub fn lookup(repo: &Repository, head: String, base: String, number: u32) -> Result<FullPr> {
    let full_pr = match fetch(repo, None, Some(&head)) {
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
        let base_time = get_date(repo, &base)?;
        let (commits, early) = dated_shas_between(repo, &base, &head)?;

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
