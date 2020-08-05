# Common Use Cases

These are some of the common ways that you might want to use Versio in
your own development. If you find a novel way to use Versio, please let
us know!

## Quick Start (Future)

> The following assumes features that haven't yet been implemented
> (auto-projects, assumed-config, missing-prev\_tag, single-proj-elide).
> See the `Quick Start` section below for getting started without these
> features.

Get up and running quickly with Versio, and get a brief introduction to
what it does. This example uses a Node.js layout with `package.json`,
but Versio works with all kinds of projects.

- Install Versio:
  ```
  $ cargo install versio  # or download a pre-built binary to your PATH
  ```
- Take a look at your project:
  ```
  $ cd ${project_root_dir}

  $ cat package.json
  ...
    "version": "1.0.1",
    "name": "myproject",
  ...
  ```
- Look at your current version:
  ```
  $ versio show
  myproject : 1.0.1
  ```
- Change it (and change it back):
  ```
  $ versio set --value 1.2.3

  $ cat package.json
  ...
    "version": "1.2.3",
  ...

  $ versio set --value 1.0.1
  ```
- After a few [conventional
  commits](https://www.conventionalcommits.org/), update it:
  ```
  $ versio run
  Executing plan:
    myproject : 1.0.1 -> 1.1.0
  Changes committed and pushed.
  ```

## Quick Start

Get up and running quickly with Versio, and get a brief introduction to
what it does. This example assumes a standard Node.js layout, but you
can adjust your config easily to something else.

- Install versio:
  ```
  $ cargo install versio  # or download a pre-built binary to your PATH
  ```
- Take a look at your project:
  ```
  $ cd ${project_root_dir}

  $ cat package.json
  ...
    "version": "1.0.1",
  ...
  ```
- Create a simple config file:
  ```
  $ git pull
  $ cat > .versio.yaml << END_OF_CFG
  projects:
    - name: project
      id: 1
      includes: ["**/*"]
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
  $ git add .versio.yaml
  $ git commit -m "build: add versio management"
  $ git push
  $ git tag -f versio-prev
  $ git push -f origin versio-prev
  ```
- Look at your current version:
  ```
  $ versio show
  project : 1.0.1
  ```
- Change it (and change it back):
  ```
  $ versio set --id 1 --value 1.2.3

  $ cat package.json
  ...
    "version": "1.2.3",
  ...

  $ versio set --id 1 --value 1.0.1
  ```
- After a few [conventional
  commits](https://www.conventionalcommits.org/), update it:
  ```
  $ versio run
  Executing plan:
    project : 1.0.1 -> 1.1.0
  Changes committed and pushed.
  ```

## Create / Update Configuration

After you've started using Versio, you may want to create a
`.versio.yaml` config file in your repo, to take advantage of more
features and improve performance. Use the following command to do so:

```
$ versio init
```

This will scan your repo for existing projects, and create a new config
file with those projects. To add new projects to the repo, you can
either add them manually to your `.versio.yaml` file, or re-scan your
repo:

```
$ versio scan
```

## CI Premerge Checks

> TODO

`versio check`, `versio plan` maybe?

> TODO supply CI orbs, github actions ?

## CI Merge

> TODO

> TODO: talk about release branches

> TODO: talk about timing : can't merge to release branch while `versio
> run` is executing

`versio plan` (maybe), `versio run`

## CD Deploy

> TODO

`versio publish`
