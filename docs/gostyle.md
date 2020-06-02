# Go Projects

Go projects don't have a manifest file. Instead, versions are tracked
via published tags in the repo, which have the form
`v<major>.<minor>.<patch>` e.g. `v2.4.15`. Furthermore, many Go projects
keep a separate directory for each major release after v1, leading to a
layout that looks like this example:

```
projectName
├─ (project and code files for v0, v1)
├─ v2
│  └─ (project and code files)
└─ v3
   └─ (project and code files)
```

Some Go projects choose to develop major releases on separate branches,
while some release everything from different directories on the same
branch. Versio can support both models, but it can't currently generate
a single release that spans multiple branches.

Using a "tags" style manifest for a project in `.versio.yaml` causes the
project to (a) use Git tags (instead of a manifest file) to track
versions; and (b) search for major versions in `vN`-named
subdirectories. The simplest example is this:

```yaml
located:
  at: projectName
  tags:
    all: {}
```

This allows for standard go-style subdirectories on any branch inside of
the `projectName` folder. You can use `at: .` if your project exists at
the top-level of your repository. Projects that use this style of
manifest are required to have `tag_prefix` property.

> Warning! This style of project requires the `tag_prefix` property to
> be present, which creates/updates git tags like
> `<<tag_prefix>>-v1.2.3` for the project. Since only one project in the
> repository can have a `tag_prefix` of "" (the empty string results in
> Go-standard tags without a prefix like `v1.2.3`), this makes it
> difficult to properly deploy monorepos that contain more than one
> Go-style project.

You can express the branch / directory structures in finer detail when
needed. If, for example, part of your project integrates with [Google's
JavaScript engine](https://v8.dev/), `v8` might be directory name
reserved for a software component; you might plan to use `vers_8` for
the 8th major version of your project, with a layout like this:

```
projectName
├─ v8 ("v8" software component)
├─ (other project and code files for v0, v1)
│
├─ v2
│  ├─ v8 (component)
│  └─ (other project and code files)
│
├─ (maybe v3-v7)
│
├─ vers_8
│  ├─ v8 (component)
│  └─ (other project and code files)
│
└─ v9
   ├─ v8 (component)
   └─ (other project and code files)
```

The project could have this `located` property in `.versio.yaml`:

```yaml
located:
  at: projectName
  tags:
    all:
      branch: master
    v8:
      branch: master
      path: vers_8
```

> WARNING: While this sort of thing is possible, it is not recommended:
> inconsistent naming will probably confuse other tools and users.
> Future versions of Versio may address the need to customize major
> subdirectory names with a different approach.

Use `path: {}` if your project does not (yet) have a subdirectory for a
major. You can use `path: .` if a specific version exists in the top of
the project rather than a subdirectory: this is the default for v0 and
v1, per Go conventions.

By using a "branch" specifier, you will ensure that Versio will only
increment the project version if you run it on the given branch. This is
a safety valve to prevent you from creating a new branch for a new
major, but forgetting to update your `.versio.yaml` for it. This only
affects the `run` command; commands like `plan` or `check` are
unaffected, so they can still be used for pre-merge validation on
feature branches.

If your major version is supposed to exist in a subdirectory, Versio
will warn you if it detects a major release is needed that does not use
that subdirectory.
