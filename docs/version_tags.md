# Version Tags

Some project types don't have a manifest file (for example: Go or
Terraform projects). Instead, versions are tracked via published tags in
the repo, which have the form `v<major>.<minor>.<patch>` e.g. `v2.4.15`.

Using a "tags" style manifest for a project in `.versio.yaml` causes the
project to use VCS tagging instead of a manifest file to track versions.

```yaml
tag_prefix: "projname"
version:
  tags:
    default: "0.0.0"
```

The `tag_prefix` property causes Versio to write out a new
"[tag\_prefix]-v*x.y.z*" tag for the project when the version number is
changed. The property is optional for most projects, but required for
projects that use `version: tags`. The default value is used when no
existing "projname-v*x.y.z*" tags currently exist.

> Since `tag_prefix` is used to find older tags of a project, you should
> not change this property value over the life of your project. If you
> change the `tag_prefix`, you may need to manually re-tag your commit
> history, or else Versio may be unable to locate past version numbers.

> If a project uses `version: tags:`, you may want to use the
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
