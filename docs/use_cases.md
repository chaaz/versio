# Common Use Cases

These are some of the common ways that you might want to use Versio in
your own development. If you find a new or novel way to use Versio,
please let us know!

## Quick Start

Get up and running quickly with Versio, and get a brief introduction to
what it does. This example assumes a standard Node.js/NPM layout, but
Versio can handle lots of different project types.

If you don't have Rust installed, you can't use `cargo install`.
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
  $ git add .versio.yaml .gitignore
  $ git commit -m "build: add versio management"
  $ git push
  ```
- If you want to use the GitHub API for [PR scanning](./pr_scanning.md),
  you'll need to update your `~/.versio/prefs.toml` file: See the
  [Reference](./reference.md#github-api).
- After some [conventional
  commits](https://www.conventionalcommits.org/), update it:
  ```
  $ versio release
  Executing plan:
    project : 1.0.1 -> 1.1.0
  Changes committed and pushed.
  ```

## Switching a Repo

If you've been releasing your project for a while before switching to
Versio, you might not want to scan your entire project history the first
time you release with Versio. You can tag the commit of your latest
release to indicate that's where Versio should pick up:

```
$ git tag -f versio-prev <last_release_commit>
```

You can add some JSON to indicate the current version of projects. This
is especially useful for `version: tags` style projects that don't have
a manifest file which lists their version.

```
$ git tag -f -a -m '{"versions":{"1":"0.1.2","2":"5.2.1"}}' \
      versio-prev <last_release_commit>
```

You can leave off the `last_release_commit` argument if you want to
start releasing from the latest commit.

In lieu of (or in addition to) using JSON, you can create separate tags
on the latest commit that indicate the versions of your projects, using
the projects' tag prefixes.

```
$ git tag -f proj_1_tag_prefix-v0.1.2
$ git tag -f v5.2.1
```

## View Project Versions

Since Versio knows where all your project versions are stored, it can
output them for you. It can even tell you the versions of projects as
they were set by the last execution of `versio release`.

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

```
$ versio set --value 1.2.3
```

The next use case: "Change a Project Version (mutliple projects)" has
caveats to use this command on "version: tags" projects.

## Change a Project Version (multiple projects)

If you have more than one project configured, and you must know the ID
or the name of the project you want to change.

```
$ versio set --id 1 --value 1.2.3
```

### Tags projects

By default, `set` has a default VCS level of `none` (see [VCS
Levels](./vcs_levels.md)), so it won't commit, tag, or push your new
version to a remote. This works great on most projects, allowing to you
make quick changes to your manifest file. However, "version: tags"
projects have no manifest, and keep version numbers only in tags; `set`
by default performs no action for these. To change a version in the VCS,
you can use a different VCS level, like this:

```
$ versio -l max set --id 1 --value 1.2.3
```

## Create a New Configuration

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

The `init` command will not scan hidden directories or file, or
directories or files listed in any `.gitignore` files. If you want to
include projects in hidden or ignored locations, you'll have to add
those by hand to the resulting `.versio.yaml` file.

## CI/CD

### GitHub Action Matrixes

If you are using a monorepo, you may want to perform the same build step
on all your (for example) Node.js projects. You can build GitHub dynamic
matrixes using the `versio info` command, and then use those matrixes to
execute multiple projects in your repo. For example, if you wanted to
run 'npm test' in every project in your monorepo with the `npm` label:

```
jobs:
  project-matrixes:
    runs-on: ubuntu-latest
    outputs:
      npm-matrix: "${{ steps.find-npm-matrix.outputs.matrix }}"
    steps:
      - name: Checkout code
        uses: actions/checkout@v2
      - name: Get versio
        uses: chaaz/versio-actions/install@v1
      - name: Find npm matrix
        id: find-npm-matrix
        run: "echo \"::set-output name=matrix::{\\\"include\\\":$(versio -l none info -l npm -R -N)}\""
  npm-tests:
    needs: project-matrixes
    runs-on: ubuntu-latest
    strategy:
      matrix: "${{ fromJson(needs.project-matrixes.outputs.npm-matrix) }}"
    steps:
      - name: Checkout code
        uses: actions/checkout@v2
      - name: Run local tests
        run: npm test
        working-directory: "${{ matrix.root }}"
```

### Authorization

In most CI/CD environments, you may not have a credentials agent
available to negotiate credentials to your git repo and/or github APIs.
Instead, your should set the `GITHUB_USER` and `GITHUB_TOKEN`
environment variables. For example, GitHub Actions provides these values
to you in various places:

```
env:
  GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  GITHUB_USER: ${{ github.actor }}
```

## CI Pre-merge

You can use Versio to check that a branch is ready to be merged to your
deployment branch. Your CI pipeline can run `versio check` to ensure
that the `.versio.yaml` file is properly configured, and can `versio
plan` to log the version changes which will be applied once merged.

### GitHub Actions

Use the example snippet to build a workflow for pull requests that can
verify that Versio is configured correctly for all projects, and which
will print out all changes in the pr, and their possible effect on the
project(s) version numbers.

Note the use of `checkout@v2`, and the following `git fetch --unshallow`
command, which is necessary to fill in the git history before `versio`
is asked to analyze it. Also, we've provided a
`versio-actions/install@v1` command which installs the `versio` command
into the job. (Currently, the `versio-actions/install` action only works
for linux-based runners.)

```
---
name: pr
on:
  - pull_request
env:
  GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  GITHUB_USER: ${{ github.actor }}

jobs:
  versio-checks:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v2
      - name: Get versio
        uses: chaaz/versio-actions/install@v1
      - name: Fetch history
        run: git fetch --unshallow
      - name: Check projects
        run: versio check
      - name: Print changes
        run: versio plan
```

## CI Release

As part of your CI (continuous integration)/CD (continuous deployment)
pipeline, you can create an action to execute `versio release`, which
will update the version numbers, generate changelogs, and commit and
push all changes. You can set this action to run automatically when a
branch has been merged to a release branch, or at any other time you
want your software to be released.

### About Timing

It's important to note that nothing can be pushed to the release branch
during the short time that Versio is running, or else `versio release`
will fail. There are a number of ways you can deal with this: from
locking the branch while Versio is running; to creating a pre-release
branch to separate merges from the release process; to simply ignoring
the problem and manually re-running the CI action if it gets stuck; and
more. The strategy you use is dependent on the specifics of your
organization and CI/CD process.

### GitHub Actions

A GitHub Actions job that releases your projects might look something
like this:

```
jobs:
  versio-release:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v2
      - name: Get versio
        uses: chaaz/versio-actions/install@v1
      - name: Fetch history
        run: git fetch --unshallow
      - name: Generate release
        run: versio release
```
