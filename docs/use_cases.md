# Common Use Cases

These are some of the common ways that you might want to use Versio in
your own development. If you find a new or novel way to use Versio,
please let us know!

## Quick Start

Get up and running quickly with Versio, and get a brief introduction to
what it does. This example assumes a standard Node.js/NPM layout, but
Versio can handle lots of different project types.

If you don't have rust installed, you can't use `cargo install`.
Instead, download a binary for your platform directly from the [Releases
page](https://github.com/chaaz/versio/releases).

- Install versio:
  ```
  $ cargo install versio  # or download a pre-built binary to your PATH
  ```
- Create and commit a simple config file:
  ```
  $ git pull
  $ versio init  # this creates .versio.yaml
  $ git add .versio.yaml
  $ git commit -m "build: add versio management"
  $ git push
  ```
- If you want to use the GitHub API for [PR scanning](./pr_scanning.md), you'll need to
  update your `~/.versio.rc.toml` file: See the
  [Reference](./reference.md#github-api).
- After some [conventional
  commits](https://www.conventionalcommits.org/), update it:
  ```
  $ versio run
  Executing plan:
    project : 1.0.1 -> 1.1.0
  Changes committed and pushed.
  ```

## View Project Versions

Since Versio knows where all your project versions are stored, it can
output them for you. It can even tell you the versions of projects as
they were set by the last execution of `versio run`.

- View current versions in "wide" format (which shows project IDs):
  ```
  $ versio show -w
  1. myproject    : 1.0.1
  2. otherproject : 1.0.1
  ```
- View a specific project version
  ```
  $ versio get --id 1
  myproject : 1.0.1
  ```
- Print just the version number
  ```
  $ versio get --id 1 -v
  1.0.1
  ```
- Show the latest-released versions:
  ```
  $ versio show --prev
  myproject    : 1.0.1
  otherproject : 1.0.1
  ```

  If you rely solely on Versio to update project numbers for you, then
  the last-released version will usually match the current version.

## Change a Project Version (solo project)

If you have a single project configured, and you want to manually view
and set its version.

- Change the version of the only project:
  ```
  $ versio set --value 1.2.3
  ```

By default, `set` has a default VCS level of `none` (see [VCS
Levels](./vcs_levels.md)), which means that it won't commit, tag, or
push your new version to a remote. This can be vexing for "version:
tags" types of projects, which keeps their version numbers only in tags.
To change a version in the VCS, you can use force a different level,
like this:

```
$ versio -l remote set --value 1.2.3
```

This will not only change the tags in the VCS, but also commit and push
any version changes in files (both locally and on the remote).

## Change a Project Version (multiple projects)

If you have more than one project configured, and you must know the ID
or the name of the project you want to change.

- Change the version of a specified project:
  ```
  $ versio set --id 1 --value 1.2.3
  ```

By default, `set` has a default VCS level of `none` (see [VCS
Levels](./vcs_levels.md)), which means that it won't commit, tag, or
push your new version to a remote. This can be vexing for "version:
tags" types of projects, which keeps their version numbers only in tags.
To change a version in the VCS, you can use force a different level,
like this:

```
$ versio -l remote set --id 1 --value 1.2.3
```

This will not only change the tags in the VCS, but also commit and push
any version changes in files (both locally and on the remote).

## Create Configuration

To start using Versio, you should create a `.versio.yaml` config file in
your repo. Use the following command to do so. Make sure you're in the
top-level directory of your repository (or the top-level directory of
your non-version-controlled monorepo) when you do so:

```
$ versio init
```

This will scan your repo for existing projects, and create a new config
file with each of those projects listed. If you change later add,
remove, or change the location of your projects, you should edit this
file by hand to keep it up-to-date.

## CI Premerge

You can use Versio to check that a branch is ready to be merged to your
deployment branch. Your CI pipeline can run `versio check` to ensure
that the `.versio.yaml` file is properly configured, and can `versio
plan` to log the version changes which will be applied once merged.

## CI Merge

As part of your CI/CD pipeline, you can create an action to execute
`versio run`, which will update the version numbers, generate
changelogs, and commit and push all changes. You can set this action to
run automatically when a branch has been merged to a release branch, or
at any other time you want your software to be released.

It's important to note that nothing can be pushed to the release branch
during the short time that Versio is running, or `versio run` will fail.
There are a number of ways you can deal with this: from locking the
branch while Versio is running; to creating a pre-release branch to
separate merges from the release process; to simply ignoring the problem
and manually re-running the CI action if it gets stuck; and more. The
strategy you use is dependent on the specifics of your organization and
CI/CD process.

<!--
## CD Deploy

> TODO

`versio publish`

-->
