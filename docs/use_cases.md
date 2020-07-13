# Common Use Cases

These are some of the common ways that you might want to use Versio in
your own development. If you find a novel way to use Versio in your
projects, please let us know!

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
  {
    "version": "1.0.1",
    ...
  }
  ```
- Create a simple config file:
  ```
  $ git pull
  $ cat > .versio.yaml << END_OF_CFG
  projects:
    - name: my-project
      id: 1
      covers: ["**/*"]
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
  commits](https://www.conventionalcommits.org/), update it:
  ```
  $ versio run
  Executing plan:
    my-project : 1.0.1 -> 1.1.0
  Changes committed and pushed.
  ```
