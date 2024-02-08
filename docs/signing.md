> (This page describes the new version of signing, available for
> versions of Versio equal to or greater than 0.8.0. To see the old
> approach, go [here](./signing-old.md))

# VCS Signing

Versio is capable of reading tags and commits which have been signed,
and can also sign the tags and commits that it generates.

## Description

Git has the ability to sign commits with the `-S` flag, and to sign
annotated tags with the `-s` flag. Versio has the ability to read these
commits and tags, and to sign its own commits and tags, as long as the
right configurations are created.

### Reading signed work

Versio will read all commits and tags that you've manually created with
a signature automatically--no configuration is required.

> While Versio has no problem reading signed commits and tags, it
> currently does not _verify_ those signatures--you'll need to do that
> on your own. The
> [Git Documentation](https://git-scm.com/book/en/v2/Git-Tools-Signing-Your-Work)
> has some good information on how to do that.

### Signing your work

> **NOTE:** Versio no longer requires GPG specifically: instead, it uses
> an [OpenPGP](https://www.openpgp.org/) key file that is available from
> any OpenPGP-compatible software (which includes GPG).

Versio commit and tag signing uses
[Sequoia-PGP](https://sequoia-pgp.org/), which is licensed under
[Creative Commons 4.0](https://creativecommons.org/licenses/by/4.0/):
see that document for terms and conditions. Sequoia-PGP is not
associated with Versio.

If you want Versio to sign your commits and/or tags, you need to have
created an OpenPGP key file. The details of creating such a file depends
on your security software. If you're using GPG on a Unix-based system,
for example, you could do something like this (where `<VERSIO_KEY_ID>`
is the ID of the private key you want Versio to use to sign your
commits).

```
mkdir -p $HOME/.keys
gpg --export-secret-keys <VERSIO_KEY_ID> > $HOME/.keys/versio-signer.pgp
chmod 600 $HOME/.keys/versio-signer.pgp
```

Once you have a key file for signing, you need to update Git
configuration values:

- `commit.gpgSign`: set this to `true` to convince Versio to sign its
  commits. Versio may create one or more commits when it runs the
  `release` command, in order to commit changelogs and manifest files
  with updated versions.
- `tag.forceSignAnnotated` or `tag.gpgSign`: set either of these to
  `true` to convince Versio to sign the "prev tag" (default:
  `versio-prev`) that it creates on release. Other tags (such as
  per-project tags created from a project's `tag_prefix` configuration)
  will not be signed, since they are not annotated tags.
- `versio.keypath`: This is the path to the key file you have created,
  e.g. `/my/path/to/versio-signer.pgp`. If either of the above options
  are `true`, then this configuration must be set.

The `git` [command-line](https://git-scm.com/docs/git-config) can set
global or per-repository configuration. For example:

```
git config --global --add versio.keypath $HOME/.keys/versio-signer.pgp
```

### Password Protection

> As always, you should have a thorough understanding of all your
> environments and workflows before making any security decisions, so
> that you avoid introducing vulnerabilities.

Currently, Versio is unable to read key files that are
password-protected. It is often recommended that you don't leave
unprotected key files on your computer, especially if there is a risk of
other users gaining access to it. If this is the case for you, there are
some options for using Versio:

1. Use your PGP software (e.g. GPG) to create the unprotected key file
   before using Versio, and then delete the key file afterwards.
1. Use your PGP software to remove the password protection from the key
   file before using Versio, and then re-add it afterwards.

In both options above, your PGP software may prompt you for a password
to create an unprotected key file.

Handling a prompt may not always be a feasible solution: you might to
run Versio in an automated CI/CD pipeline which can't stop to type in a
value, or as part of a script which doesn't have the capacity to display
or read from prompts. Or, you might just not like the constant
interruption of being asked for a password.

There are some solutions to this problem:

- If you're running in a CI/CD environment such Github Actions, you may
  be able to use a plugin such as [Import
  GPG](https://github.com/marketplace/actions/import-gpg), which injects
  CI/CD secrets into the GPG passwords table without requiring a prompt.
- You might be able to provide a non-password-protected key file in a
  write-only environment, container secret, securely mounted volume, or
  some other means that mitigates the need for password protection.
- Don't attempt to always sign your work--not every commit or tag has to
  be signed. If your workflow makes it hard to provide a key password
  for you signature, reconsider if you need one.
- If you're running the commands manually, most modern operating systems
  and PGP software have options to password prompt only occasionally.
  For example, MacOS has the ability to integrate GPG passwords in the
  MacOS Keychain, which means that you only get prompted for your
  password only once (in a while). Similar tools and configurations
  exist for Windows and Linux workstations.
- You can simply use a key file that is not password protected,
  especially if it's used on a system with limit access.
