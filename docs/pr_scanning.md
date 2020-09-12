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
