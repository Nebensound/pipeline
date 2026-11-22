# Repository settings

Callers pin an exact version of this repository, so a tag that can be moved or
a release that can be changed would change what every caller runs. These are
the settings of `Nebensound/pipeline` that protect it. Each entry gives the
setting, why it is set, and the `gh api` call that reads it back.
The last part lists the settings that every repository of the organization
carries, with the calls that read them back across the organization.

Organization rulesets and the repository properties are not repository
settings: they are described, as files that can be applied, in
[the organization's rulesets and properties](organization/README.md). Which
repository properties remain, and why, is summed up under
[Repository properties](#repository-properties). Organization secrets get
their own document with the issue that introduces them.

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
- Merge commits are allowed because a pre-branch, a branch that gathers
  several pull requests, lands in `main` by merge commit. The merge commit
  takes the pull request's title and description as well; those settings are
  carried by every repository and listed under
  [Every repository of the organization](#every-repository-of-the-organization).
  Until the owner has applied them, the read-back below fails on
  `merge_commit_title` and `merge_commit_message`.
  The ruleset of the default branch allows squash and merge commit once the
  owner has applied it (see
  [`Grundschutz Standard-Branch`](organization/README.md#grundschutz-standard-branch)).
- Rebase is off: it would put a pull request's individual commits on `main`,
  and those are not required to be Conventional Commits.
- Head branches are deleted after merge so that no stale branch lingers.

```sh
gh api repos/Nebensound/pipeline \
  --jq '{allow_squash_merge, allow_merge_commit, allow_rebase_merge,
         squash_merge_commit_title, squash_merge_commit_message,
         merge_commit_title, merge_commit_message,
         delete_branch_on_merge}'
# expected: allow_squash_merge true, allow_merge_commit true,
#           allow_rebase_merge false, squash_merge_commit_title "PR_TITLE",
#           squash_merge_commit_message "PR_BODY", merge_commit_title "PR_TITLE",
#           merge_commit_message "PR_BODY", delete_branch_on_merge true
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

- **Squash and merge commit are allowed, and both take the pull request's
  title and description** (`allow_squash_merge` and `allow_merge_commit`
  `true`; `squash_merge_commit_title` and `merge_commit_title` `PR_TITLE`;
  `squash_merge_commit_message` and `merge_commit_message` `PR_BODY`). A pull
  request enters the default branch as a squash commit, and a pre-branch lands
  there as a merge commit; the title of the landing commit decides the
  version. Without `merge_commit_title` set to `PR_TITLE`, GitHub titles a
  merge commit "Merge pull request #N from …", which is no Conventional
  Commit. With these settings a merge commit carries the same title and
  message a squash commit of the same pull request would carry. Which merge
  methods the default branch accepts is set by its ruleset
  ([`Grundschutz Standard-Branch`](organization/README.md#grundschutz-standard-branch));
  GitHub offers only the methods that both the settings and the rulesets
  allow.
- **Head branches are deleted after merge.** A merged pull request leaves no
  stale branch behind, and a pre-branch is deleted once it has landed in the
  default branch. The setting never deletes the default branch or a protected
  branch, and a branch that no pull request was merged from is not touched
  either.
- **Forks are left out.** A fork belongs to someone else's project, and its
  settings follow that project's habits. The read-back call lists no fork, and
  archived repositories are left out as well because nobody merges there.

Read the state of every active repository that is not a fork, one line per
repository:

```sh
gh repo list Nebensound --no-archived --source --limit 1000 \
  --json nameWithOwner --jq '.[].nameWithOwner' |
  while read -r repo; do
    gh api "repos/$repo" \
      --jq '[.full_name, .allow_squash_merge, .squash_merge_commit_title,
             .squash_merge_commit_message, .allow_merge_commit,
             .merge_commit_title, .merge_commit_message,
             .delete_branch_on_merge] | @tsv'
  done
# expected on every line, after the name:
#   true  PR_TITLE  PR_BODY  true  PR_TITLE  PR_BODY  true
```

List only the repositories that still need a setting (empty output means the
organization is in line):

```sh
gh repo list Nebensound --no-archived --source --limit 1000 \
  --json nameWithOwner --jq '.[].nameWithOwner' |
  while read -r repo; do
    gh api "repos/$repo" \
      --jq 'select(.allow_squash_merge != true
                   or .squash_merge_commit_title != "PR_TITLE"
                   or .squash_merge_commit_message != "PR_BODY"
                   or .allow_merge_commit != true
                   or .merge_commit_title != "PR_TITLE"
                   or .merge_commit_message != "PR_BODY"
                   or .delete_branch_on_merge != true) | .full_name'
  done
```

Set them for one repository:

```sh
gh api -X PATCH repos/Nebensound/<repository> \
  -F allow_squash_merge=true \
  -f squash_merge_commit_title=PR_TITLE \
  -f squash_merge_commit_message=PR_BODY \
  -F allow_merge_commit=true \
  -f merge_commit_title=PR_TITLE \
  -f merge_commit_message=PR_BODY \
  -F delete_branch_on_merge=true
```

## Repository properties

A fact about a repository lives in one place, so that none is kept twice (see
[Where a fact lives](declaration.md#where-a-fact-lives)). A repository property
on GitHub holds only what GitHub has to decide without reading the
repository's tree, which is what an organization ruleset targets. Two
properties remain for that, described in
[the organization's properties](organization/README.md#properties):

- **`lifecycle`**: whether the repository is gated at all.
- **`pipeline`**: whether the repository has switched to the central pipeline.

For whom a repository is meant is declared in its `.nebensound/repo.toml` as
`audience` (see [Audience](declaration.md#audience)), not as a property. A
property that repeats a fact of the declaration would be a copy for
administration, and no step of the pipeline reads a property to decide
anything.

The properties `language` and `kind` repeat what the declaration and the files
already say, and no ruleset targets them. The organization owner removes them,
with the calls in
[the owner's checklist there](organization/README.md#checklist-for-the-organization-owner).
Read back which properties the organization still defines (`lifecycle` and
`pipeline` are expected once they are removed):

```sh
gh api orgs/Nebensound/properties/schema --jq '.[].property_name'
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

- The organization rulesets and the switch of this repository to them, with
  the calls, are listed in
  [the owner's checklist there](organization/README.md#checklist-for-the-organization-owner).
