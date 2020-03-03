use crate::error::Result;
use git2::build::CheckoutBuilder;
use git2::{
  AnnotatedCommit, AutotagOption, Cred, FetchOptions, Reference, Remote, RemoteCallbacks, Repository, RepositoryState,
  Status, StatusOptions
};
use std::env::var;
use std::io::{stdout, Write};
use std::path::Path;

pub fn pull_ff_only(remote_name: Option<&str>, remote_branch: Option<&str>) -> Result<()> {
  let remote_name = remote_name.unwrap_or("origin");
  let remote_branch = remote_branch.unwrap_or("master");
  let repo = Repository::open(".")?;

  let state = repo.state();
  if state != RepositoryState::Clean {
    return versio_err!("Can't pull: repository {:?} isn't clean.", state);
  }

  let mut status_opts = StatusOptions::new();
  status_opts.include_ignored(false);
  status_opts.include_untracked(true);
  status_opts.exclude_submodules(false);
  if repo.statuses(Some(&mut status_opts))?.iter().any(|s| s.status() != Status::CURRENT) {
    return versio_err!("Can't pull: repository isn't current.");
  }

  let mut remote = repo.find_remote(remote_name)?;

  let fetch_commit = do_fetch(&repo, &[remote_branch], &mut remote)?;
  do_merge(&repo, &remote_branch, fetch_commit)
}

fn do_fetch<'a>(repo: &'a Repository, refs: &[&str], remote: &'a mut Remote) -> Result<AnnotatedCommit<'a>> {
  let mut cb = RemoteCallbacks::new();

  cb.credentials(|_url, username_from_url, _allowed_types| {
    Cred::ssh_key(
      username_from_url.unwrap(),
      None,
      Path::new(&format!("{}/.ssh/id_rsa", var("HOME").unwrap())),
      Some("unVm7JekaHpvyefTJMHK")
    )
  });

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
  println!("Fetching {} for repo", remote.name().unwrap());
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
  Ok(repo.reference_to_annotated_commit(&fetch_head)?)
}

fn do_merge<'a>(repo: &'a Repository, remote_branch: &str, fetch_commit: AnnotatedCommit<'a>) -> Result<()> {
  let analysis = repo.merge_analysis(&[&fetch_commit])?;

  if analysis.0.is_fast_forward() {
    println!("Updating branch (fast forward)");
    let refname = format!("refs/heads/{}", remote_branch);
    match repo.find_reference(&refname) {
      Ok(mut r) => Ok(fast_forward(repo, &mut r, &fetch_commit)?),
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
    Ok(println!("Up to date."))
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
