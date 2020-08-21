# Versio

Versio (pronounced _vir_-zee-oh) is a simple tool to manage the
progression of a project. It intelligently reads and writes project
versions, updates version numbers based on conventional commits,
generates changelogs, and publishes the project to a variety of
distribution targets.

Versio is especially intelligent when dealing with monorepos, allowing
not only individual control of each project within the repo, but also
managing dependencies and references among them.

## Quick Start

Versio is a self-contained binary written in Rust. If you have
[installed Rust](https://www.rust-lang.org/tools/install), you can do
this:

```
$ cargo install versio
```

Or, you can download one of the pre-built binaries for your platform,
making sure that it's in your PATH.

See the [Quick Start](./docs/use_cases.md#quick-start) use case to get
up and running quickly with Versio, once you have it installed.

## Background

<!---
A developer of a project&mdash;after making some changes to a
project&mdash;might "release" her work: she will update the version
number, write a short log explaining her changes, and then publish the
new software to make it widely available. However, this
code-then-release process quickly becomes difficult to maintain.

In projects of even modest complexity, a software release usually
corresponds to a set of development changes _in toto_, and not to a
specific contribution from a single person. In larger communities,
individual contributors might not even decide when a release should
occur. If multiple developers provide conflicting version increments, it
can be a headache to resolve. And it can be inefficient to halt all
contributions while a release is being built.
-->

There have been many tools and strategies surrounding the  *release
process* in software: the series of steps by which a set of changes to a
software product is described, assigned a unique version number, and
then made available to a wider audience. Versio is one such tool: it can
use information found in [conventional
commits](https://www.conventionalcommits.org/) to update version
numbers, generate a changelog, and publish the software to standard
distribution targets. It is especially adept at handling multiple
projects in a single repository (a so-called
[monorepo](https://en.wikipedia.org/wiki/Monorepo)), tracking separate
version numbers and interdependencies for each.

Software projects keep their version number somewhere using a manifest
file or tagging scheme. Node.js projects (that use `npm` to manage) have
a "version" property in their "package.json" file, Rust (`Cargo`) uses
"Cargo.toml", Java (`Maven`) has "pom.xml", Python (`pip`) has
"setup.py", Ruby (`gem`) has gemspec files and so forth; Go projects do
something similar, but use VCS tags instead of a manifest file. To use
Versio, you have to create a config file that lists the location of each
project in your repo, along with the location of the project's version
number.

> TODO: future versions of Versio will be able to automatically detect
> existing versions

Here's a very simple example project that covers the entirety of
the repository.

```
- name: project
  id: 1
  includes: ["**/*"]
  located:
    file: "package.json"
    json: "version"
```

## How It Works

- Versio reads a config file (default: `.versio.yaml`) in your
  repository, and finds the version number of each project referenced
  there.
- It also reads previous versions of the same config file and version
  numbers, starting at a specific tag (default: `versio-prev`) in your
  change control history.
- Based on the old versions, new version, and the contents of
  intervening conventional commits, Versio will update your projects'
  version numbers.
- Versio will commit and push the updated manifest files, and push
  forward the `versio-prev` tag.
- Versio can also create or update per-project version tags.
- Versio can generate or update a changelog based on the pull requests
  and commits that have been added since the last release.
- Finally, Versio can publish each project to its most appropriate
  distribution targets.

## PR Scanning

While using commits in Git is helpful to determine the general size and
complexity of a release, they don't always tell the whole story. Lots of
minor or trivial commits are often collected in a single pull request
(PR) to implement a story-level feature. Additionally, sometimes PRs are
"squashed" onto a release branch, generating a single commit that elides
the per-project size information inherent in the PR.

If your repository uses GitHub as its remote, then Versio will use the
GitHub v4 GraphQL API to extract more information about the PRs and
associated commits that went into the release changes. If Versio creates
or updates a changelog, it will group commits into whatever PRs can be
found.

If a PR has been squashed onto the branch, Versio will "unsquash" that
PR for changelog and increment sizing purposes. Unsquashing is only
possible if the PR's commits still exist on the Git remote: if the
branch has been deleted (which is typical for squashes), then the
commits may have been garbage collected, and unavailable for
examination. In this case, Versio will make some guesses, but might get
some sizing or grouping wrong. If unsquashing is important, don't delete
PR branches from GitHub until after they've been part of a release.

> TODO: After using a PR as part of release, Versio can delete its
> associated branch, since it won't need to be used anymore.

## Go-style Projects

Go projects don't have a manifest file, but use Git (or other VCS) tags
in the form `v1.2.3` to track version numbers. Furthermore, many Go
projects keep a separate directory for each major release after v1.
Versio supports both of these idioms.

### Version Tags

You can use Git or other VCS tags to record the version of a project
instead of writing it in a manifest file. To do so, simply use `tags` as
your `located` for the project:

```yaml
tag_prefix: "projname"
located:
  tags:
    default: "0.0.0"
```

The `tag_prefix` property causes Versio to write out a new
"projname-v*x.y.z*" tag for the project when the version number is
changed. The property is optional for most projects, but required for
projects that use `located: tags`. The default value is used when no
existing "projname-v*x.y.z*" tags currently exist.

> Since `tag_prefix` is used to find older tags of a project, you should
> not change this property value over the life of your project. If you
> change the `tag_prefix`, you may need to manually re-tag your commit
> history, or else Versio may be unable to locate past version numbers.

> If a project uses `located: tags:`, you may want to use the
> `--vcs-level=max` option while running the `versio set` command for
> that project. This command normally runs at the `none` VCS level,
> because it typically used to just update a file. However, for
> tags-based projects, you may want to instead update the VCS tags on
> the local machine and/or the remote, which requires a higher VCS
> level. See [VCS Levels](./docs/vcs_levels.md) and [Usage:
> Set](./docs/usage.md#setting) for more info.

> If the tag prefix is *empty* (`tag_prefix: ""`), then tags for the
> project take a non-prefixed form "v*a.b.c*", which is combatible with
> most Go tools. Especially `go get` and `go mod`, which search for
> version tags in that form. If you do use a prefix, you'll need to
> reference your project with the fully-qualified tag: e.g. `go get
> server.io/path/to/proj@projname-v1.2.3`. Failure to use a tag will
> probably just get you the latest commit. If you need to also use a
> major subdirectory (see below), you'll need to use a full path like
> `server.io/path/to/proj/v3@projname-v3.2.1`.
>
> This problem is compounded in a monorepo with two or more Go projects:
> only one of those projects can have an empty prefix, because prefixes
> must be unique. Also, tags in most VCS apply to an entire repo, and
> not just a single project. Be very careful referencing your projects
> with Go tools in this situation: it's usually best in that case to be
> explicit and reference all your projects with tags.

There are other options available to fine-tune control of version tags:
see [Go Projects](./docs/gostyle.md) for more info.

### Subdirectories

It's common in Go projects to keep the major versions 0 and 1 code in a
top-level directory, but to put later versions inside their own
sub-directories, such as `v2`, `v3`, etc.

You can do this in Versio by providing a `subs` property:

```
root: `my_proj_dir`
subs: {}
```

`root` is a useful property that specifies a relative base directory for
the `changelog`, `located.file` and `includes`/`excludes` of a project.
If you don't provide it, it is assumed to be `"."` (the repository
root).

By default, `subs` creates a "subproject" for each directory it detects
in a name like "v*N*". The root directory for the sub is extended by
that directory, but other properties are (more or less) copied over
unaltered. Subprojects are prohibited from having version numbers that
that don't match up with their directory name.

There are other options available to fine-tune control of subprojects:
see [Go Projects](./docs/gostyle.md) for more info.

## Running

Check out the [Using Versio](docs/usage.md) page for details on running
Versio, including all command-line options and the format of the
`.versio.yaml` config file. [Use Cases](./docs/use_cases.md) lists
specific use cases that might meet a need in your project or
organization. The [Publishing](./docs/publishing.md) page shows
specifically how Versio can publish your software. Finally, the [Go
Projects](./docs/gostyle.md) doc talks about how to use Versio for the
unique versioning approach of "Go"-style projects.

## Troubleshooting

There's a whole [Troubleshooting](./docs/troubleshooting.md) document
for tracking down and reporting errors or other unexpected behavior. A
lot of the time, though, it comes down to running Versio with logging
and error tracing activated:

```
RUST_LOG=versio=trace RUST_BACKTRACE=1 versio <command>
```

## Contributing

We would love your code contributions to Versio! Feel free to branch or
fork this repository and submit a pull request.

`versio` is written in Rust, a powerful and safe language for writing
native executables. Visit the Rust lang
[homepage](https://www.rust-lang.org/en-US/index.html) to learn more
about writing and compiling Rust programs, and see the
[Contributing](docs/contributing.md) page for Versio specifically.

We also happily accept ideas, suggestions, documentation, tutorials, and
any and all feedback. Leave a message on the support pages of this repo,
or send messages directly to its owners.
