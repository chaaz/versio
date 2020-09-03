# Common Use Cases

These are some of the common ways that you might want to use Versio in
your own development. If you find a novel way to use Versio, please let
us know!

## Quick Start

Get up and running quickly with Versio, and get a brief introduction to
what it does. This example assumes a standard Node.js/NPM layout, but
Versio can handle lots of different project types.

- Install versio:
  ```
  $ cargo install versio  # or download a pre-built binary to your PATH
  ```
- Create and commit a simple config file:
  ```
  $ git pull
  $ versio init  # this creates .versio.yaml
  $ git add .versio.yaml
  $ git commit -m "build: add versio management"
  $ git push
  ```
- If you want to use the GitHub API for [PR scanning](./pr_scanning.md), you'll need to
  update your `~/.versio.rc.toml` file: See the
  [Reference](./reference.md#github-api).
- After some [conventional
  commits](https://www.conventionalcommits.org/), update it:
  ```
  $ versio run
  Executing plan:
    project : 1.0.1 -> 1.1.0
  Changes committed and pushed.
  ```

## Manual Changes (solo project)

If you have a single project configured, and you want to manually view
and set its version:

- Take a look at your project:
  ```
  $ cd ${project_root_dir}

  $ cat package.json
  ...
    "name": "myproject"
    "version": "1.0.1",
  ...
  ```
- View your current version:
  ```
  $ versio get
  myproject : 1.0.1
  ```
- Change it
  ```
  $ versio set --value 1.2.3

  $ cat package.json
  ...
    "version": "1.2.3",
  ...
  ```

## Manual Changes (multiple projects)

If you have more than one project configured, and you want to manually
view and set the version of one of them. You must know the ID or the
name of the project you want to affect:

- Take a look at your project:
  ```
  $ cd ${project_root_dir}

  $ cat proj_1/package.json
  ...
    "version": "1.0.1",
  ...
  ```
- View the current version:
  ```
  $ versio get --id 1
  myproject : 1.0.1
  ```
- Change it
  ```
  $ versio set --id 1 --value 1.2.3

  $ cat proj_1/package.json
  ...
    "version": "1.2.3",
  ...
  ```

## Create Configuration

To start using Versio, you should create a `.versio.yaml` config file in
your repo. Use the following command to do so. Make sure you're in the
top-level directory of your repository (or the top-level directory of
your non-version-controlled monorepo) when you do so:

```
$ versio init
```

This will scan your repo for existing projects, and create a new config
file with each of those projects listed. If you change later add,
remove, or change the location of your projects, you should edit this
file by hand to keep it up-to-date.

<!--

## CI Premerge Checks

> TODO

`versio check`, `versio plan` maybe?

> TODO supply CI orbs, github actions ?

## CI Merge

> TODO

> TODO: talk about release branches

> TODO: talk about timing. TIMING IS KEY. can't merge to release branch
> while `versio run` is executing

`versio plan` maybe, `versio run`

## CD Deploy

> TODO

`versio publish`

-->
