# Dependencies

We've tried to make Versio as self-contained as possible, to make it
easy to install and run. However, there are some external requirements
that are unavoidable. This page is designed to help you get those
satisfied quickly, so that Versio can run as soon as you're ready.

The good news is: if you're going to use Versio on a typical laptop,
server, or CI installation, chances are you that you already have
everything you need.

## GnuPG

GnuPG (sometimes called _GPG_) is a complete and free implementation of
the OpenPGP standard as defined by
[RFC4880](https://www.ietf.org/rfc/rfc4880.txt) (also known as _PGP_).
GnuPG allows you to encrypt and sign your data and communications; it
features a versatile key management system, along with access modules
for all kinds of public key directories. See the [GnuPG
homepage](https://gnupg.org/) for more.

Versio uses GPG to read cryptographically signed version tags, as well
as to sign its own commits and tags in a manner consistent with Git
workflows; read more on the [Signing page](./signing.md).

It's impossible to fully bake the GPG toolchain into Versio itself,
since some of the work of GPG is done by connecting to or spawning
external tools (such as `gpg-agent`). We've done as much as is feasible
for each platform, though, so you should need to do the least amount of
work to get this dependency installed.

### Linux

Linux Versio requires only GnuPG to be installed, and most Linux
distributions come with it pre-bundled. If you can run the `gpg`
program, you probably already have what is necessary. Linux Versio has
been tested using GnuPG version 2.2.20 and 2.3.1: run `gpg --version` to
see what version you have.

If GnuPG is not installed, you may be able to install it with your
package manager: e.g. `sudo apt-get update && sudo apt-get install
gnupg` for Debian-based distributions.

### MacOS

MacOS Versio requires only GnuPG to be installed, and most MacOS
computers that can run `git` already have it. If you can run the `gpg`
program from a terminal, you probably already have what is necessary.
MacOS Versio has been tested using GnuPG version 2.3.1: run `gpg
--version` to see what version you have.

If GnuPG is not installed, you may be able to install it as part of
XCode command-line tools, which are optionally bundled with XCode, but
also available separately. Run `xcode-select --install` to install these
tools. Or if you use [Homebrew](https://brew.sh/), you can use `brew
install gnupg` to get the latest version.

### Windows

Windows requires GnuPG and its associated dynamic libraries to be
installed, but most Windows distributions don't have these by default.
In order to install everything, you should download and run the GnuPG /
GpgME package self-executing installer, which is available
[here](https://gnupg.org/ftp/gcrypt/binary/gnupg-w32-2.3.1_20210420.exe)
(The signature and checksum available from its [parent
directory](https://gnupg.org/ftp/gcrypt/binary/)). Or
[Chocolatey](https://chocolatey.org/) users can run `choco install
gnupg` to install the appropriate libraries and toolchain.

Once installed, you should see the `C:\Program Files (x86)\gnupg`
directory, with a bunch of files and folders inside it. Windows Versio
has been tested with GnuPG version 2.3.1: run `"C:\Program Files
(x86)\gnupg\bin\gpg.exe" --version` to check your version.

When running git from `msys2` or `mingw` installation (for example, [git
for Windows](https://gitforwindows.org/), you should ensure that your
PATH environment includes `/c/Program Files (x86)/gnupg/bin` **first**,
so that the gpg programs are run from the gpgme installation, and not
from the gpg programs included in the shell.
