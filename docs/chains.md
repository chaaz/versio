# Versio Version Chains

Versio allows you to perform some simple dependency management inside of
a monorepo. If you use _implicit versioning_ in your monorepo, chaining
can save you a lot of time.

## The Problem

Imagine a scenario where some 3rd party Nodejs library upgrades to a new
version. If your core project depends on that library, you might update
your core `package.json` to upgrade to the new version. Or maybe you
just run `npm update` and let it update your `package-lock.json`. Either
way, you'll end up bumping your core version number, and re-releasing
the new core library with the new library.

But then, maybe you have a top-level application that uses your core
lib. You'll do the same thing: change the core version in your
dependency list to match, bump the app's own version, and re-release it.

Of course, once you do _that_, you might also have a Helm chart that
deploys your app to a kubernetes cluster. And, you need to do the same
thing _again_: update the version number used by the Deployment resource
to match the new application release, then bump the chart's own version,
and re-release *it*.

And so on, and so forth, to your Dockerfiles, other sub-projects,
packging projects, Terraform modules, etc. etc.

Now, imagine that your core project, top-level app, and your chart all
lived in the same monorepo, and that the release process occurs only
when you merge to the release branch using conventional commits.
Running this dependency process would mean that:

1. You change your core library `package.json`, and commit it with a
   message like "fix: update 3rd party".
2. Your changes are merged to release, and then CI/CD (eventually) pops
   out a new release number for the core lib.
3. You use the new release number in your top-level app, committing with
   a message like "fix: update core lib"
4. Your changes are merged to release, and then CI/CD releases that.
5. etc, etc.

Obviously, this involves a lot of commits in the same monorepo, and a
lot of waiting for your pipeline to generate releases.

## The Solution

If you're managing your release via Versio, then you can list explicit
dependencies between the projects, so that all versions get
automatically updated during the same release.

Here's a snippet from `.versio.yaml` that has version chaining:

```
projects:
  - name: proj_1
    id: 1
    root: "proj_1"
    version:
      file: "package.json"
      json: "version"

  - name: proj_2
    id: 2
    root: "proj_2"
    depends:
      1:
        size: patch
        files:
          - file: "package.json"
            json: 'dependencies.@myorg/core'
    version:
      file: "package.json"
      json: "version"
```

You can see that the `depends` property of `proj_2` lists a dependency
to project 1. The `size: patch` means that any time `proj_1` changes,
`proj_2` gets at least a patch-level increment to its own version (other
options are `none`, `minor`, `major`, or `match`). Also, it lists the
location in files that need to change to match the new version of
`proj_1`, which have the same format as the `version` property of files.

### Formatting output

When writing depends files, you don't need to write the exact version
string: You can instead use a [liquid](https://crates.io/crates/liquid)
template to edit how you want to write the value. The context of the
template has just a single value, "v", which is the depended-on version
string. For example, you can write a two-digit NPM version syntax:

```
depends:
  1:
    size: patch
    files:
      - file: "package.json"
        json: 'dependencies.@myorg/core'
        format: '{% assign a = v | split "." %}^{{a[0]}}.{{a[1]}}'
```
