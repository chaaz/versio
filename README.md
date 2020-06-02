# Versio

Versio (pronounced _vir_-zee-oh) is a simple command-line version
management tool, which reads and writes versions of your projects for
you. It also can scan your conventional commits to calculate the next
best version, build release notes (a.k.a. changelog), and create GitHub
Releases.

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

Most software projects have some sort of manifest. Node projects have a
"package.json" file, Rust has "Cargo.toml", Java projects have their
Maven "pom.xml" and so forth. Within each manifest file is a
value&mdash;usually something in "_&lt;major>.&lt;minor>.&lt;patch>_"
format&mdash;that indicates the current version of the project. (Go
projects don't have a manifested version _per se_, but use Git tags
instead; see below.) The value itself can be somewhat arbitrary, except
that larger numbers usually indicate later versions. Standards like
[semver](https://semver.org/) apply more meaning to the numbers,
correlating kinds of version number increments to the scope of changes.

Naïvely, developers may be expected to update the manifest as they
commit changes to the codebase, to reflect how the software version is
evolving with their changes. However, this doesn't work well in
practice: a version number change usually corresponds to a set of
development changes _in toto_, and not to a specific contribution. Thus,
developers might not be able to intelligently decide when a version
number should be bumped. If multiple developers provide conflicting
version increments, it can be a headache to resolve those conflicts
automatically.

Versio can view and edit version numbers with simple commands, allowing
those values to be kept up-to-date by tooling during the release
process, instead of by individual contributors during development. It
can keep track of multiple projects in a single repository (a so-called
[monorepo](https://en.wikipedia.org/wiki/Monorepo)), and track separate
version numbers and interdependencies for each. If developers use
[conventional commits](https://www.conventionalcommits.org/), Versio can
intelligently aggregate commit information to choose the best new
version of each project during a release.

## How It Works

- Versio reads from a `.versio.yaml` file at the root of your
  repository, and reads the version number of each project referenced
  there.
- It also reads older versions of the same `.versio.yaml` file, starting
  at a `versio-prev` tag from git history, and also reads historic
  version numbers of your projects.
- Based on the old version, new version, and the contents of
  conventional commits, Versio can update a project version number.
- Versio will commit/push the updated manifest files, and push forward
  the `versio-prev` tag.
- Versio can create or update per-project version tags.
- Versio can generate or update a changelog based on the pull requests
  and commits that have been added since the last release.

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

> TODO: After using a PR as part of release, Version can delete its
> associated branch, since it won't need to be used anymore.

## Go Style Projects

Go projects don't have a manifest file, but use Git tags in the style of
`v2.4.15` to track version numbers. Furthermore, many Go projects keep a
separate directory for each major release after v1.

To accommodate these departures from the norm, you can use a "tags"
style manifest for a project:

```yaml
located:
  at: projectName
  tags:
    all: {}
```

> Warning! This style of project requires the `tag_prefix` property to
> be present, which creates/updates git tags like
> `<<tag_prefix>>-v1.2.3` for the project. Since only one project in the
> repository can have a `tag_prefix` of "" (the empty string results in
> Go-standard tags without a prefix like `v1.2.3`), this makes it
> difficult to properly deploy monorepos that contain more than one
> Go-style project.

This allows for standard go-style subdirectories on any branch inside of
the `projectName` folder. You can use `at: .` if your project exists at
the top-level of your repository.

There are more options that let you have a finer control over the
directory layout and branching. See [Go Projects](./docs/gostyle.md) for more
details.

## Running

Check out our [Using Versio](docs/usage.md) page for details on running
Versio, including all command-line options and the format of the
`.versio.yaml` config file.

## Troubleshooting

### Tracking

If you suspect that Versio is not tracking commits, you can have it
stream out all files that it considers with the `versio files` command;
this will output lines of data like this:

```
$ versio files
fix : path/to/file.txt
(...)
```

Each line is a conventional commit type, followed by `:`, followed by a
path to a file which has been altered since the previous tag. This
stream of files forms the basis of the increment plan. You can see the
plan itself using the `versio plan` command, which outputs exactly the
sizes it hopes to apply to each project:

```
$ versio plan
codebase : minor
```

If you have outstanding changes either locally or remote, do a `git
push` and/or `git pull` to make your repo current, and compare the
results of `versio plan` or `versio files` to `git log --stat --oneline
versio-prev..`

You can also view the differences between the previous and current
config files:

```
$ versio diff
New projects:
  secondary : 0.0.4

Unchanged versions:
  codebase : 1.0.1
```

Or show projects from the previous version:

```
$ versio show --prev
codebase : 1.0.1
```

### Rebase

If you rebase your branch, it might cause the last `versio-prev` tag to
no longer be an ancestor of your latest commit. In that case, Versio
might not find the correct commits to update the version.

If you perform such a rebase, you should manually move the `versio-prev`
tag to the corresponding commit on your new branch, with the
command-line `git tag -f versio-prev (new commit sha)`, or something
similar. If your repo has a remote, you should also push this tag with
e.g. `git push --tags --force`, or else it will be reverted when versio
pulls the tag.

### Dry run

Ultimately, you can put this all together with the `run` command, but
pass `--dry-run` in order to suppress actually changing files, or
committing or pushing changes.

```
$ versio run --dry-run
Executing plan:
  codebase : 1.0.1 -> 1.1.0
Dry run: no actual changes.
```

Most of these commands will fetch from the remote first (if it exists)
to ensure that you have the correct `versio-prev` tag and branch data.

> TODO: --no-fetch and fetch conflicts

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
