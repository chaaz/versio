# VCS Levels

## Description

The **VCS Level** of a Versio command is the extent to which that
command interacts with your Version Control System (VCS) (e.g. Git).
There are four such levels, ordered from minimal to maximal:

- **None**: The command does not interact with VCS at all: no commits,
  pulls, merges, fetches, etc. are done. Tags are not searched for
  version numbers, etc.; nor are they updated with new version numbers.

- **Local**: The command interacts with the VCS system only at the local
  level: no network or remote interaction is allowed. Fetches, pulls
  and pushes are not done: not even with tags. No effort is made to
  ensure that the local repository is synchronized with any remote.

- **Remote**: The command interacts fully with the VCS system, including
  a guarantee that the local repository is fully synced with the remote
  both before and after the command executes.

- **Smart**: As "Remote", but also applies intelligence that requires
  interaction with other VCS-related entities e.g. the GitHub API. For
  example, commits in a GitHub-based repo can be grouped by Pull
  Request (see [PR Scanning](./pr_scanning.md)).

### Vs Dry Run

The VCS Level is distinct from the idea of a "dry run". A "dry run" flag
prohibits a command from writing anything at any level, but doesn't
affect what is read. For example, the flags `--vcs-level=smart
--dry-run` can still read data from the GitHub remote and API, but will
not commit any changes. Using `--vcs-level=local` without `--dry-run`
will not read any data from the remote, nor will it write to the remote:
however, it may still write to the filesystem, and commit and tag any
changes to the local repository.

### Vs Pause

The VCS Level is distinct from the idea of a "pause". The "pause" flag
exits a command just before executing a stage (e.g. "commit"), but
otherwise doesn't affect operations or have any effect on the VCS level.
In fact, you can supply a different VCS level to commands with the
`--pause` and `--resume` commands, and that level will apply for the
portion of the operation it applies to.

You could, for example, run `versio -l smart release --pause commit` to
gather information and write new versions and changelogs based on pull
request information from the remote GitHub API, but then run `versio -l
local release --resume` to commit and tag only the local repo.

### Vs Current

The `--no-current` flag only works on some commands, and only when the
final VCS level is calculated as "local" (see "Calculation" below).
Normally all commands at the "local" level will verify that the repo is
current: i.e., it does not contain local modifications or untracked
files. Some commands don't make any changes though, so it may be safe to
run them without this check, depending on your workflow.

At a VCS level of "remote" or higher, the check is always done (the
`--no-current` flag is ignored) because only a current repo can be
reconciled with remote changes. At the level of "none", the VCS isn't
consulted in any case, so the flag is effectively ignored.

## Calculation

Every Versio command except for `init` and `schema` calculates the final
VCS level using three inputs, each of which is itself a range of levels:

1. The _preferred_ range is given by the user, or by the command itself
   if the user doesn't provide it.
1. The _required_ range is the VCS range in which the command can
   operate, and is provided by the command. Some commands can only act
   if they can interact with the VCS system in certain ways. The max of
   the required range is usually the highest value, "Smart".
1. The _detected_ range is the VCS levels supported by the current
   working directory. Is it in a repo, does the repo have a remote, etc.
   The min of the detected range is usually the lowest value, "None".

In the presence of these three ranges, the final VCS level is calculated
as the maximum of the intersection of all three ranges. For example, if
the preferred range is [Local, Remote], and the required range is
[Local, Smart], and the detected range is [None, Remote], then the
highest intersected value is 'Remote', which is what the command will
run as.

If there is no common intersection of the three ranges, then the command
will immediately fail without any attempt to read or write anything.

The `init` and `schema` commands do not interact with VCS, and so ignore
all VCS levels.

Most Versio commands try to find the `root` of a repository to run in:
this is either the base directory of the local VCS (if any is detected),
or it's the nearest (inclusive) ancestor from the current working
directory that contains a `.versio.yaml` config file. If neither such
directory can be found, then the current directory is used.

## Detection

Currently, Git is the only VCS that Versio understands; it creates the
detected range like this:

- The minimum of the range is always "None". The maximum of the range is
  at least "None".
- If the working directory is a local working directory, and if the
  directory is checked out of a branch, then the maximum is at least
  "Local".
- Additionally, if the current branch has a configured remote, or if the
  repository itself has exactly one remote, then the maximum is at least
  "Remote".
- Additionally, if the remote URL starts with "https://github.com/" or
  "git@github.com:", then the maximum is "Smart".

## Options

Normally, you don't have to do anything with VCS levels: the best level
for a command is picked naturally. However, you can alter the min and
maximum of the preferred range when you run Versio. There are three
command-line options you can use to set the preferred range:

- `vcs-level` (`-l`): This allow you specify both the max and min of the
  preferred range in one shot. There are six possible arguments:
  - The discreet levels `none`, `local`, `remote`, or `smart`, which
    sets both the min and max of the preferred range to the given value.
  - `max`, which sets the minimum to `none` and the maximum to `smart`.
    This runs the command at the maximum allowable level, even if the
    default for a command is lower.
  - `auto`, which declines to set a default range, and allows the
    command to set the default. This is the same as not providing any
    default at all.

- `vcs-level-min` (`-m`) and `vcs-level-max` (`-x`): You must use these
  options together, and can't use them with `vcs-level`. These manually
  set the minimum and maximum level of the preferred range to one of
  their four possible values `none`, `local`, `remote`, or `smart`. If
  you set the max level below the min value, the preferred range is
  considered empty, and the command will fail.

## Tips

- Use `vcs-level-max=local` to avoid incurring any network traffic.

- Use `versio -l local -c <command>` to run a versio command without
  worrying about the state of your repository.

- Use `vcs-level-max=remote` to avoid using the GitHub API. All commands
  can operate at this level, although your changelogs and sizing
  calculation might suffer because of the lack of PRs/unsquash.
