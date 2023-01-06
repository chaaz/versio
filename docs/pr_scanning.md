# Versio PR Scanning

While using commits in Git is helpful to determine the general size and
complexity of a release, they don't always tell the whole story. Lots of
minor or trivial commits are often collected in a single Pull Request
(PR) to implement a story-level feature. Additionally, sometimes PRs are
"squashed" onto a release branch, generating a single commit that elides
the per-project size information otherwise found in the individual
commits.

If your repository uses GitHub as its remote, then Versio will use the
GitHub v4 GraphQL API to extract information about the PRs and
associated commits that went into the release changes. If Versio creates
or updates a changelog, it will group commits into whatever PRs can be
found.

If a PR has been squashed onto the branch, Versio will "unsquash" that
PR for changelog and increment sizing purposes. Unsquashing is only
possible if the PR's commits still exist on the Git remote: if the
branch has been deleted (which is typical for squashes), then the
commits may have been garbage collected and unavailable for examination.
In this case, Versio will make some guesses, but might get some sizing
or grouping wrong. If unsquashing is important, don't delete PR branches
from GitHub until after they've been part of a release.

PR scanning works perfectly with [version chains](./chains.md), allowing
the correct version of all interdependent projects to be selected from
an unsquashed PR.

## Unsquash Example

As an example of why unsquashing might be useful, consider this squashed
PR commit:

```
commit 12345abcde12345abcde12345abcde12345abcde
Author: Me <myself@mycompany.com>
Date:   Mon Jan 2 12:00:30 2023 -0700

    feat!: remove bozo API (PR #121 squashed)

 README.md
 app/my_app/src/apis.js
 app/my_app/docs/apis.md
 app/internal_reader/src/bozo.js
 lib/common/src/proc.js
```

If you naively release this in a
[monorepo](https://en.wikipedia.org/wiki/Monorepo), it would increment
the major versions of the "my_app", "internal_reader", and "common"
projects (because the `!` on the commit summary indicates a breaking
change, according to the [commit
convention](https://www.conventionalcommits.org/)). But what if the
original commits in the PR looked like this?

```
commit 11111aaaaa11111aaaaa11111aaaaa11111aaaaa
Author: Me <myself@mycompany.com>
Date:   Mon Dec 12 12:00:30 2022 -0700

    fix: update processing

    Remove unncessary circular logic. This needs
    to be done before the bozo API can be removed.

 lib/common/src/proc.js
```

```
commit 22222bbbbb22222bbbbb22222bbbbb22222bbbbb
Author: Me <myself@mycompany.com>
Date:   Tue Dec 13 12:00:30 2022 -0700

    feat: don't read bozo

    The bozo API will be removed. Read the kozo
    API instead.

 app/internal_reader/src/bozo.js
```

```
commit 33333ccccc33333ccccc33333ccccc33333ccccc
Author: Me <myself@mycompany.com>
Date:   Wed Dec 14 12:00:30 2022 -0700

    feat!: remove bozo API

    Finally, we can remove the bozo API.

 README.md
 app/my_app/src/apis.js
 app/my_app/docs/apis.md
```

When versio performs a release, it will increment the major version of
the "my_app" project, the minor version of "internal_reader", and the
patch version of "common". This more accurately reflects how those
projects have evolved. This especially impacts other users of the
"common" lib, who will be able to upgrade to the latest version without
worrying about a breaking change.

Additionally, Versio will list the original commits in the changelogs of
their respective projects, rather than squash commit, which makes it
easier to track why a particular version number was chosen.
