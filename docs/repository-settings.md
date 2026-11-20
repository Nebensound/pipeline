# Repository settings

Callers pin an exact version of this repository, so a tag that can be moved or
a release that can be changed would change what every caller runs. These are
the settings of `Nebensound/pipeline` that protect it. Each entry gives the
setting, why it is set, and the `gh api` call that reads it back.
The last part lists the settings that every repository of the organization
carries, with the calls that read them back across the organization.

Organization rulesets, the repository custom property and organization secrets
are not repository settings and are not described here. They get their own
documents with the issues that introduce them.

## Immutable releases

Enabled. Once a release is published, its assets cannot be added to, replaced
or deleted, and its tag cannot be moved or deleted while the release exists.
A pinned version therefore keeps resolving to the same code. The limits: the
title and notes of a release can still be edited, a release can still be
deleted (its tag name can then never be used again), and a tag pushed without
a release is not covered by this setting.

```sh
gh api repos/Nebensound/pipeline/immutable-releases
# expected: {"enabled":true,"enforced_by_owner":false}
```

## Secret scanning and push protection

Both enabled. A secret committed to this public repository is detected, and a
push that contains a supported secret is blocked before it lands.

```sh
gh api repos/Nebensound/pipeline \
  --jq '.security_and_analysis
        | {secret_scanning: .secret_scanning.status,
           push_protection: .secret_scanning_push_protection.status}'
# expected: {"push_protection":"enabled","secret_scanning":"enabled"}
```

## Private vulnerability reporting

Enabled. `SECURITY.md` asks reporters to use it, so it has to be on.

```sh
gh api repos/Nebensound/pipeline/private-vulnerability-reporting
# expected: {"enabled":true}
```

## Merge methods

- Squash is allowed and is how a pull request enters `main`. The squash commit
  takes the pull request's title and description, so the commit on `main` is a
  Conventional Commit without anyone retyping it.
- Merge commits are allowed in the settings because a later branch behind
  `main` takes its changes as merge commits. On `main` a ruleset allows squash
  only.
- Rebase is off: it would put a pull request's individual commits on `main`,
  and those are not required to be Conventional Commits.
- Head branches are deleted after merge so that no stale branch lingers.

```sh
gh api repos/Nebensound/pipeline \
  --jq '{allow_squash_merge, allow_merge_commit, allow_rebase_merge,
         squash_merge_commit_title, squash_merge_commit_message,
         delete_branch_on_merge}'
# expected: allow_squash_merge true, allow_merge_commit true,
#           allow_rebase_merge false, squash_merge_commit_title "PR_TITLE",
#           squash_merge_commit_message "PR_BODY", delete_branch_on_merge true
```

## Workflow runs from forks

A workflow run started from a fork by someone who is not a member or
collaborator of the repository needs a maintainer's approval, every time and
not only for a first contribution. Such a pull request cannot run workflows,
and so cannot spend minutes or probe the runner, before a maintainer has
looked at it.

```sh
gh api \
  repos/Nebensound/pipeline/actions/permissions/fork-pr-contributor-approval
# expected: {"approval_policy":"all_external_contributors"}
```

## Workflow token

The default permissions of the workflow token are read, and workflows cannot
approve or open pull requests (the API calls this
`can_approve_pull_request_reviews`). A workflow or job that needs more asks for
it in its own `permissions` block; one that forgot to say anything gets a
read-only token, and no workflow can count as an approving review. Every
workflow here states its permissions explicitly, so none relies on the default.

```sh
gh api repos/Nebensound/pipeline/actions/permissions/workflow
# expected: {"default_workflow_permissions":"read",
#           "can_approve_pull_request_reviews":false}
# (the API returns it on one line)
```

## Every repository of the organization

The settings above protect this repository. The settings below are carried by
every repository of the organization, because GitHub offers them per
repository only and an organization cannot set them as a default. Nothing in
the pipeline changes repository settings, so a repository created later does
not get them by itself: whoever creates a repository sets them with the call
below, and the read-back call shows any repository that was missed.

- **Head branches are deleted after merge.** A merged pull request leaves no
  stale branch behind. The setting never deletes the default branch or a
  protected branch, and a branch that no pull request was merged from is not
  touched either. A `release` branch therefore relies on its ruleset, not on
  this setting, to stay.
- **Forks are left out.** A fork belongs to someone else's project, and its
  settings follow that project's habits. The read-back call lists no fork, and
  archived repositories are left out as well because nobody merges there.

Read the state of every active repository that is not a fork (one line per
repository, `true` is expected on every line):

```sh
gh repo list Nebensound --no-archived --source --limit 1000 \
  --json nameWithOwner,deleteBranchOnMerge \
  --jq '.[] | [.nameWithOwner, .deleteBranchOnMerge] | @tsv'
# expected: every line ends in "true"
```

List only the repositories that still need the setting (empty output means the
organization is in line):

```sh
gh repo list Nebensound --no-archived --source --limit 1000 \
  --json nameWithOwner,deleteBranchOnMerge \
  --jq '.[] | select(.deleteBranchOnMerge | not) | .nameWithOwner'
```

Set it for one repository:

```sh
gh api -X PATCH repos/Nebensound/<repository> -F delete_branch_on_merge=true
```

## Checklist for the organization owner

Settings that only the organization owner can change, or that the API refused,
are listed here with the exact call (and the response, if refused).

- The same two settings at organization level (default workflow permissions
  read, approval of pull request reviews by workflows off). Only the
  organization owner can change them; the repository setting above does not
  depend on them.

  ```sh
  gh api -X PUT orgs/Nebensound/actions/permissions/workflow \
    -f default_workflow_permissions=read \
    -F can_approve_pull_request_reviews=false
  ```
