mod error;

use git2::{Repository, MergeOptions, RemoteCallbacks, Cred, FetchOptions};
use crate::error::Result;

fn main() -> Result<()> {
  let repo = Repository::open(".")?;

  // PULL:

  let head = repo.head()?;
  println!("Head name is {}", head.name().unwrap()); // "refs/heads/branchname"

  if !head.is_branch() {
    println!("But it's not a branch, somehow. Exiting.");
    return versio_err!("Not a branch.");
  }

  let our_commit = head.resolve()?.peel_to_commit()?;

  let mut remote = repo.find_remote("origin")?;

  let mut callbacks = RemoteCallbacks::new();
  callbacks.credentials(|_url, username_from_url, _allowed_types| {
    Cred::ssh_key(
      username_from_url.unwrap(),
      None,
      std::path::Path::new(&format!("{}/.ssh/id_rsa", std::env::var("HOME").unwrap())),
      Some("unVm7JekaHpvyefTJMHK")
    )
  });
  let mut fetch_opts = FetchOptions::new();
  fetch_opts.remote_callbacks(callbacks);

  remote.fetch(&["refs/tags/versio", head.name().unwrap()], Some(&mut fetch_opts), None)?;

  let reference = repo.find_reference("FETCH_HEAD")?;
  let their_commit = reference.peel_to_commit()?;

  let mut merge_ops = MergeOptions::new();
  merge_ops.fail_on_conflict(true);
  let mut index = repo.merge_commits(&our_commit, &their_commit, Some(&merge_ops))?;
  if index.has_conflicts() {
    panic!("Shouldn't have conflicts.");
  }
  let oid = index.write_tree()?;

  // OLD

  // let remote = repo.find_remote("origin")?;

  // let fetch_opts = OPTIONS_INIT;

  // remote.fetch(&["refs/tags/**", &format!("refs/branches/{}", current_brnc)], Some(fetch_opts), None)?;

  // repo.fetchhead_foreach(|name, url, oid, is_merge, payload| {
  //   if is_merge {
  //     strcpy_s( branchToMerge, 100, name );
  //     memcpy( &branchOidToMerge, oid, sizeof( git_oid ) );
  //   }
  // });

  // let merge_opts = GET_MERGE_OPTIONS_INIT;
  // let checkout_opts = GIT_CHECKOUT_OPTIONS_INIT;

  // let heads[0] = repo.annotated_commit_lookup(&branchOidToMerge)?;
  // repo.merge(heads, 1, merge_opts, checkout_opts)?;


  // let last = repo.rev-parse("versio^{}")?;
  // println!("Found {} {}", last.kind().unwrap(), last.short_id()?.as_str().unwrap());

  Ok(())
}
