# Using Versio

You can use versio to manually get and set version numbers, but it also
seemlessly integrates with git and your CI/CD pipeline. Here's some
examples:

## Getting and Setting

For example, let's say you have a simple Node.js project, but the
project is nested in a subdirectory `codebase` with the following
`codebase/package.json`:

```json
{
  "version": "1.0.1",
  "... other properties": "..."
}
```

You also have a `.versio.yaml` at the root of your repository:

```yaml
projects:
  - name: codebase
    root: "codebase"
    id: 1
    version:
      file: "package.json"
      json: "version"
```

Versio has handlers for JSON, YAML, XML, and TOML files, as well as
plain files which contain only a version number, or any file which can
be scanned for a version number via a regular expression.

### Getting

With the config file in place, you can now _show_ versions of all your
projects:

```
$ versio show
codebase : 1.0.1
```

### Setting

You can also _set_ a version to a new value:

```
$ versio set --id 1 --value 1.2.3
```

Which would modify your `codebase/package.json` file:

```json
{
  "version": "1.2.3",
  "... other properties": "..."
}
```

If you only have one project, you don't need to supply the `--id`
option.

This is a pretty simple example, but you can imagine how useful it is to
manage multiple projects in the same repo.

By default, `set` has a default VCS level of `none` (see [VCS
Levels](./vcs_levels.md)), which means that it won't commit, tag, or
push your new version to a remote. This can be vexing for "version:
tags" types of projects, which keeps its version numbers only in tags.
To change a version in the VCS, you can use force a different level,
like this:

```
$ versio -l remote set --id 2 --value 1.2.3
```

## Git Integration

### The "prev" tag

Versio sometimes scans your commit history, and uses a tag named
`versio-prev` to indicate where it should start. This bounds the work it
has to do, and also allows incremental version changes over time. If
there is no such tag present, Versio will scan through your entire
commit history; if you don't want to do this, you should manually create
a `versio-prev` tag to point at the last commit where any version number
has changed.

### Clean and current

When operating on a repository, many commands won't work unless the
local repo is "clean" (not in the middle of a merge, rebase, conflict
resolution, etc) and "current" (all local files have been staged and
committed). If you get an error about being "not current", take steps in
your local repo to get yourself up-to-date, and try again.

### Conventional commits

In a `git` repository, `versio` is even more useful. You can assign
"includes" to your projects, and "sizes" to your repo:

```yaml
projects:
  - name: codebase
    id: 1
    root: codebase
    includes: ["**/*"]
    version:
      file: "package.json"
      json: "version"

sizes:
  use_angular: true
  major: [ breaking, incompatible ]
  minor: [ minor ]
  patch: [ "*" ]
  fail: [ "-" ]
```

Now, Versio can scan your commits to determine how it should increment
each project version.

A project is said to _cover_ a git commit if that commit has a file
change matching any glob pattern of the project's "includes" key. The
_size_ of a commit is determined by the type of its [conventional
commit](https://www.conventionalcommits.org/en/v1.0.0/) message, as it
maps to the "sizes" property in the Versio config.

### Running with Git

For example, let's say you're using the above config, and have a single
commit since the `versio-prev` tag:

```
b3ed0f0 feat: Change an important file
 codebase/src/misc.js | 6 ++++++
 1 file changed, 7 insertions(+)
```

Notice that the commit message starts with "feat": "feat" maps to the
"minor" size in our config. And the commit contains a file
`codebase/src/misc.js` which is covered by the `codebase/**/*` glob in
our single project.

We can run the Versio git integration like this:

```
$ versio run
Executing plan:
  codebase : 1.0.1 -> 1.1.0
Changes committed and pushed.
```

The `run` command will:

1. fetch the latest branches from the "origin" remote
1. merge the current branch from the remote into the clean and current
   working directory
1. scan through the git log for conventional commits since the
   `versio-prev` tag to generate a plan
1. find the size for each commit according to the plan
1. possibly update a changelog according to the plan
1. increment each project version by the maximum size of commits
   it includes
1. possibly generate a git tag for the new project version
1. commit those version increments (and maybe changelog)
1. re-assign the `versio-prev` tag (and maybe the new project tag) to
   that latest commit
1. push both the commit and the tag(s) to the remote

### Plans

A *plan* is created by Versio by reading the current and previous
versions of commits and configuration. It contains the size increment
for each project and the changes that have occurred. A plan is used to
generate a changelog for a project, and to determine what a project's
target version number should be.

Plans are built according to their configuration at each commit. For
example, if a commit lists a changed file `dir1/file.txt`, and if the
version of `.versio.yaml` *on that commit* has project 1 that includes
`"dir1/**/*"`, then that counts as a change to project 1, even if the
current version of `versio.yaml` does not include that file.

The consequence here is that you should always keep your `.versio.yaml`
file in sync with your projects. If you move a project into a different
directory, you should change the `root` property of that file in
`.versio.yaml`; if you add files to a property that you don't want
included/excluded, you should also change that projects
`includes`/`excludes` properties; etc. Changes to your project structure
should be committed on the same commit as matching changes to your
`.versio.yaml` file.

On the other hand, only the current `.versio.yaml` will be considered to
determine what sizes correspond to commit messages, and only currently
listed projects can have their version number incremented.

## CI/CD Integration

One useful application of Versio is in a CI/CD pipeline: you can call
`versio run` after a merge to the release branch. Since Versio will only
run on a clean and current branch, and will fail on any commit error, it
is guaranteed to keep versions up-to-date when it succeeds. Versio will
not interfere with manual version increments that are already sized
large enough.

You can use Versio as part of a pre-merge and post-merge process, too:
`versio check`, `versio diff`, and `versio plan` should all succeed
before merging into a deployment branch, and they will output status
messages that make it easy to track where changes to version numbers
have occurred.
