# Installing

We've tried to make Versio as self-contained as possible, to make it
easy to install and run. Depending on your computer's configuration,
though, you may need to additionally install some dependencies. Here
we'll help you get those satisfied quickly, so that Versio can run as
soon as you're ready.

## Versio Itself

The easiest way to install Versio is to just download the latest binary
from our [Releases page](https://github.com/chaaz/versio/releases). On
Windows (and rarely on Linux and MacOS), you may also need to install
GnuPG.

Versio is written in the Rust programming languague. If you have the
[Rust](https://www.rust-lang.org/tools/install) development environment
installed, you can build Versio from the source:

```
$ cargo install versio
```

There may be caveats building for your particular platform: see
[Platform-specific help](./docs/contributing.md#platform-specific-help)
in our contributions document.

## GnuPG

Older versions of Versio (0.7 and earlier) required GnuPG (a.k.a. _GPG_)
to be installed, which introduced some complexity in their requirements.
However, Versio is now built using a secure
[OpenPGP](https://www.openpgp.org/) library, and works with any
OpenPGP-compatible security software (including GPG).

If you've been using an older version of Versio with GPG, and would like
to upgrade, read the current [VCS Signing](signing.md) page to learn how
to use GPG to create and configure an OpenPGP-compatible key file for
use with Versio.
