# Versio Reference Doc

## Authorization

Versio uses authorization options to connect to both Git remote
repositories, and the GitHub API. You may need to provide credentials to
Versio if you need to use these services with authorization.

### Git remotes

Versio will attempt to use an underlying credentials helper agent in
order to provide the correct SSH key to GitHub remote servers.
Instructions to set this up are outside the scope of this document, but
once you set this up, you should be able to see something like this
(example output on macOS):

```
$ git config credential.helper
osxkeychain
```

You'll need authorization to push to and pull from the remote if you
expect Versio to keep your remote in sync. Make sure that commands like
e.g. `git fetch` work from the command-line if versio is having trouble.

### GitHub API

For GitHub remotes, Versio is capable of scanning through the PRs
associated with commits: see [PR Scanning](./pr_scanning.md). In order
to do this, though, it may need an authorization token. You can generate
a new personal access token for this purpose via the GitHub web UI in
your user's Settings -> Developer settings.

Once you have the new token, you can add it to your user configuration
in `~/.versio.rc.toml`. Here's an example of such a file:

```
[auth]
github_token = "thisisa40charactertokeniamnotevenjokingg"
```

## Command-line options

Versio, like many comprehensive command-line applications, has a number
of _subcommands_ that dictate what task is actually performed. Most of
these subcommands have their own set of flags and options. There are
also a couple of _global_ flags and options that apply to all commands.

Run Versio like this:

```
$ versio <global options> [subcommand] <subcommand options>
```

### Global options

- `vcs-level` (`-l`), `vcs-level-min` (`-m`), `vcs-level-max` (`-x`):
  see the [VCS Levels](./vcs_levels.md) page for a description of these
  global options.

### Subcommands

The following is a complete list of subcommands available in Versio,
along with their options and flags. You can always use `versio help` or
`versio help <subcommand>` to get the latest list.

- `check`: Run this command to ensure that your config file and
  repository is properly configured.
- `show`: Show all projects in your monorepo, along with their current
  versions.
  - `--prev` (`-p`): Show the previous versions instead, created by the
    last run of `versio run`. This will differ from the current version
    if you have added/removed projects, or manually made version number
    changes since the last time Versio ran.
  - `--wide` (`-w`): Output a wide format that includes the project ID.
- `get`: Show one or more projects' version numbers.
  - `--id` (`-i <ID>`): Show only the project that matches the given ID.
  - `--version-only` (`-v`): Output only the version number(s)
  - `--name` (`-n <name>`): Show only the project(s) whose name at least
    partially matches. Mutually exclusive with `id`.
  - `--prev` (`-p`): Show the previous versions instead, created by the
    last run of `versio run`. This will differ from the current version
    if you have added/removed projects, or manually made version number
    changes since the last time Versio ran.
  - `--wide` (`-w`): Output a wide format that includes the project ID.

  If you only have a single project configured, you don't need to
  provide the `id` or `name` option.
- `set`: Change one project's version number.
  - `--id` (`-i <ID>`): Change the project that matches the given ID.
  - `--name` (`-n <name>`): Change the project that matches the given
    name.
  - `--value` (`-v <value>`): The new version value

  If you only have a single project configured, you don't need to
  provide the `id` or `name` option. Depending on the VCS level, the
  changed version may be also committed, pushed, and/or tagged.
- `diff`: See differences between the current and previous versions.
- `files`: See all files that have changed since the previous version.
- `plan`: View the update plan.
- `log`: Create/update all project changelogs.
- `run`: Apply the update plan: update version numbers, create/update
  changelogs, commit/tag/push all changes, and publish new builds.
  - `--dry-run` (`-d`): Don't actually commit, push, tag, change any
    files, or publish anything, but otherwise run as if you would.
  - `--show-all` (`-a`): Show the run results for all projects, even
    those that weren't updated.

## The config file

A config file named `.versio.yaml` must be located at the base directory
of your repository. Here's an example:

```yaml
options:
  prev_tag: "versio-prev"

projects:
  - name: proj_1
    id: 1
    root: "proj_1"
    includes: ["**/*"]
    tag_prefix: "proj1"
    version:
      file: "package.json"
      json: "version"

  - name: proj_2
    id: 2
    root: "proj_2"
    includes: ["**/*"]
    tag_prefix: ""
    version:
      tags:
        default: "0.0.0"
    subs: {}

sizes:
  use_angular: true
  fail: ["*"]
```

- `options`

  These are general project options. Currently the only option is
  `prev_tag`, which specifies the tag used to locate the latest run of
  `versio run`. It has a default value of "versio-prev".

