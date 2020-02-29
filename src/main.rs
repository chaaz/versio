mod error;

use git2::Repository;
use crate::error::Result;

fn main() -> Result<()> {
  let repo = Repository::open(".")?;

  // PULL:

  let head = repo.head()?;
  println!("Name is {}", head.name().unwrap());

  let obj = head.resolve()?.peel_to_commit()?;
  let our_commit = obj.into_commit()?;

  let remote = repo.find_remote("origin")?;

  // let fetch_opts = FetchOptions::new().remote_callbacks(...)
  remote.fetch(&["refs/tags/*", &format!("refs/heads/{}", current_brnc)], None, None)?;

  let reference = repo.find_reference("FETCH_HEAD")?;
  let their_commit = reference.peel_to_commit()?;

  let _index = repo.merge_commits(&our_commit, &their_commit, None)?;


  // OLD

  let remote = repo.find_remote("origin")?;

  let fetch_opts = OPTIONS_INIT;

  remote.fetch(&["refs/tags/**", &format!("refs/branches/{}", current_brnc)], Some(fetch_opts), None)?;

  repo.fetchhead_foreach(|name, url, oid, is_merge, payload| {
    if is_merge {
      strcpy_s( branchToMerge, 100, name );
      memcpy( &branchOidToMerge, oid, sizeof( git_oid ) );
    }
  });

  let merge_opts = GET_MERGE_OPTIONS_INIT;
  let checkout_opts = GIT_CHECKOUT_OPTIONS_INIT;

  let heads[0] = repo.annotated_commit_lookup(&branchOidToMerge)?;
  repo.merge(heads, 1, merge_opts, checkout_opts)?;



  println!("Found {} {}", last.kind().unwrap(), last.short_id()?.as_str().unwrap());
  Ok(())
}
