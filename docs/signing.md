# VCS Signing

Versio is capable of reading tags and commits which have been signed,
and can also sign the tags and commits that it generates.

## Description

Git has the ability to sign commits with the `-S` flag, and to sign
annotated tags with the `-s` flag. Versio has the ability to read these
commits and tags, and to sign its own commits and tags, as long as the
right GPG and Git configurations are created.

### Reading signed work

Versio will read all commits and tags that you've manually created with
a signature automatically--no configuration is required.

> Note: while Versio has no problem reading signed commits and tags, it
> currently does not _verify_ those signatures--you'll need to do that
> on your own. The
> [Git Documentation](https://git-scm.com/book/en/v2/Git-Tools-Signing-Your-Work)
> has some good information on how to do that.

### Signing your work

If you want Versio to sign your commits and/or tags, you need to have
[GPG](https://gnupg.org/) installed, and have one or more keys created
on your GPG keyring. You then need to have some of the following `git`
standard configuration options set:

- `user.signingKey`: if this is set, then the given value identifies
  which key to use to sign commits and tags. If this value is not set,
  then your default key will be used. This value must be the ID of one
  of your keys: use `gpg --list-keys --keyid-format 0xLONG` to see the
  IDs--the show up as e.g. `rsa1024/0xKEY_ID_HERE`.
- `commit.gpgSign`: set this to `true` to convince Versio to sign its
  commits. Versio may create one or more commits when it runs the
  `release` command, in order to commit changelogs and manifest files
  with updated versions.
- `tag.forceSignAnnotated` or `tag.gpgSign`: set either of these to
  `true` to convince Versio to sign the "prev tag" (default:
  `versio-prev`) that it creates on release. Other tags (such as
  per-project tags created from a project's `tag_prefix` configuration)
  will not be signed, since they are not annotated tags.

### Password Interruptions

It is generally recommend when you create keys, that you protect them
with a strong password. This prevents malicious operators from using
your keys if they gain access to your computer, or if you accidently
release your keys into the wild. However, this means that when you run
`gpg`, `git`, or `versio`, that you may be prompted for your password to
sign data with your keys.

Handling a prompt may not always be a feasible solution: you might be
running Versio in an automated CI/CD pipeline which can't stop to type
in a value, or as part of a script which doesn't have the capacity to
display or read from prompts. Or, you might just not like the constant
interruption of being asked for a password.

There are some solutions to this problem:

- If you're running in a CI/CD environment such Github Actions, you may
  be able to use a plugin such as [Import
  GPG](https://github.com/marketplace/actions/import-gpg), which injects
  CI/CD secrets into the GPG passwords table without requiring a prompt.
- Don't attempt to always sign your work--not every commit or tag has to
  be signed. If your environment makes it hard to provide a key password
  for you signature, maybe you don't need one.
- If you're running the commands manually, most modern operating systems
  work with GPG to prompt for a password only occasionally. For example,
  MacOS has the ability to integrate GPG passwords in its Keychain,
  which means that you only get prompted for your password once (in a
  while). Similar tools and configurations exist for Windows and Linux
  workstations.
- You can use a key that is not protected with a password. **Be
  careful** using this option, as it may create vulnerabilities in your
  keyring; you should understand the security implications before
  creating an unprotected key.
