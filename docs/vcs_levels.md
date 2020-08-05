# VCS Levels / Dry run

- `dry-run` flag prohibits writing to the file system, git local, git
  remote, or github API. Everything can still be read just fine, though.
  - github API currently doesn't write in any case

- Generally, writes are queued up by commands and committed at the end
  of a command; `dry-run` only suppresses the final commit.

- If you don't have write permissions to file/git/remote/github, you
  won't see those errors while using `dry-run`.

- `vcs-level` options are different from the `dry-run` flag, which only
  affects writability. `--vcs-level=smart --dry-run` will still read
  data from github for remote and API: it just won't write to files,
  git, or github. Conversely, `--vcs-level=local` without `dry-run` will
  read and write to the file system and local git, but won't read
  anything or write anything to the remote or github API.

- If you don't have read or write permissions to git/remote/github, you
  won't see those errors while using a lower level `vcs-level`.

- levels:

  - `none`: no vcs interaction at all.
  - `local`: interact with vcs locally: no network or remote
    interaction.
  - `remote`: dumb remote interaction: push/pulls, ensuring repo is
    current.
  - `smart`: smart remote interaction: uses github API to group by PR,
    unsquash commits, etc.

- `vcs-level`
  - can't be used with `min` or `max`
  - `none|local|remote|smart`: sets `min` and `max` to same.
  - `max`: sets `min=none` and `max=smart`.
  - `auto`: default: command's choice

- option `vcs-level-max`
  - requires `vcs-level-min`
  - `none|local|remote|smart`: sets the maximum acceptable interaction:
    will find maximum level that does not exceed this level, even if
    available.

- option `vcs-level-min`
  - requires `vcs-level-max`
  - `none|local|remote|smart`: sets the minimum acceptable interaction:
    will complain and exit if the level can't be reached.

- some commands have a default min and max values: e.g. `set` has
  `max=none`, which are overriden by your choices except `auto`.

- Some commands have command-level min and max values which can *narrow*
  your choice: the command-level min represents what's required for the
  command to run: the command-level max represents the features that are
  used.

- if you don't use `auto`, some things might not go as
  planned.
  - you might tag locally, but not push the tags--your local tags could
    then be overriden on next pull (which could happen the next time you
    run versio).
  - you could change the local filesystem, but not commit or tag
    locally, which could confuse versio as to what the actual current
    version is.
  - you could do things that the command normally isn't thought of as
    doing: `level=max` for example, means that `set` will commit, tag,
    and push.

- using `vcs-level-max=local` will usually not incur any network
  traffic.

- using `vcs-level-max=remote` is "safe" if you don't like using the
  github API. All commands can run at operate at `max=remote`, although
  your changelogs and sizing might suffer because of the lack of
  PRs/unsquash.
