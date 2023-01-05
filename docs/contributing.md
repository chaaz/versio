# Contributing to Versio

This page is for developers that want to contribute to the Versio
application. It assumes that you already have basic familiarity with
building Rust programs.

See ex. [the book](https://doc.rust-lang.org/book/index.html), or
elsewhere on the internet for help getting started with rust.

Also, make sure you've read the project [README](../README.md) and
[Installation page](./installing.md) so that you have an idea of how
Versio must be installed.

## Project structure

Here is the structure of "versio":

```
versio
├─ LICENSE.md
├─ README.md
├─ docs
│  ├─ contributing.md  . . . . . .  this document
│  └─ ...
├─ Cargo.toml  . . . . . . . . . .  project file
├─ Cargo.lock  . . . . . . . . . .  deps locking
├─ rustfmt.toml  . . . . . . . . .  format config
├─ src
│  └─ ...          . . . . . . . .  src and unit tests
├─ tests
│  └─ ...          . . . . . . . .  integration tests
└─ .github
   └─ ...          . . . . . . . .  GitHub Actions
```

<!--
TODO: Further work:

└─ builds
   ├─ deploy
   │  ├─ build-app.sh  . . . . . .  deploy build script
   │  └─ ...       . . . . . . . .  deploy support
   └─ test
      ├─ service-tests.sh  . . . .  test run script
      └─ docker-compose.yml  . . .  test run config
-->

## Running

The `versio` app is very simple with minimal runtime dependencies; you
can run it locally as described in the [use cases doc](./use_cases.md).

## Dev Guidelines

[dev guidelines]: #dev-guidelines

Here are the development guidelines for Versio. In order to practice
them, you may need to install some tools:

```
$ rustup toolchain install nightly
$ rustup component add rustfmt --toolchain nightly
$ cargo install cargo-audit
$ rustup component add clippy
```

### Warnings

Unless there's a very good reason, you should never commit code that
compiles with warnings. In fact, it is suggested that you set the
`RUSTFLAGS='-D warnings'` before building, which treats all warnings as
errors. Most rust warnings have reasonable work-arounds; use them.

For example, "unused variable" warnings can be suppressed by starting
the variable name with an underscore (`_thing`). Of course, it's always
better to re-factor the code so that the variable doesn't exist, where
possible.

### Style

We generally adhere to the principles of "Clean Code", as described in
the first half of [Clean
Code](https://www.amazon.com/Clean-Code-Handbook-Software-Craftsmanship-ebook-dp-B001GSTOAM/dp/B001GSTOAM/ref=mt_kindle?_encoding=UTF8&me=&qid=1541523061).
This means well-chosen names; small, concise functions; limited, well
written comments; and clear boundaries of abstraction.

We also follow best Rust and Cargo practices: using references,
iterators, functional techniques, and idiomatic use of `Option`,
`Result`, `?` operator, and the type system in general. Most of this is
shown clearly in the [the
book](https://www.amazon.com/Clean-Code-Handbook-Software-Craftsmanship-ebook-dp-B001GSTOAM/dp/B001GSTOAM/ref=mt_kindle?_encoding=UTF8&me=&qid=1541523061)

### Documentation

You should keep all technical documentation--including the top-level
README, code comments, and this document--up-to-date as you make changes
to tests and code.

### Coding Format

**Always format your code!** You should format your (compiling, tested)
code before submitting a pull request, or the PR will be rejected.

We use the nightly [rust
formatter](https://github.com/rust-lang-nursery/rustfmt), and have a
`rustfmt.toml` file already committed for its use. Run `cargo +nightly
fmt -- --check` to preview changes, and `cargo +nightly fmt` to apply
them.

### Linting

**Always lint your code!** You should lint your code before submitting a
pull request, or the PR will be rejected.

[Clippy](https://github.com/rust-lang/rust-clippy) is the standard cargo
linter. Run `cargo clippy` to run all lint checks.

### Security/Dependency Auditing

**Always audit your dependencies!** If you don't, your PR will be
rejected.

We use the default [cargo audit](https://github.com/RustSec/cargo-audit)
command. Run `cargo audit --deny-warnings` to perform a vulnerability
scan on all dependencies. The `--deny-warnings` flag treats warnings as
errors.

### Testing

**Always run tests!** Obviously, if your code fails any unit or service
test, then your PR will be rejected.

Any new modules created should have their own set of unit tests.
Additions to modules should also expand that module's unit tests. New
functionality should expand the application's integration tests.

Run `cargo test` to run all unit tests.

## GitHub Actions

Versio uses
[Yambler](https://github.com/chaaz/versio-actions/tree/main/yambler) in
order to more easily handle repetitive GitHub Actions. The main
workflows are saved in `.github/workflows-src/` and snippets in
`.github/snippets/`. When you create or change workflows, just run the
script `yamble-repo.sh` (available from
[here](https://github.com/chaaz/versio-actions/blob/main/scripts/yamble-repo.sh))
which generates workflow files into `~/.github/workflows`.

**DON'T EDIT THE WORKFLOW FILES DIRECTLY**. You should only edit the
workflow sources in `workflow-src`, or the snippets in `snippets`, and
then run the `yamble-repo` script. You will still need to
add/commit/push the generated files in `workflow`, however, in order for
GitHub Actions to use them.

As mentioned in the Yambler README, you can copy the companion script
`yamble-repo-pre-push.sh` to a file named `.git/hooks/pre-push` in your
local copy of the `versio` repo. This will ensure that your workflows
are synced before you share them.

## Platform-specific help

[platform-specific help]: #platform-specific-help

### Linux

[linux]: #linux

When building on linux, you should set the following environment
variables before running `cargo build` or `cargo install`. The options
below are the standard locations for Ubuntu 18 (bionic).

```
$ export RUSTFLAGS='-D warnings -C link-args=-s'
```

`link-args=-s` passes `--strip-debug` to the linker, and ensures that
the resulting executable is a reasonable size: without that option, the
binary easily expand to over 100M. If you forget to include this option,
you should manually run `strip` on the resulting executable.

You also need to install some gpg libraries first:

```
sudo apt update
sudo apt install libgpg-error-dev
sudo apt install libgpgme-dev
```

### Windows

[windows]: #windows

We compile using the MSVC ([M]icro[S]oft [V]isual studio [C]omponents)
toolchain, which is the default. The latest version of Rust allows you
to install Community Edition C Components as part of the Rust MSVC
target installation; this is the easiest option if you don't already
have Visual Studio installed. Otherwise, you'll need to install MSVC
Visual Studio (Community Edition 2017 is tested) or its runtime
components yourself.

Because of the distribution of the GnuPG libraries for Windows, we build
using the MSVC 32-bit toolchain to cross-compile for the GNU 32-bit
target. It may be possible to build solely with the GNU toolchain via
MSYS2 and/or Mingw32, but this is currently untested. Additionally,
statically linking in the GnuPG libraries is problematic, even if you
can get it to work, and is not recommended.

In order to cross-compile to the windows-gnu target, the Gnu toolset
must be installed: the best way for that is to install
[MinGW](https://www.mingw-w64.org/) (Minimum Gnu for Windows).

Additionally, you need to have the GpgME (GnuPG Made Easy) libraries
installed to build. We've tested building with MinGW version 8.1.0.

Using [Chocolatey](https://chocolatey.org/) to install both GpgME and
MinGW is probably the easiest. Once you've installed Rust and
Chocolatey, you can run the following commands. For more details,
examine our GitHub Actions "release" workflow, which generates binaries
for all platforms.

```
choco install -y gnupg
choco install mingw --version 8.1.0 --x86 -y
choco install pkgconfiglite -y

rustup toolchain add stable-i686-pc-windows-msvc
rustup target add --toolchain stable-i686-pc-windows-msvc i686-pc-windows-gnu
rustup default stable-i686-pc-windows-msvc
rustup set default-host i686-pc-windows-gnu

refreshenv  # or just restart the terminal window

# to install:
cargo +stable-i686-pc-windows-msvc install --target i686-pc-windows-gnu versio

# or just build:
cd VERSIO_REPO
cargo +stable-i686-pc-windows-msvc build --target i686-pc-windows-gnu
```

### MacOS

[macos]: #macos

You need to install the GpgME (GnuPG Made Easy) libraries before running
`cargo build` or `cargo install`; [homebrew](https://brew.sh/) is
probably the easiest:

```
brew update
brew install gpgme
```
