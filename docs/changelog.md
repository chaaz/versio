# Changelog Management

Changelogs are a useful artifact of deployment: they are a
human-readable record that lists what changes have occurred to a program
on each release. Versio can automatically maintain a changelog for each
project by extracting information from the PRs and conventional commits
that contributed to each release.

During the release process, Versio will update a project's changelog
using the default [Liquid](https://shopify.github.io/liquid/) html
template and the details of the release plan. You can activate this
behavior by providing a `changelog` property for your projects in your
`.versio.yaml`:

```yaml
projects:
  - name: "myproject"
    changelog: "dev_docs/CHANGELOG.html"
```

If you don't like the default template, you can pick your own:

```yaml
projects:
  - name: "myproject"
    changelog:
      file: "dev_docs/CHANGELOG.html"
      template: "file:dev_docs/CHANGELOG.html.liquid"
```

See the "Template URLs" section below to find out what templates you can
use in this property.

## Other Commands

In addition to writing to a changelog at release, Versio has a few more
commands to generate and examine your records and templates.

### Plan formatting

`versio plan --template=<template URL>`: While `plan` normally outputs a
simple text description of version changes, using the `template` flag
will produce output that is instead formatted according to a provided
changelog template. It is especially useful here to use
`--template=builtin:json`, which will output a JSON document that
describes all the PRs, commits, etc in a machine-readable format. You
can later use that document in your own changelog process, if you want
to go beyond Versio's capabilities.

When using this style of the `plan` command, if you have more than one
project, you must provide a project ID (`--id=<project ID>`); Versio
doesn't know how to stitch changelogs from multiple projects into a
single document.

Using `versio plan --template=...` will generate a document without
considering existing changelog contents. If your template uses the
`old_content` property (see below), then it will always be resolved to
an empty string when using this command.

### Changelog previews

The Versio `release` command has a `--changelog-only` flag, which is
similar to the `--dry-run` flag. Where `--dry-run` suppresses all output
and VCS actions, `--changelog-only` suppresses all output and VCS
actions **except** writing to the changelog(s). (`--dry-run` and
`--changelog-only` cannot be used together.) This lets you generate a
"preview" changelog before a release happens, which may be useful for
some workflows.

Just like `--dry-run`, `--changelog-only` will not commit any changes,
push to a remote repo, change any files (other than the changelogs), or
tag any releases. Furthermore, the new changelogs count as local
modifications, so later Versio commands in that workspace may fail. If
you intend to perform further release actions in the same workspace,
first finish using the preview changelogs (or copy them somewhere they
can be used), then revert them with `git restore` or (for older versions
of Git) `git checkout`. You should probably not commit the previews,
since that could risk double-writing release information.

Be aware that previews may not exactly match the later release
changelog: the release process may find different commits, PRs, access
permissions to Git or GitHub, or other environmental differences that
may create a differing release plan. There may be commit or PR ordering
differences, depending on recorded commit times. Also, if the changelog
template uses the release date, clock time, or another non-fixed value,
those might be different during the actual release.

### Show template

`versio template --template=<template URL>`: This will output the
verbatim content of the given changelog template. This is especially
useful if you want to peruse the builtin templates, or save them as a
starting point to create your own templates.

> Currently, this command only works for `builtin` template types.

## Template URLs

When providing a specific template, you must give a full URL in the form
`protocol:details`. The template system accepts three different
protocols:

- The `builtin` protocol can be `builtin:html` or `builtin:json`, which
  uses templates provided internally by Versio. If no template URL is
  provided, then `builtin:html` is assumed.

- The `file` protocol will accept a relative path to a file. If you're
  providing the file name in the `.versio.yaml` configuration file, then
  the file name is assumed relative to the project root, and must be in
  forward-slash format (`"path/to/file"`). If provided on the
  command-line, then the file name is relative to the current working
  directory, and should be given in native format (forward-slash in
  Unix-based systems, backward-slashes on Windows). An example might be
  `file:internal/CHANGELOG.html.tmpl`

- The `http` and `https` protocols allow you to pull _remote templates_
  using HTTP GET. A full HTTP URL is allowed here; including scheme,
  user info, query, and fragment. For example:
  `http://ci.myco.com/releases/templates/CHANGELOG.html.liquid-tmpl`.

  Remote templates is a powerful feature, allowing you or your
  organization to manage a consistent document style across multiple
  repos. However, keep in mind these caveats:
    - The entire document is used as the template, even if the URL
      contains a fragment section.
    - No client-side authentication is performed.
    - Missing or invalid certificates on HTTPS are rejected.
    - Remote templates may impact the performance of a release,
      depending on the speed of the HTTP response.

## Template variables

Like the default template, the provided template must be in Liquid
format. The following variables are available to the template:

- `project`: a structure that contains information about the current
  project:
    - `id`: The ID of the project.
    - `name`: The name of the project.
    - `tag_prefix`: The tag-prefix of the project (if it exists).
    - `tag_prefix_separator`: The tag-prefix separator of the project
      (defaults to "-")
    - `version`: The current version of the project (which should match
      `release.version`).
    - `full_version`: The full version name of the project. The version
      number is preceded by the letter `v`. If there is a `tag_prefix`,
      it is prepended and separated from the version number with
      `tag_prefix_separator`.
    - `root`: The directory root of the project, (relative to the
      repository root)
- `release`: this is a structure that contains details of the current
  release:
    - `date`: The current date, in Y-M-D format.
    - `prs`: A list of PRs that are included in this release. This is an
      array of structures. The last element of the array will be an
      "Other commits" psuedo-PR that contains all commits in the release
      that don't fall into any of the previous PRs:
        - `title`: The human-readable title of the PR.
        - `name`: The name of the PR, something like "PR 23" or "Other
          commits"
        - `size`: The size of the PR as it applies to the project.
          "major", "minor", etc.
        - `href`: A URL to the PR, if any.
        - `link`: True if and only if the PR has a valid href.
        - `commits`: A list of commits in this PR, as an array of
          structures:
            - `href`: the URL of the commit, if any.
            - `link`: True if and only if the PR has a valid href.
            - `shorthash`: The 7-digit has of the commit.
            - `size`: The size of the commit as it applies to the
              project. "major", "minor", etc.
            - `summary`: A short summary of the commit
            - `message`: The complete commit message.
    - `deps`: Dependencies on other projects that caused the current
      project to be released. This is a list of simple structures:
        - `id`: The ID of the depended-on project.
        - `name`: The name of the depended-on project.
    - `version`: The version number of the release.
- `old_content`: The previous content found in an existing CHANGELOG,
  between the begin- and end-content flags.

### Old content

The `old_content` variable, if used in a liquid template, is set to the
property of the old changelog file. By using this, you can let a
changelog to grow over time, adding new entries to existing data every
time a release is performed.

The entire content of the old changelog is not provided: instead, only
lines between the first line that contains the start marker, and the
first following line that contains the end marker is included. You can
use this to good effect to create a growing changelog: for example, a
simple HTML template might look something like:

```html
<html>
<body>
<!-- header stuff -->

<!-- ### VERSIO BEGIN CONTENT ### -->
<!-- ENTRY: {{release.date | date: "%Y-%m-%d" }} -->
{% for pr in release.prs %}
<span>{{pr.title}}</span>
{% endfor %}
{{old_content}}
<!-- ### VERSIO END CONTENT ### -->

<!-- footer stuff -->
</body>
</html>
```

When a changelog is created using the above template, it will start with
a single `<span>` in the body for each PR. Notice the placement of the
`old_content` variable: this will be set to the empty string if there is
no existing file, or if the existing file doesn't have the start marker.
On subsequent releases, it will contain all the existing inner content,
so new entries appear at the top of the list.

The start marker is always the string `### VERSIO BEGIN CONTENT ###`
regardless of what format the template is, and the end marker is `###
VERSIO END CONTENT ###`.

## Builtin templates

Versio currently supports two builtin templates: `html` and `json`.

### HTML template

The `html` builtin template is a simple HTML template that creates a new
entry for a PR, and stacks the new entry on top of all previous entries
via the `old_content` property. Values have a simple CSS/JavaScript
tree, so each entry, each PR in an entry, and each commit in a PR can be
expanded/collapsed for easy viewing.

The style itself has minimal decoration and text, but it does provide
the important information about each release, as well as links to the
PRs and commits where available. You can use this template directly, or
print it with `versio template show --template=builtin:html` and use it
as a basis for your own templates.

### JSON template

The `json` builtin template is a simple JSON document that is primarily
used to output a given release plan via `versio plan
--template=builtin:json`. No start/end markers are present in this
template, and the `old_content` variable is not used; so it outputs only
data from the current plan.

The JSON document generated is comprehensive, and includes all the
information about the current release that is available to the changelog
system. Since the output from this template is in machine-readable
format, you can use it as useful input to your own changelog generation,
if you want to do something beyond the capabilities described here.
