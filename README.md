# Versio

A simple tool to help manage versions in a monorepo.

Versio will scan all git commits that were subitted since its last run,
and will increment all applicable version numbers by using [conventional
commits](https://www.conventionalcommits.org/en/v1.0.0/)

## Multiple numbers

In order to locate version numbers, versio requires that you have a file
named `.versio/numbers.yaml` checked into the top level of your git
repository. The file should look something like this:

```
projects:
  - name: everything
    id: 1
    covers: ["**"]
    located:
      file: "toplevel.json"
      json: "version"

  - name: project1
    id: 2
    covers: ["project1/**"]
    located:
      file: "project1/Cargo.toml"
      toml: "version"

  - name: "combined a and b"
    id: 3
    covers: ["nested/project_a/**", "nested/project_b/**"]
    located:
      file: "nested/version.txt"
      pattern: "v([0-9]+\\.[0-9]+\\.[0-9]+) .*"

  - name: "build image"
    id: 4
    depends: [2, 3]
    located:
      file: "build/VERSION"
```

```
commands:
  check
  get --prev --name comb --wide
  get --prev --id 4
  show --prev --wide
  set --name comb --value 1.2.3
  set --id comb --value 1.2.3
  diff --no-fetch
  files --no-fetch
  plan --no-fetch
  bump --name comb
  run --commit --push
```

`plan`: Plans wrt the **previous** projects, covers, and dependencies;
but uses **current** sizes, and returns results wrt **current**
projects.

## How it works:

```
Git Pull

Looks for (versio) tag ancestor, and gets all versions from that commit
(reading the **previous** .versio) (or none, if it can't find a (versio)
tag in the branch). Then examines conventional commit history since
then, using **current** covers (TODO: adapted covers?) to determine the
minimum value that each new ID should have. (If the new ID isn't in the
previous map, it's assumed at '0.0.0').

It then looks at existing **current** versions, and if the version isn't
high enough, it bumps it to the minimum value.

Git Commit

Git Push

If encounters conflict when pushing, it throws everything out (including
tag advance), pulls, and tries again.
```

## Troubleshooting:

Rebase might cause the last (versio) tag to not be an ancestor. What
then?
