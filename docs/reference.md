# Versio Reference Doc

## Contents

  - [Authorization]
    - [Git remotes]
    - [GitHub API]
  - [Command-line options]
    - [Global options]
    - [Subcommands]
  - [Common project types]
  - [The config file]
    - [Version config]
    - [File parsing]
    - [Assumed default]

## Authorization
[Authorization]: #authorization

Versio uses authorization options to connect to both Git remote
repositories, and the GitHub API. You may need to provide credentials to
Versio if you need to use these services with authorization. If you
don't want to bother with authorization (and are will to accept reduced
behavior), you can always force Versio to stick to the local repository
with `-l local`; or to not use the GitHub API with  `-l remote`. See
[VCS Levels](./vcs_levels.md) for more information.

See the `CI` sections in [Use Cases](./use_cases.md#ci-authorization)
for info on how to set up authorization for common CI/CD systems.

### Git remotes
[Git remotes]: #git-remotes

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

If you don't have an agent set up, or if your agent is unable to
negotiate credentials, you can set the environment variables
`GITHUB_USER` and `GITHUB_TOKEN` to use more traditional user/password
authorization. Note that the `GITHUB_TOKEN` can be the github password
for this user, or (suggested) an access token generated for this user
and appropriately scoped for versio operations.

### GitHub API
[GitHub API]: #github-api

For GitHub remotes, Versio is capable of scanning through the PRs
associated with commits: see [PR Scanning](./pr_scanning.md). In order
to do this, though, it may need an authorization token. You can generate
a new personal access token for this purpose via the GitHub web UI in
your user's Settings -> Developer settings.

Once you have the new token, you can set the environment variable
`GITHUB_TOKEN` (this can be the same `GITHUB_TOKEN` used for `git`
authorization as well), or you can add it to your user preferences in
`~/.versio/prefs.toml`. Here's an example of such a file:

```
[auth]
github_token = "thisisa40charactertokeniamnotevenjokingg"
```

The environment variable has precedence over the preferences file, but
the file approach may be more convenient for some users.

## Command-line options
[Command-line options]: #command-line-options

Versio, like many comprehensive command-line applications, has a number
of _subcommands_ that dictate what task is actually performed. Most of
these subcommands have their own set of flags and options. There are
also a couple of _global_ flags and options that apply to all commands.

Run Versio like this:

```
$ versio <global options> [subcommand] <subcommand options>
```

### Global options
[Global options]: #global-options

- `vcs-level` (`-l`), `vcs-level-min` (`-m`), `vcs-level-max` (`-x`):
  see the [VCS Levels](./vcs_levels.md) page for a description of these
  global options.

### Subcommands
[Subcommands]: #subcommands

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
  provide the `id` or `name` option. Depending on the VCS level
  (default: "none"), the changed version may be also committed, pushed,
  and/or tagged.
- `diff`: See differences between the current and previous versions.
- `files`: See all files that have changed since the previous version.
- `plan`: View the update plan.
- `release`: Apply the update plan: update version numbers,
  create/update changelogs, and commit/tag/push all changes.
  - `--show-all` (`-a`): Show the run results for all projects, even
    those that weren't updated.
  - `--pause` (`-p <stage>`): Pause the release process before a stage
    of operation. Currently, only the `commit` stage is supported, which
    means that Versio will exit after it writes any local files, but
    before it commits, tags, or pushes to the remote repository. You can
    use this feature to perform additional changes before committing
    your version update. This will create a `.versio-paused` file at the
    top level of your local repository that stores the planned resume
    action: while this file exists, only the `release --resume` or
    `release --abort` commands can be used.
  - `--resume` will perform the planned commits, tags, pushes, etc.
    which were paused from a previous `release --pause`. Any local file
    changes made after the `release --pause` will also be committed. You
    may supply a different VCS Level to this command than the original
    `release --pause` command.
  - `--abort` will simply delete the `.versio-paused` file from a
    previous `release --pause`, discarding any planned commits, tags,
    pushes. This command will *not* rollback any local changes made as
    part of the previous `release --pause`; if needed, you should do
    that yourself with e.g. `git checkout -- .`. You can't use both
    `--resume` and `--abort`.
  - `--dry-run` (`-d`): Don't actually commit, push, tag, or change any
    files, but otherwise run as if you would. `dry-run` is incompatible
    with `--pause`, `--resume`, and `--abort`.
- `init`:
  - `--max-depth` (`-d <depth>`): The maximum directory depth that
    Versio will search for projects. Defaults to `5`.

  Run this command at the base directory of an uninitialized repository.
  It will search the repository for projects, and create a new
  `.versio.yaml` config based on what it finds. It will also append
  `/.versio-paused` to your `.gitignore` file, as a safety measure while
  using the `release --pause` command.

## Common project types
[Common project types]: #common-project-types

`versio init` recognizes the following files as indicative of common
projects, and will create projects in the `.versio.yaml` file as best it
can. The search technique is quite primitive, though. It may not find
the projects you are interested in, or it may set its configuration
incorrectly. You might want to double check the `.versio.yaml` contents
when this command completes.

In some cases, `versio init` will emit a warning that it can't find or
construct a legitimate project. You should definitely manually edit the
`.versio.yaml` in that case--questionable fields will have the value
"EDIT\_ME" when this happens.

Here's a listing of the files that `versio init` searches for:
- `pom.xml` : Maven / Java
- `package.json` : NPM/Node JavaScript
- `go.mod` : Go
- `Cargo.toml` : Cargo / Rust
- `setup.py` : Pip / Python
- `*.gemspec` : Gem / Ruby
- `*.tf` : Terraform
- `Dockerfile` or `.dockerfile` : Docker

## The config file
[The config file]: #the-config-file

A config file named `.versio.yaml` must be located at the base directory
of your repository. Here's an example:

```yaml
options:
  prev_tag: "versio-prev"

projects:
  - name: proj_1
    id: 1
    root: "proj_1"
    tag_prefix: "proj1"
    version:
      file: "package.json"
      json: "version"

  - name: proj_2
    id: 2
    root: "proj_2"
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
  `versio release`. It has a default value of "versio-prev".

- `projects`

  This is a list of projects: you can leave this out if your repo
  doesn't have any projects. Each project has the following properties:

  - `name`: (required) The name of the project.
  - `id`: (required) The numeric ID of the project. By maintaining a
    unique ID for each project, you can track the continuity of a
    project over multiple commits, even if the project name or location
    changes.
  - `root`: (optional, default `"."`) The location, relative to the base
    of the repo, where the project is located. The `changelog`,
    `includes`, `excludes`, and `version: file` properties are all
    listed relative to `root`. Additionally, if `subs` is given, the
    major subdirectories (`v2`, etc) are searched for in root.
  - `includes`, `excludes`: (optional, default includes: `["**/*"]`,
    excludes: `[]`) A list of file glob patterns which specify which
    files are included in/excluded from the project. "*" matches a
    single file, and "**" matches zero or more nested directories. Only
    files covered by `includes` and not by `excludes` are included.
    These patterns are used to determine which commits are applicable to
    a project.
  - `depends`: (optional, default `[]`) A list of projects on which the
    current project depends. Any version number increment in any
    dependancy will result in at least that level of increment in the
    current project.
  - `changelog`: (optional) The file name where the changelog is
    located. Not providing this will cause no changelog to be
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
  commit](https://www.conventionalcommits.org/) type applies to what
  size of increment. Each sub-property is one of the five increment
  sizes: `major`, `minor`, `patch`, `none`, and `fail`; and their values
  are the types that should trigger that size. Here's an example:

  ```yaml
  use_angular: true
  major: [ breaking, incompatible ]
  minor: [ minor, docs ]
  patch: [ "*" ]
  none: [ ignore ]
  fail: [ "-" ]
  ```

  If you include the `use_angular: true` key in your sizes, then the
  following angular conventions will be added to your sizes: `major: [
  "!" ]`, `minor: [ feat ]`, `patch: [ fix ]`, and `none: [ docs,
  style, refactor, perf, test, chore, build ]`. You can override these
  by placing those specific types in different properties (as `docs` is
  done here).

  "!" is a special type which matches a commit whose type ends with "!"
  (as in `refactor!: remove NodeJS 6 support` or `chore(toil)!: delete
  deprecated APIs`), or which contains a footer starting with "BREAKING
  CHANGE:" or "BREAKING-CHANGE:"&mdash;the actual type is ignored in
  this case. "-" is a special type which matches all non-conventional
  commits (commits for which a type can't be parsed). "\*" is a special
  type which matches all commit types that are not matched elsewhere
  (including non-conventional commits if "-" is not listed). If you
  don't provide a "\*" type in your sizes config, versio will exit in
  error as soon as an unmatched commit message is encountered.

  The "none" size indicates that a matched commit shouldn't trigger a
  version increment. The "fail" size indicates that the entire run
  process should fail if a matching type is encountered.

### Version config
[Version config]: #version-config

The "version" property is used both to look up a project's old version,
and to decide where to write the new version. Broadly speaking, there
are two places a project's version can be found: either a structured
manifest file, or some VCS tagging scheme. If the version number is
found in a manifest file, you can list that using something like:

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

```yaml
version:
  file: "weird.json"
  json: ["outer", "0", 1, "the.version"]
```

Versio understands `toml`, `json`, `yaml`, and `xml` formatting, and
`pattern` to use the extended regex pattern for searching through a
file. Or, provide just the "file" property, and Versio will assume
version number makes up the file contents in their entirety (See "File
parsing" below).

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

### File parsing
[File parsing]: #file-parsing

When you specify a file as the version location, you also need to tell
Versio where in the file the version number is. You can use `xml:`,
`json:`, `yaml:`, `toml:`, or `pattern:` types.

- XML: If your version is located in an XML, use this style. The version
  will be found in the text area between the tags matched by the value.
  For example, if your version is stored in a `pom.xml`:

  ```xml
  <project xmlns="http://maven.apache.org/POM/4.0.0" ...>
      <version>0.1.0</version>
  </project>
  ```

  You can configure your project like this:

  ```yaml
  version:
    file: "pom.xml"
    xml: "project.version"
  ```

  Currently, the XML parser can't find a version number inside a CDATA
  block or in XML attributes.

- TOML: Some projects keep the current version in a TOML file. For
  example, Rust projects have a `Cargo.toml` file:

  ```toml
  [package]
  name = "versio"
  version = "0.1.1"
  ```

  ```yaml
  version:
    file: "Cargo.toml"
    toml: "package.version"
  ```

  TOML is a straightforward language, so most things there are
  supported. However, the TOML parser will probably have difficulty with
  triple-quoted string literals.

- YAML: If your project has its version number saved in a YAML file such
  as `project.yaml`, you can access it like this:

  ```yaml
  package:
    version: "0.1.1"
  ```

  ```yaml
  version:
    file: "project.yaml"
    yaml: "package.version"
  ```

  YAML has a lot of different ways to represent data. If the version
  number is stored in a `|` or `>` string literal, or if the value is
  accessed through an alias somehow, Versio might have a hard time
  reading or writing to it.

- JSON: Many project types use JSON to save project metadata. For
  example, NPM projects have a manifest file named "package.json"

  ```json
  {
    "version": "0.1.1"
  }
  ```

  ```yaml
  version:
    file: "package.json"
    json: "version"
  ```

- Regex: If your version number is listed in a file that doesn't match
  one of the common types, you can instead supply a regex pattern: The
  first capturing group of the first match found in the file will be
  used as the version. For example, if you have a file `version.md` that
  has the version number:

  ```markdown
  What's interesting about this file, is that
  the version is _not_ 1.4.2. Instead, it's quite
  clear the version is 50.49.3.
  ```

  ```yaml
  version:
    file: "version.md"
    pattern: '[Tt]he version is (\d+\.\d+\.\d+)\.'
  ```

### Assumed default
[Assumed default]: #assumed-default

If no `.versio.yaml` file is found, the default configuration is
assumed, which looks something like this:

```
options:
  prev_tag: "versio-prev"

sizes:
  use_angular: true
  fail: ["*"]
```
