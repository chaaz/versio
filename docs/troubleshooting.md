# Troubleshooting Versio

Versio is not a perfect program; even it was, it might not perfectly
handle the idiosyncrasies of your workflow. Still, we want Versio to
work for as many people as possible, so please try to work through
errors. If you can't, let us know what's wrong so that we can keep
things working for everyone.

## Reporting

If you think you've found a bug, or if you're frustrated by some
behavior and you want Versio to act better, let us know! We're using 
issue tracking on this this repository, so just open a issue.

While we always like feedback of any size and style, you'll have a much
better chance of being heard if you include the following in your
ticket:

- The current behavior. If possible, list a simple set of steps to
  consistently reproduce this behavior from scratch.
- The proximate cause. Does this happen on a brand-new repository? If
  not, what's the special thing about *your* repository that is causing
  the problem. Try to isolate what's going on: we probably don't have
  the time to comb through your 50,000-line project to find the one
  thing that's triggering the issue.
- The expected / hoped-for behavior. Be as precise as you can; write
  some example output or attach some files if you feel like it.
- The version / environment you're using. Versio itself goes through
  different versions! If you type `versio -V` you'll see which one
  you're using; please let us know. If you type `env`, you'll get a huge
  list of variables that represent your environment Versio is working
  in: send that as well, especially if you're reporting a bug or error.

  (Note: sometimes, sensitive information is stored in environment
  variables; this is usually not good practice, but it happens. You
  might want to read through your environment and edit the personal
  info, passwords, and other stuff you don't want to send. For example,
  if you change `MYSECRETPASS=abcde12345` to `MYSECRETPASS=****` before
  sending it, we'll understand.)
- Any other considerations that might be important. Are you running this
  on a 20-year old computer? Is this part of a CI/CD project you're
  putting together? Does the error only happen on Tuesdays with a full
  moon? Did you get your copy of Versio from a shady-looking figure in a
  back alley? This is all useful data to us.

You can also attach some technical notes: if your issue deals with a
particular command, you can run it with some environment variables like
this:

```
RUST_LOG=versio=trace RUST_BACKTRACE=1 versio <command>
```

Copy and paste the command you're running, along with everything that is
output, and put the whole thing in the issue; if the output is long, you
can attach it as a separate file. We promise that these logs make it *so
much* easier to track down your problem.

## Types of Errors

Broadly speaking, there are three ways that Versio can fail:

1. It will fail to run entirely, exiting with some kind of error.
1. It will incorrectly calculate the new version for one or more
   projects.
1. It will incorrectly write files, or commit, tag, or push the
   repository; or it will perform incomplete actions.

## Errors

Versio uses the powerful `error-chain` crate to track errors and
(possibly) generate a backtrace. It also uses `env_logger` to output
live log messages. You can take advantage of both of these using
environment variables. For example:

```
RUST_LOG=versio=trace RUST_BACKTRACE=1 versio <command>
```

This will generate logs at maximum verbosity and&mdash;if the program
exits with an error&mdash;a causal chain and back trace to the source of
the error. While the back trace is not useful to most users, it's
extremely helpful to provide when filing a bug report to the dev team.

You can read about [VCS Levels](./vcs_levels.md) if your error has
to do with VCS levels or ranges; some commands can't execute if the
preferred or detected VCS Level is insufficient.

## Bad Calculations

Sometimes, `versio` will run just fine, but will incorrectly calculate
the previous, current, or next version number for a project. There are
many possible reasons for this, ranging from misconfiguration, to
unexpected file formats, to a git branch or tag setup that the program
is just not capable of handling.

You can use the `RUST_LOG=versio=trace` environment variable as
mentioned above to get a thorough output of Versio's logic as it
performs its calculation: usually that's enough to understand why the
program arrives at the numbers it does.

In addition, there are a few commands and options specifically built to
provide insight:

- `versio show` and `versio show --prev` will output the
  current/previous version numbers.
- `versio plan --verbose` will print a full listing of all PRs, files,
  dependencies, locks, etc. that go into calculating version numbers;
  essentially the same data as is in a change log, even for projects
  that don't have a change log.
- `versio log` will write out all change logs. In addition to providing
  users with a nice listing of changes, the change log lists sizing for
  each PR and commit that goes into version number calculation.
- `versio files` will list each file that has been changed since the
  previous run, and the commit size in which the file was found. A full
  accounting of files may help you understand why a project ended up
  with a particular version.
- `versio run --dry-run` will prevent any writing from taking place,
  either on the filesystem or in the repository. Use this flag if you
  need to trace through a failed execution, or if you want to preview an
  execution without commitment.

The [VCS Level](./vcs_levels.md) may affect version calculation.
For example, if the level is "Local", changes that exist only on the
remote will not be considered. Or, if the level is "Remote" instead of
"Smart", then Versio will not perform PR unsquashing or PR grouping of
commits, which can affect the final version calculation.

You can view the current VCS level by setting the environment variable
`RUST_LOG=versio::vcs=trace`, which causes the program to out the VCS
level as it is calculated.

Another reason that Versio might have trouble with version numbers is
when it deals with a rebased, squashed, or otherwise replayed
repository; if the previous version tag is ever not an ancestor of the
current commit, it might cause Versio to search the VCS history
incorrectly. If this is the case, you should manually move and push the
previous version tag (which is `versio-prev` by default) to a more
suitable location.

## Bad or Incomplete Operations

Occasionally, Versio might improperly write to the filesystem, repo, or
tags. We try to avoid this by having Versio write everything as the very
last step, but sometimes errors are inevitable. Fortunately, you can
revert any incorrect changes by using Git itself: roll back to a
previous version of files, remove bad tags, etc.

Use the environment variable `RUST_LOG=versio=trace` to have Versio
output all the write attempts that are made while executing. This is
probably the best way to figure out why Versio might be behaving
incorrectly.

The [VCS Level](./vcs_levels.md) may affect how things are written.
For example, if the level is "Local", no changes to files or tags will
be pushed to the remote. If the level is "None", nothing will even be
committed! 

You can view the current VCS level by setting the environment variable
`RUST_LOG=versio::vcs=trace`, which causes the program to out the VCS
level as it is calculated.

You may have inadvertently used the `--dry-run` flag on some commands,
which prevents any writing being done at all: either to the file system,
or to the local or remote VCS.

It's possible that Versio is running in an environment where it does not
have the permissions to write, commit, and/or push its changes.

A similar problem is when Versio only performs some operations, but not
others. For example, if Versio might successfully write a file, but then
have trouble commit that change. Or, it might be able to push tags
changes, but be unable to push new commits. In such a case, you'll
usually see a Versio error, and that your repo has uncommitted or
not-yet-pushed changes: you can manually unwind or commit/push as you
see fit.
