# Organization rulesets and properties

The release model relies on rules that GitHub enforces: what may enter the
default branch, which checks it needs, and that a version tag stays where it
was set.
This directory holds those rules as files that can be applied, each a JSON body
for GitHub's rulesets API, and says for each what it protects against and what
it does not.

Nothing in the pipeline applies them. Rulesets and properties of the
organization need organization owner rights, so applying them is the owner's
step, listed in [the checklist](#checklist-for-the-organization-owner) at the
end.

## The files

| File                                                                   | What it is                                                                        | State    |
| ---------------------------------------------------------------------- | --------------------------------------------------------------------------------- | -------- |
| [`version-tags.json`](version-tags.json)                               | Version tags `v*` and `*-v*` are never moved or deleted.                          | to apply |
| [`required-checks.json`](required-checks.json)                         | The default branch needs `Version`, `Check` and `Build`.                          | to apply |
| [`grundschutz-standard-branch.json`](grundschutz-standard-branch.json) | The existing ruleset on the default branch: pull request, squash or merge commit. | to apply |
| [`conditions-central.json`](conditions-central.json)                   | Which repositories the two new rulesets bind.                                     | to apply |
| [`conditions-active.json`](conditions-active.json)                     | Which repositories the existing ruleset binds.                                    | live     |
| [`property-pipeline.json`](property-pipeline.json)                     | The repository property `pipeline`.                                               | live     |

A ruleset file holds no organization condition. It is the body that the
rulesets API of a single repository accepts, and it was applied in that form to
a test repository and read back unchanged; see [Evidence](#evidence). The
condition that selects repositories is a fragment of its own, which the apply
command merges in, so that what was tested on a repository is what the
organization gets, apart from that one step.

The existing ruleset is described under its live name. Its file differs from
the live ruleset in one parameter: `allowed_merge_methods` holds `merge` beside
`squash`, because a pre-branch lands in the default branch by merge commit (see
[`Grundschutz Standard-Branch`](#grundschutz-standard-branch)). Until the owner
applies the file, the live ruleset allows squash only. A second ruleset of the
organization requests an automatic Copilot review on every branch; the release
model does not rely on it, and it is not described here.

## Which repositories they bind

The two new rulesets hold only for a repository whose property `pipeline` is
`central` and whose property `lifecycle` is neither `sandbox` nor
`archived`. A repository that has not switched yet stays untouched as long as
it carries `legacy`; a new repository and a new fork carry `central` from their
creation (see [Properties](#properties)). The existing ruleset holds for every repository whose
`lifecycle` is neither `sandbox` nor `archived`, whatever its `pipeline`.

Neither of the two has a bypass for anyone, administrators included. The way
out in an emergency is the owner setting the repository's property `pipeline`
to `legacy`: both stop holding for it at once. That is deliberate and visible,
unlike a bypass that nobody sees being used.

## The rulesets

### `version-tags`

On every tag that starts with `v` or contains `-v`, which covers every version
tag: `v*` for the repository and `*-v*` for a component or a generated unit
(the tag names are listed in [the declaration](../declaration.md#tag-names)):
no update and no deletion, for anyone, administrators included. A tag that only
looks like those, such as `vendor-drop`, is held as well.

Tags are records: no version is read from a tag; the versions come from the
history (see [the declaration](../declaration.md#versions)), so moving a tag
would not change a version. What a tag holds is what a caller pins and what the
release stage wrote, and this ruleset keeps both where they were set. Measured
on the test repository: moving `v0.0.1` and `tool-v0.0.1` was refused with
"Cannot update this protected ref", deleting them with "Cannot delete this tag", and a tag
that matches neither pattern was created and deleted freely.

It does not protect against creating a tag. The release stage creates its tags
with the token of the workflow run, and GitHub Actions cannot be put on a
bypass list, so creation cannot be restricted while the pipeline tags. A person
or a workflow with write access can therefore create a tag that reads as a
newer version on any commit, and a tool that raises a caller's pin reads tags.
Until a repository restricts that creation (#102 does it for this repository),
the protection is the review of the pull request that raises a caller's pin.

### `required-checks`

On the default branch: `nebensound / Version`, `nebensound / Check` and
`nebensound / Build` must have passed, reported by GitHub Actions, with no
bypass, administrators included. Why exactly these three names is in
[the phases](../phases.md); "branch must be up to date" stays off.

It protects against a change entering the default branch without the pipeline
having versioned, checked and built it. Measured on the test repository: an
administrator, who bypasses the existing ruleset, could not merge a pull
request before its checks had passed ("3 of 3 required status checks are
expected").

It is not enforced when the branch is created (`do_not_enforce_on_create`), so
that a new repository, which carries `central` from its creation, can be
created at all. Measured on an empty test repository (see
[Evidence](#evidence)): the first push, which creates the default branch, went
through, and a pull request afterwards could not be merged before the three
checks had passed. The repository's first pull request, the one that adds the
caller, reports them itself, but it can merge only once they pass (see
[Properties](#properties) for a repository the pipeline cannot pass yet). That
measurement was taken with an administrating account, which `required-checks`
does not exempt but the two `Grundschutz Standard-Branch` rulesets do (role
Admin, bypass `always`); a creator who is not an administrator was not
measured, and [#119](https://github.com/Nebensound/pipeline/issues/119)
tracks it.

It does not protect against a workflow of the repository reporting the names
itself, which the next section explains, nor against what the first push into
an empty repository brings.

### `Grundschutz Standard-Branch`

The existing ruleset on the default branch: a change arrives only through a
pull request, squash and merge commit are the merge methods, every review
thread is resolved, a stale review is dismissed, no approval is required, the
branch cannot be deleted, and no push may rewrite it. The repository role Admin
may bypass it, always. The live ruleset allows squash only; the file adds the
merge commit, and the owner applies it (see
[the checklist](#checklist-for-the-organization-owner)).

The release model relies on its merge methods, because the title of the commit
that lands on the default branch decides the version. A pull request becomes
one squash commit with the pull request's title as a Conventional Commit. A
pre-branch, a branch that gathers several pull requests, lands as one merge
commit, which keeps the commits of the pre-branch in the history. A merge
commit carries the title of its pull request only when the repository sets
`merge_commit_title` to `PR_TITLE`; that and the other merge settings every
repository carries are in
[the repository settings](../repository-settings.md#every-repository-of-the-organization).
A merge commit on the line of the default branch counts like a squash commit
with the same changes and message (see
[How a version is resolved](../declaration.md#how-a-version-is-resolved)).
Rebase stays excluded: it would put the individual commits of a pull request on
the default branch, and those are not required to be Conventional Commits.

It protects against a direct push, a force push and a rebase merge into the
default branch, for everyone below Admin. It does not hold for an
administrator: measured on the test repository while the ruleset allowed squash
only, an administrator merged a pull request into the default branch with a
merge commit. `required-checks` still held for that administrator, since it has
no bypass.

## Where the checks come from

`required-checks` counts the three names only when GitHub Actions reports
them: the rule pins the integration (`integration_id` 15368, the app
`github-actions`). A status that a person or another app sets under the same
name does not count.

The gap that remains: every workflow of the repository runs as GitHub Actions,
including the workflow of the pull request being judged. A pull request that
changes the caller file, or adds a workflow whose jobs are named like the
pipeline's, reports the three names itself and satisfies the ruleset.
Measured on the test repository: a stand-in workflow that only printed a line
in jobs named `Version`, `Check` and `Build` satisfied it. So the review of a
pull request that touches `.github/workflows/` decides whether the checks mean
anything.

## Properties

A repository property exists for one reason: a ruleset of the organization
targets it, and GitHub has to decide without reading the repository's tree.
Two properties remain for that:

- **`pipeline`**: whether the repository has switched to the central pipeline.
  It is live as [`property-pipeline.json`](property-pipeline.json) describes
  it: single select, required, values `central` and `legacy`, default
  `central`, editable by the organization, and the live description "central:
  the repository calls the central pipeline and the three checks are required.
  legacy: not switched yet." On 2026-11-20, 159 repositories carried it: 3 with
  `central` and 156 with `legacy`.
- **`lifecycle`**: whether the repository is gated at all. Single select,
  required, values `active`, `sandbox` and `archived`, default `active`. Every
  ruleset of this directory leaves `sandbox` and `archived` out.

For whom a repository is meant is declared in the repository as `audience`
(see [the declaration](../declaration.md#audience)), not as a property. The
properties `language` and `kind` repeat what the declaration and the files
already say, and no ruleset targets them. Once the first repository has
switched, the owner deletes both properties; that removes them from every
repository of the organization at once, the `legacy` ones included.

The default of `pipeline` is `central`, so a new repository is bound from the
start: the two new rulesets hold for it before it has a caller. It can still be
created, because `required-checks` is not enforced on creation (measured, see
[Evidence](#evidence)), and its first pull
request adds the caller, which reports the three checks; it merges once they
have passed.

What follows from it:

- A new repository can merge into its default branch only once the pipeline
  passes for it. One that the pipeline cannot pass yet stays blocked until the
  owner sets it to `legacy`: a check the pipeline has not written yet fails by
  name, and a kind of file that no check covers is `language-without-check`
  (see [the declaration](../declaration.md#language-without-check)).
- A fork of someone else's project carries `central` as well and is bound by
  the two new rulesets until the owner sets it to `legacy`. A ruleset condition
  cannot tell a fork apart.
- The rulesets see only properties, not whether a repository really calls the
  pipeline. A daily pass over all repositories of the organization (#108) will
  find what they cannot see, such as a repository bound to the central pipeline
  that does not call it. A fork that still carries `central` is found only by
  the owner's step in [the checklist](#checklist-for-the-organization-owner).

### The one-time step

The step sets every repository whose default branch does not report the three
checks to `legacy`. It is defined by that condition, not by a list of names, and
it runs once, before the rulesets are applied. Run again later, it would also
set a new repository that has not merged its caller yet to `legacy`, against
the default `central`; an empty repository is listed as well, because its
check-run call fails and counts as no check. On 2026-11-20 it would change 2 of
the 3 repositories that carry `central`.

List the repositories that carry `central` although their default branch does
not report the three checks (empty output means the organization is in line):

```sh
org=Nebensound
gh api --paginate "orgs/$org/properties/values" \
  --jq '.[] | select(any(.properties[];
                .property_name == "pipeline" and .value == "central"))
        | .repository_name' |
  while read -r repo; do
    branch="$(gh api "repos/$org/$repo" --jq .default_branch)"
    found=0
    for job in Version Check Build; do
      count="$(gh api "repos/$org/$repo/commits/$branch/check-runs?app_id=15368&check_name=nebensound%20%2F%20$job" \
        --jq .total_count 2>/dev/null)"
      [ "${count:-0}" -gt 0 ] && found=$((found + 1))
    done
    [ "$found" -eq 3 ] || echo "$repo"
  done
```

Set them to `legacy`, at most 30 names per call:

```sh
gh api -X PATCH orgs/Nebensound/properties/values --input - <<'EOF'
{
  "repository_names": ["<repository>", "<repository>"],
  "properties": [{ "property_name": "pipeline", "value": "legacy" }]
}
EOF
```

## Evidence

Every ruleset file was applied at repository level to a temporary public test
repository with `gh api -X POST repos/Nebensound/<test repository>/rulesets
--input <file>` and read back, which gave what is stated in the sections above.
The measurements below were taken on a second, empty test repository, with the
account of an organization administrator. Of the rulesets applied there, only
`required-checks` has no bypass actor; both `Grundschutz Standard-Branch`
rulesets, the organization's and the repository-level copy, let the role Admin
bypass them.

- **Applied and read back.** `required-checks.json` in its present form, with
  `do_not_enforce_on_create` set to `true`, and
  `grundschutz-standard-branch.json`, each accepted by the API. The read-back
  of each, compared with its file as in
  [Applying and reading back](#applying-and-reading-back), was identical. The
  rules of the default branch then listed `required_status_checks` from the
  repository-level `required-checks`, and `deletion`, `non_fast_forward` and
  `pull_request` from both the repository-level copy and the live ruleset of
  the organization of the same name, which binds the repository as well.
- **Half one, the first push.** With both rulesets active, the first push,
  `git push origin main` into the empty repository, created the default branch
  and went through. Its rule suite (`rulesets/rule-suites`) records `pass`, not
  `bypass`, for every rule of the three rulesets that bind the branch:
  `pull_request`, `non_fast_forward` and `deletion` of both
  `Grundschutz Standard-Branch` rulesets, and `required_status_checks` of
  `required-checks`. So nothing was refused and no bypass was used on the
  creation. That shows nothing for a creator who is not an
  administrator, whom the Admin bypass does not cover. Tracked in
  [#119](https://github.com/Nebensound/pipeline/issues/119).
- **Half two, a pull request.** A pull request from a second branch into the
  default branch showed `mergeable: MERGEABLE` and `mergeStateStatus: BLOCKED`.
  A merge by the API, once with `squash` and once with `merge`, and a merge by
  `gh pr merge --squash --admin` all answered HTTP 405 "Repository rule
  violations found: 3 of 3 required status checks are expected." The
  administrator did not get past it, since `required-checks` has no bypass.
- **The merge commit on the default branch.** On 2026-11-22,
  `grundschutz-standard-branch.json` in its present form, with
  `allowed_merge_methods` `["squash", "merge"]`, was applied as a
  repository-level ruleset to a temporary public test repository, accepted by
  the API, read back identical to its file as in
  [Applying and reading back](#applying-and-reading-back), and removed again.
- **Not measured.** That the push is refused with `do_not_enforce_on_create`
  set to `false` was not repeated on the empty repository; a creator who is
  not an administrator was not measured; and a pull request landed by merge
  commit, with the version table of its push run, was not measured (#165).

## Applying and reading back

The calls below run from the root of a checkout of this repository.

**On a test repository**, which is how every ruleset file was tested: post the
file as it is.

```sh
gh api -X POST repos/Nebensound/<test repository>/rulesets \
  --input docs/organization/required-checks.json
```

**In the organization**: merge the condition in and post the result. The two
new rulesets take `conditions-central.json`; the existing one is described by
`conditions-active.json`.

```sh
jq -s '.[0] * {conditions: .[1]}' \
  docs/organization/required-checks.json \
  docs/organization/conditions-central.json |
  gh api -X POST orgs/Nebensound/rulesets --input -
```

To change a ruleset that exists already, such as `Grundschutz Standard-Branch`
with `conditions-active.json`, send the same body with `-X PUT` to
`orgs/Nebensound/rulesets/<id>`.

**Read back** a ruleset of the organization and compare it with its file and
fragment. GitHub returns a few fields the file does not hold (its id, dates and
links) and adds a disabled `dismissal_restriction` to the pull-request rule on
a repository; the comparison leaves those out.

```sh
normalize='{name, target, enforcement, bypass_actors, conditions, rules}
  | del(.rules[].parameters.dismissal_restriction | select(.enabled == false))'
name=required-checks file=required-checks fragment=conditions-central
# existing ruleset: name='Grundschutz Standard-Branch'
#   file=grundschutz-standard-branch fragment=conditions-active
id="$(gh api --paginate orgs/Nebensound/rulesets \
  --jq ".[] | select(.name == \"$name\") | .id")"
diff <(gh api "orgs/Nebensound/rulesets/$id" | jq -S "$normalize") \
  <(jq -s '.[0] * {conditions: .[1]}' \
      "docs/organization/$file.json" "docs/organization/$fragment.json" |
    jq -S "$normalize")
# expected: no output
```

For the existing ruleset, set the three variables as the comment says. On a test
repository, read `repos/Nebensound/<test repository>/rulesets/<id>` and compare
with the file alone. The rules that hold for one branch of one repository, from
every source, are listed by
`gh api repos/Nebensound/<repository>/rules/branches/<branch>`.

**The property**: create or change it, and read it back.

```sh
gh api -X PUT orgs/Nebensound/properties/schema/pipeline \
  --input docs/organization/property-pipeline.json
diff <(gh api orgs/Nebensound/properties/schema/pipeline |
       jq -S 'del(.property_name, .url, .source_type)') \
  <(jq -S . docs/organization/property-pipeline.json)
# expected: no output
```

## The day a repository switches

In this order, so that the required checks never bind a repository that cannot
pass them:

1. **Merge the caller**, pinned to a version of the pipeline that reports all
   three checks, and wait until the push run on the default branch has reported
   them.
2. **Wait until the three checks have passed** on the tip of the default
   branch, not only been reported. If they fail, fix that first: with the
   property set while they fail, nothing can merge into the default branch.
3. **Set the property** `pipeline` to `central`:

   ```sh
   gh api -X PATCH orgs/Nebensound/properties/values --input - <<'EOF'
   {
     "repository_names": ["<repository>"],
     "properties": [{ "property_name": "pipeline", "value": "central" }]
   }
   EOF
   ```

4. **Delete the local ruleset**: a ruleset of the repository itself that the
   organization rulesets now replace, such as one that required checks.

   ```sh
   gh api repos/Nebensound/<repository>/rulesets \
     --jq '.[] | select(.source_type == "Repository") | [.id, .name] | @tsv'
   gh api -X DELETE repos/Nebensound/<repository>/rulesets/<id>
   ```

5. **Read back** the rules of the default branch: the required checks of
   `required-checks` are among them.

   ```sh
   branch="$(gh api repos/Nebensound/<repository> --jq .default_branch)"
   gh api "repos/Nebensound/<repository>/rules/branches/$branch" \
     --jq '.[] | [.type, .ruleset_source_type] | @tsv'
   ```

## Checklist for the organization owner

- Run [the one-time step](#the-one-time-step) once, before applying the
  rulesets, and read it back.
- Apply `version-tags` and `required-checks` with `conditions-central.json`,
  and read each back.
- Apply the changed `grundschutz-standard-branch.json` with
  `conditions-active.json` to the existing ruleset of that name with `-X PUT`,
  and read it back. Before that, set the merge settings of every repository
  (see [the repository settings](../repository-settings.md#every-repository-of-the-organization)),
  so that no merge commit lands with GitHub's default title.
- Switch this repository first, `Nebensound/pipeline`, by
  [the steps above](#the-day-a-repository-switches). Its default branch already
  reports the three checks, and it has no local ruleset.
- Once the first repository has switched, delete the properties `language` and
  `kind`, once for the whole organization (each call removes the property from
  every repository):

  ```sh
  gh api -X DELETE orgs/Nebensound/properties/schema/language
  gh api -X DELETE orgs/Nebensound/properties/schema/kind
  ```

- Set a fork of someone else's project to `legacy` once it is made (see
  [Properties](#properties)).
