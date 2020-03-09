# Versio

Versio (pronounced _vir_-zee-oh) is a simple command-line version
management tool, which reads, calculates, and updates versions for you.
It's a bit similar to the versioning capabilities of `semantic-release`,
but with the ability to track multiple projects in a monorepo.

## Summary

Everyone likes version numbers! Version numbers let you keep track of
all the iterations of a project, and communicate how similar one is to
another. If you use [semver](https://semver.org/), then this
communication is formal, and users of a project can rely on changes (or
lack thereof) from one version to the next.

A version number is usually written into some sort of standard project
or package file. For example, Node.js projects use a `package.json` file
with a top-level "version" property. Rust projects (like this one!) have
a `Cargo.toml` with a `package.version` key. Other project types have
their own conventions.

Versio can track those version numbers, if you tell it where they are.

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

And then you had a `.versio.yaml` at the root of your repository:

```yaml
projects:
  - name: codebase
    id: 1
    located:
      file: "codebase/package.json"
      json: "version"

```

Versio has handlers for JSON, YAML, and TOML files, as well as plain
files which contain only a version number, or any file which can be
scanned for a version number via a regular expression.

With the config file in place, you can now _show_ versions of all your
projects:

```bash
$ versio show
codebase : 1.0.1
```

You can also _set_ a version to a new value:

```bash
$ versio set --id 1 --value 1.2.3
```

Which would modify your `codebase/package.json` file:

```json
{
  "version": "1.2.3",
  "... other properties": "..."
}
```

This is a pretty simple example, but you can imagine how useful it is to
manage multiple projects in the same repo.

## Git Integration

In a `git` repository, `versio` is even more useful. You can assign
"covers" to your projects, and "sizes" to your repo:

```yaml
projects:
  - name: codebase
    id: 1
    covers: ["codebase/*"]
    located:
      file: "codebase/package.json"
      json: "version"

sizes:
  major: [ break, "-" ]
  minor: [ feat ]
  patch: [ fix ]
  none: [ none ]
```

Now, Versio can scan your commits to determine how it should increment
each project version.

A project is said to _cover_ a git commit if that commit has a file
change matching any glob pattern of the project's "covers" key. The
_size_ of a commit is determined by the type of its [conventional
commit](https://www.conventionalcommits.org/en/v1.0.0/) message, as it
maps to the "sizes" property in the Versio config.

Versio uses a tag named `versio-prev` to indicate where it should start
scanning for commits. This bounds the work it has to do, and also allows
incremental version changes over time. When you first commit the root
`.versio.yaml` config file, you should tag that commit with the
`versio-prev` tag, and make sure that tag is pushed to the remote.
(TODO: `versio init` ?)

For example, let's say you're using the above config, and have a single
commit since the `versio-prev` tag:

```
b3ed0f0 feat: Change an important file
 codebase/src/misc.js | 6 ++++++ 
 1 file changed, 7 insertions(+)
```

Notice that the commit message starts with "feat": "feat" maps to
"minor" in our config. And it contains a file `codebase/src/misc.js`
which is covered by the `codebase/*` glob in our single project.

We can run the Versio git integration like this:

```bash
$ versio run
Executing plan:
  codebase : 1.0.1 -> 1.1.0
Changes committed and pushed.
```

The `run` command will:

1. fetch the latest branches from the "origin" remote
1. merge the current branch from the remote
1. scan through the git log for conventional commits since the 
   `versio-prev` tag
1. find the size for each commit
1. increment each project version by the maximum size of commits
   it covers
1. commit those version increments to git
1. re-assign the `versio-prev` tag to that latest commit
1. push both the commit and the tag to the remote

It's important to note that plans are built with respect to **previous**
projects and covers. That is, Versio will read the `.versio.yaml` file
as it existed in the past (at the `versio-prev` tag) to determine which
projects need to be incremented, and what version they need to be
incremented to. New projects that were added since then will not be
incremented, nor will projects which have already been incremented by at
least the correct amount.

Of course, the `.versio.yaml` file might itself have gone through
several iterations since `versio-prev`, corresponding to changes in the
repo structure. Future releases of Versio will follow the evolution of
`.versio.yaml`, making sure that project changes are identified
correctly. (TODO: do this.)

On the other hand, Versio will use the **current** version of
`.versio.yaml` to determine what size a commit has, and will only
attempt to increment projects that also exist in the current config.
This is why each project needs a unique and unchanging ID: it makes it
possible to change the name, coverage, etc of a project, but still
correlate its present incarnation with its past self.

## CI/CD Integration

One useful application of Versio is in a CI/CD pipeline: you can call
`versio run` after a merge to the release branch. Since Versio will only
run on a clean and current branch, and will fail on any commit error, it
is guaranteed to keep version up-to-date when it succeeds. Additionally,
Versio will not interfere with manual version increments, as long as
they're sized large enough.

## Troubleshooting:

If you rebase your branch, it might cause the last `versio-prev` tag to
no longer be an ancestor of your latest commit. In that case, Versio
will be unable to find any commits, so will not properly increment
versions.

If you perform such a rebase, you should manually move the `versio-prev`
tag to the corresponding commit on your new branch, with the
command-line `git tag -f versio-prev (new commit sha)`, or something
similar.

If you suspect that Versio is not tracking commits, you can have it
stream out all files that it considers with the `versio files` command;
this will output lines of data like this:

```
$ versio files
fix : path/to/file.txt
(...)
```

Each line is a conventional commit type, followed by `:`, followed by a
path to a file. This stream of files forms the basis of the increment
plan. You can see the plan itself using the `versio plan` command, which
outputs exactly the sizes it hopes to apply to each project:

```
$ versio plan
codebase : minor
```

You can also view the differences between the previous and current
config files:

```
$ versio diff
New projects:
  secondary : 0.0.4

Unchanged versions:
  codebase : 1.0.1
```

Or just get or show projects from the previous version:

```
$ versio show --prev
codebase : 1.0.1
```

Ultimately, you can put this all together with the `run` command, but
pass `--dry-run` in order to suppress actually changing files, or
committing or pushing changes.

```
$ versio run --dry-run
Executing plan:
  codebase : 1.0.1 -> 1.1.0
Dry run: no actual changes.
```

Most of these commands will fetch from the remote first to ensure that
you have the correct `versio-prev` and branch data. This will fail if
your current working directory is not "clean": that is, if you're in the
middle of a manual merge, conflict resolution, rebase, etc. You can skip
the fetch by supplying the `--no-fetch` (`-F`) flag.

`--no-fetch` will only work on the `run` command if `--dry-run` is also
supplied. On a real run to avoid conflicts, fetching is always enforced.
Additionally, a real run check that the repository is "current": that
is, there are no uncommitted, modified, or untracked files in the
working directory, and will halt if that check fails.
