# Repository settings

Callers pin an exact version of this repository, so a tag that can be moved or
a release that can be changed would change what every caller runs. These are
the settings of `Nebensound/pipeline` that protect it. Each entry gives the
setting, why it is set, and the `gh api` call that reads it back.

Organization rulesets, the repository custom property, environments and their
secrets are not repository settings in this sense and are described elsewhere.

## Immutable releases

Enabled. A published release and its tag can no longer be changed or deleted,
so a pinned version always resolves to the same code.

```sh
gh api repos/Nebensound/pipeline/immutable-releases
# expected: {"enabled":true,...}
```

## Secret scanning and push protection

Both enabled. A secret committed to this public repository is detected, and a
push that contains a supported secret is blocked before it lands.

```sh
gh api repos/Nebensound/pipeline --jq '.security_and_analysis | {secret_scanning: .secret_scanning.status, push_protection: .secret_scanning_push_protection.status}'
# expected: {"push_protection":"enabled","secret_scanning":"enabled"}
```

## Private vulnerability reporting

Enabled. `SECURITY.md` asks reporters to use it, so it has to be on.

```sh
gh api repos/Nebensound/pipeline/private-vulnerability-reporting
# expected: {"enabled":true}
```

## Merge methods

Squash and merge commit are allowed, rebase is off. A squash commit takes the
pull request's title and description, which keeps the history in Conventional
Commit form. Head branches are deleted after merge so no stale branch lingers.

```sh
gh api repos/Nebensound/pipeline --jq '{allow_squash_merge, allow_merge_commit, allow_rebase_merge, squash_merge_commit_title, squash_merge_commit_message, delete_branch_on_merge}'
# expected: allow_squash_merge true, allow_merge_commit true,
#           allow_rebase_merge false, squash_merge_commit_title "PR_TITLE",
#           squash_merge_commit_message "PR_BODY", delete_branch_on_merge true
```

## Workflow runs from forks

Workflow runs from forks by outside contributors require approval, every
time. A fork's pull request cannot run workflows, and so cannot spend minutes
or probe the runner, before a maintainer has looked at it.

```sh
gh api repos/Nebensound/pipeline/actions/permissions/fork-pr-contributor-approval
# expected: {"approval_policy":"all_external_contributors"}
```

## Checklist for the organization owner

Settings the API refused are listed here with the exact call and response.
None so far.
