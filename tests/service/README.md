# Versio service tests

Because of the nature of what versio does, its service tests are a bit
unique.

## Setup

By default, service tests run in a Docker Compose network, in order to
isolate its filesystem activities, and to allow it to spoof the
`github.com` endpoint (see "Smart Tests" below). It is possible to run
most service tests directly on your machine, though, if you don't have
(or don't want to use) Docker.

To run the service tests inside of Compose, just run the `service-tests`
script directly: (TODO: write this)

### Bare Setup

You can run service tests "bare" on your local box instead of using
Compose. The tests can be run without any alteration to your box, and
will run on your local filesystem. You must have Git installed to run
these tests. To run "smart" tests, you must also have the GitHub
Emulator (`ghe`) installed. (TODO: build GHE)

**Warning**: Running service tests "bare" will use your local `git`
command under your user with your git configuration, and will create and
alter files on your machine (in `/tmp`, or wherever your OS puts
temporary files). For "smart" tests, using the GitHub emulator will
listen on a TCP socket (default port 8282) for the test duration.

## Basic Structure

Since versio works primarily by manipulating files and git repositories,
we've created a simple service test framework that works in a shell. For
example, a service test might look like this:

```
init_repo_mono

cat >> proj_1/file.txt <<< new_feature
git add proj_1/file.txt
git commit -m "feat: add new feature to proj_1"

cat >> proj_2/file.txt <<< bug_fix
git add proj_2/file.txt
git commit -m "fix: bug fix proj_2"

versio_capture run

grep -F -q -x '  proj_1: 0.0.1 -> 0.1.0' ${versio_stdout}
grep -F -q -x '  proj_2: 0.0.1 -> 0.0.2' ${versio_stdout}

grep -F -q '"version": "0.1.0"'  proj_1/package.json
grep -F -q 'version = "0.0.2"'   proj_2/Cargo.toml
```

Service tests are run in a simple Git repository, whose initial
structure is determined by a `init_repo_xxx` command, which must be the
first command. In the above example, `init_repo_mono` creates a
versio-ready monorepo with an NPM project in a `proj_1` subdirectory,
and a Cargo project in `proj_2`.

> TODO: List all `init_repo` options

In addition to the `init_repo_2` command, the `versio_capture` command
is also provided, which is just like running `versio`, except that its
output are captured to the files `${versio\_stdout}` and
`${versio\_stderr}`.

## Remote Tests

For some service tests, a remote repository is required to ensure that
`versio` is capabile of performing remote actions. Some `init_repo`
commands create a `remote` that actually lives locally, so service tests
that involve remote Git activity (push/pull/fetch etc) can be executed
without involving a network.

## Smart Tests

Some service tests require "smart" remote activity (see [VCS
Levels](../../docs/vcs_levels.md)) to unsquash or group commits by pull
request. These service tests run as normal, but must be run in a 

> TODO: actually create `gh_em` and docker compose.
