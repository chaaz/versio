# Versio

Versio (pronounced _vir_-zee-oh) is a simple command-line version
management tool, which reads, calculates, and updates versions for you.
It's a bit similar to the versioning capabilities of `semantic-release`,
but with the ability to track multiple projects in a monorepo.

## Quick Start

- Install versio:
  ```
  $ cargo install versio  # or download a pre-built binary to your PATH
  ```
- Take a look at your project:
  ```
  $ cd $(git rev-parse --show-toplevel)

  $ cat package.json
  {
    "version": "1.0.1",
    ...
  }
  ```
- Create a simple config file:
  ```
  $ cat > .versio.yaml << END_OF_CFG
  projects:
    - name: my-project
      id: 1
      covers: ["*"]
      located:
        file: "package.json"
        json: "version"

  sizes:
    use_angular: true
    fail: [ "*" ]
  END_OF_CFG
  ```
- Commit and tag your config file
  ```
  $ versio check
  $ git pull
  $ git add .versio.yaml
  $ git commit -m "build: add versio management"
  $ git push
  $ git tag -f versio-prev
  $ git push -f origin versio-prev
  ```
- Look at your current version:
  ```
  $ versio show
  my-project : 1.0.1
  ```
- Change it (and change it back):
  ```
  $ versio set --id 1 --value 1.2.3

  $ cat package.json
  {
    "version": "1.2.3",
    ...
  }

  $ versio set --id 1 --value 1.0.1
  ```
- After a few [conventional
  commits](https://www.conventionalcommits.org/en/v1.0.0/), update it:
  ```
  $ versio run
  Executing plan:
    my-project : 1.0.1 -> 1.1.0
  Changes committed and pushed.
  ```

## Background

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

Versio can manage those version numbers, if you tell it where they are.

## How It Works

- Versio reads from a `.versio.yaml` file at the root of your
  repository, and reads the version number of each file references
  there.
- It also reads older versions of the same `.versio.yaml` file from git
  history, and also reads historic version numbers of your projects.
- Based on the old version, new version, and the contents of
  conventional commits, Versio can update version numbers to appropriate
  new values.

## Troubleshooting

If you rebase your branch, it might cause the last `versio-prev` tag to
no longer be an ancestor of your latest commit. In that case, Versio
will be unable to find any commits, so will not properly increment
versions.

If you perform such a rebase, you should manually move the `versio-prev`
tag to the corresponding commit on your new branch, with the
command-line `git tag -f versio-prev (new commit sha)`, or something
similar. If your repo has a remote, you should also push this tag with
e.g. `git push --tags --force`, or else it will be reverted when versio
pulls the tag.

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
This will fail if your current working directory is not "clean": that
is, if you're in the middle of a manual merge, conflict resolution,
rebase, etc. You can skip the fetch by supplying the `--no-fetch` (`-F`)
flag.

`--no-fetch` will only work on the `run` command if `--dry-run` is also
supplied. On a real run to avoid conflicts, fetching is always enforced.
Additionally, a real run won't proceed if the repository isn't
"current": that is, there are no uncommitted, modified, or untracked
files in the working directory. If you have such changes, you must
commit or stash them before `versio run`.

Finally, you can use the `--all` (`-A`) flag to `run`, which will also
generate a descriptive "no change" message for projects that aren't
automatically incremented.

## Contributing

We would love your code contributions to Versio! Feel free to branch or
fork this repository and submit a pull request.

`versio` is written in Rust, a powerful and safe language for writing
native executables. Visit the Rust lang
[homepage](https://www.rust-lang.org/en-US/index.html) to learn more
about writing and compiling Rust programs, and see the
[Contributing](docs/contributing.md) page for Versio specifically.

We also happily accept ideas, suggestions, documentation, tutorials, and
any and all feedback, positive or negative. Leave a message on the
support pages of this repo, or send messages directly to its owners.
