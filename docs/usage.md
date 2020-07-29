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

```
$ versio show
codebase : 1.0.1
```

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

This is a pretty simple example, but you can imagine how useful it is to
manage multiple projects in the same repo.

## Git Integration

In a `git` repository, `versio` is even more useful. You can assign
"includes" to your projects, and "sizes" to your repo:

```yaml
projects:
  - name: codebase
    id: 1
    includes: ["codebase/**/*"]
    located:
      file: "codebase/package.json"
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

If you include the `use_angular: true` key in your sizes, then the
following angular conventions will be added to your sizes unless you
override them: `minor: [ feat ]`, `patch: [ fix ]`, and `none: [ docs,
style, refactor, perf, test, chore, build ]`.

"-" is a special type which matches all non-conventional commits. "\*"
is a special type which matches all commit types that are not matched
elsewhere (including non-conventional commits, if "-" is not listed
elsewhere). If you don't provide a "\*" type in your sizes config,
versio will exit in error as soon as an unmatched commit message is
encountered.

The "none" size indicates that a matched commit shouldn't trigger a
version increment. The "fail" size indicates that the run process should
plan to fail, rather than increment, if a matching type is encountered.

Versio uses a tag named `versio-prev` to indicate where it should start
scanning for commits. This bounds the work it has to do, and also allows
incremental version changes over time. When you first commit the root
`.versio.yaml` config file, you should tag that commit with the
`versio-prev` tag, and make sure that tag is pushed to the remote.

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
   `versio-prev` tag
1. find the size for each commit
1. increment each project version by the maximum size of commits
   it includes
1. commit those version increments to git
1. re-assign the `versio-prev` tag to that latest commit
1. push both the commit and the tag to the remote

The fetch, merge, and push steps are not performed if the repository
doesn't have any remotes added.

### Plans

> TODO: the description here is unclear, and not entirely accurate.

Plans are built with respect to **previous** projects and includes. That
is, Versio will read the `.versio.yaml` file as it existed in the past
(at the `versio-prev` tag) to determine which projects need to be
incremented, and what version they need to be incremented to. New
projects that were added since then will not be incremented, nor will
projects which have already been incremented by at least the correct
amount.

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
is guaranteed to keep versions up-to-date when it succeeds. Versio will
not interfere with manual version increments that are already sized
large enough.

You can use Versio as part of a pre-merge and post-merge process, too:
`versio check`, `versio diff`, and `versio plan` should all succeed
before merging into a deployment branch, and they will output status
messages that make it easy to track where changes to version numbers
have occurred.