- `projects`

  This is a list of projects: you can leave this out if your repo
  doesn't have any projects. Each project has the following properties:

  - `name`: (required) The name of the project.
  - `id`: (required) The numeric ID of the project. By maintaining a
    unique ID for each project, you can track the continuity of a
    project over multiple commits, even if the project name or location
    changes.
  - `root`: (optional, default `"."`) The location, relative to the base
    of the repo, where the project is located. The `change_log`,
    `includes`, `excludes`, and `version: file` properties are all
    listed relative to `root`. Additionally, if `subs` is given, the
    major subdirectories (`v2`, etc) are searched for in root.
  - `includes`: (optional, default `[]`) A list of file glob patterns
    which specify which files are included in the project.
  - `excludes`: (optional, default `[]`) A list of patterns which
    specify which files are excluded from the project. Only files
    covered by `includes` and not by `excludes` are included. These
    patterns are used to determine which commits are applicable to a
    project.
  - `depends`: (optional, default `[]`) A list of projects on which the
    current project depends. Any version number increment in any
    dependancy will result in at least that level of increment in the
    current project.
  - `change_log`: (optional) The file name where the changelog is
    located. Not providing this will cause no change log to be
    created/updated.
  - `version`: The location of the project version. See "Version config"
    below.
  - `tag_prefix`: (optional) (required when using version tags) The
    prefix to use when reading/writing tags for this project. Not
    providing this will result in no tags being written. Using the empty
    string "" will use tags with no prefix. Each project's tag prefix,
    if any, must be unique.
  - `subs`: If provided, allows a project to be subdivided into "major"
    versions, each in its own subdirectory. See [Major
    Subdirectories](./subs.md) for more info on this feature.

- `sizes`

  This is a mapping of what [conventional
  commit](https://www.conventionalcommits.org/) label applies to what
  size of increment. Each sub-property is one of the four increment
  sizes: `major`, `minor`, `patch`, and `fail`; and their values are the
  labels that should trigger that size. Here's an example:

  ```yaml
    use_angular: true
    major: [ breaking, incompatible ]
    minor: [ minor, docs ]
    patch: [ "*" ]
    none: [ ignore ]
    fail: [ "-" ]
  ```

  If you include the `use_angular: true` key in your sizes, then the
  following angular conventions will be added to your sizes: `minor: [
  feat ]`, `patch: [ fix ]`, and `none: [ docs, style, refactor, perf,
  test, chore, build ]`. You can override these by placing those
  specific labels in different properties (as `docs` is done here).

  "-" is a special type which matches all non-conventional commits
  (commits for which a label can't be parsed). "\*" is a special type
  which matches all commit types that are not matched elsewhere
  (including non-conventional commits if "-" is not listed). If you
  don't provide a "\*" type in your sizes config, versio will exit in
  error as soon as an unmatched commit message is encountered.

  The "none" size indicates that a matched commit shouldn't trigger a
  version increment. The "fail" size indicates that the entire run
  process should fail if a matching type is encountered.

### Version config

Broadly speaking, there are two places a project's version can be found:
either a structured manifest file, or some VCS tagging scheme. If the
version number is found in a manifest file, you can list that using
something like:

```yaml
version:
  file: "Cargo.toml"
  toml: "package.version"
```

The "package.version" above indicates the structured location within the
file where the version number is located. Here, it's listed as the usual
location within a `Cargo.toml` file:

```toml
[package]
version = "0.1.0"
```

The structured location can be fairly complex: here you can specify that
the version is located in a deeply nested location (which has the value
"1.2.3":

```json
{
  "version": {
    "thing": [
      "2.4.6",
      { "version": "1.2.3" }
    ]
  }
}
```

```yaml
version:
  file: "custom.json"
  json: "version.thing.1.version"
```

Or, you can be extremely specific for weird edge cases:

```json
{
  "outer": {
    "0": [
      { "not.the.version": "2.4.6" },
      { "the.version": "1.2.3" }
    ]
  }
}
```

```
version:
  file: "weird.json"
  json: ["outer", "0", 1, "the.version"]
```

Versio understands `toml`, `json`, `yaml`, and `xml` formatting. Or you
can provide a Perl extended regex pattern: the first captured value of
the first capturing group will be considered to match the version
number.

```yaml
version:
  file: "version.txt"
  pattern: '[Tt]he version is now (\d+\.\d+\.\d+)\.'
```

If you provide just a `file:` (with no structural property), then it's
assumed the version makes up the file contents in their entirety.

If you are using VCS tagging to track your project version number (which
is common in Go and Terraform projects), then you can use something like
this instead:

```yaml
tag_prefix: "projname"
version:
  tags:
    default: "0.0.0"
```

See [Version Tags](./version_tags.md) for more info on the benefits and
pitfalls of this technique.

### Assumed default

If no `.versio.yaml` file is found, the default configuration is
assumed, which looks something like this:

```
options:
  prev_tag: "versio-prev"

sizes:
  use_angular: true
  fail: ["*"]
```
