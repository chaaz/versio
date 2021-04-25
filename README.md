# Versio

Versio (pronounced _vir_-zhee-oh) is a tool that manages your project's
advancement. It intelligently updates version numbers based on
[conventional commits](https://www.conventionalcommits.org/), generates
changelogs, and tags your code.

Versio is especially intelligent when dealing with
[monorepos](https://en.wikipedia.org/wiki/Monorepo), allowing not only
individual control of each project within the repo, but also managing
dependencies and references among them.

## Quick Start

Versio is a self-contained binary written in the Rust programming
language. If you have [installed
Rust](https://www.rust-lang.org/tools/install), you can do this:

```
$ cargo install versio
```

Or, you can download one of the pre-built binaries for your platform
from the [Releases
page](https://github.com/chaaz/versio/releases).

See the [Quick Start](./docs/use_cases.md#quick-start) use case to get
up and running quickly with Versio. Or, try this and see what happens:

```
$ versio init  # this creates .versio.yaml
$ git add .versio.yaml .gitignore
$ git commit -m "build: add versio management"
$ git push
$ versio release
```

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

Versio can help automate the release process by updating
[semver](https://semver.org/) version numbers from [conventional
commits](https://www.conventionalcommits.org/), generating a changelog,
and managing dependencies between projects. This frees developers from
having to coordinate among themselves what versions should be assigned.

Versio can also deliver a machine-readable listing of your projects:
where they live, how they're related, what their tags are--this can be
used to help construct other parts of your release process: such as when
you build, test, publish, and deploy.

## How It Works

Many software projects declare their version number in some sort of
manifest file. Node/NPM projects have a "package.json" file, Rust/Cargo
uses "Cargo.toml", Java/Maven has "pom.xml", Python/pip has "setup.py",
Ruby/gem has gemspec files, and so forth. Go projects and Terraform
modules, among others, opt to keep version numbers in VCS tags instead
of a file. However your project is structured, you can list the location
of your projects' version numbers in a Versio config file, and
thenceforth Versio will be able to manage them.

- Versio reads a config file (by default named `.versio.yaml`) in your
  repository, and finds the version number of each project referenced
  there.
- It also reads previous versions of the same config file and version
  numbers, starting at a specific tag (by default: `versio-prev`) in
  your version control history.
- Based on the old versions, current version, and intervening
  conventional commits, Versio will update your projects' version
  numbers.
- Versio will commit and push the updated manifest files, and update
  `versio-prev` tag.
- Versio can also create or update per-project version tags.
- Versio can generate or update a changelog based on the pull requests
  and commits that have been made since the last release.

## Running

Check out the [Use Cases](./docs/use_cases.md) to learn how to use
Versio via specific use cases that you or your organization might be
interested in, or the [Versio Reference](./docs/reference.md) for all
command-line options and the format of the `.versio.yaml` config file.

## Features

Versio has some nice features that make it easy to use in your projects;
here are just a few.

### Pull Request Scanning

Versio can use the Git API to group commits by PR in its changelog, and
can even "unsquash" PRs to extract the conventional commits hidden
inside a squashed commit. This process happens automatically for
GitHub-originated repositories. See the [PR
Scanning](./docs/pr_scanning.md) page for more information.

### Version Tags

You can write VCS tags, and use them instead of a manifest file; this is
a common pattern in Go and Terraform projects. To use this feature, you
need to provide the project's tag prefix and a default value. See the
[Version Tags](./docs/version_tags.md) document for details.

### Major subdirectories

Some projects keep major revisions of software in different
subdirectories, usually named `v2`, `v3` etc. This allows developers to
keep track of multiple, sometimes very different application structures
at the same time. You can utilize this feature by providing a `subs`
property in your project configuration. See the [Major
Subdirectories](./docs/subs.md) page for a description.

### VCS Levels

VCS Levels allow you to control the way Versio interacts with a Git
repository: you can interact only locally, with a remote, or not at all.
See the description in its [document](./docs/vcs_levels.md) for more
information.

### Version Chains

Sometimes a version in one project will depend on a change in another
project, even when both projects are in the same monorepo. Versio allows
you to manage these dependencies, and automatically increment all
dependent versions. See the [Version Chains](./docs/chains.md) document
for more info.

### VCS Signing

You might like to sign your commits or your tags to provide more
security to your users and co-workers. Versio likes security, too!
Versio can read tags and commits that have been signed, and with the
right configuration, will sign its own commits and tags. See the
[Signing](./docs/signing.md) page for how to do this.

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
