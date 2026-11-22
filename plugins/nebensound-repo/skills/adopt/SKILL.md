---
name: adopt
description: Switch a repository to Nebensound's central pipeline and bring it in line with phase 0, ending with one pull request that lists the decisions asked. Use when the user asks to switch a repository to Nebensound/pipeline.
disable-model-invocation: true
---

# Adopt the central pipeline

Switch the repository in the current working directory to the central pipeline
of `Nebensound/pipeline`. The run ends with one pull request whose
`nebensound / Declare` (phase 0) passes, with every decision you asked for
listed in its description.

What this skill never does:

- merge anything or push to the default branch;
- create, merge into or delete a pre-branch, a branch that gathers several
  pull requests before it lands in `main`;
- change a repository setting, a ruleset or a repository property;
- create a tag or a release, or rewrite history;
- release anything. Every merge into `main` is a full release
  (`docs/declaration.md#releases` at the pin), and the merge is the owner's.

## What exists and what is still coming

Issue numbers below are those of `Nebensound/pipeline`. Use a command, field or
finding only once the tool at the pin knows it. When a step needs something that
has not arrived, say which issue brings it and stop that step. Never imitate a
missing command by hand.

| What                                                                                                                                                                  | State                                                  |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| `nebensound-repo check` with `--name`, `--default-branch` and `--visibility`                                                                                          | exists                                                 |
| The fields of `.nebensound/repo.toml`, among them `audience`, `minimum-version` and `feat-types`, and the findings of phase 0, all in `docs/declaration.md`           | exist                                                  |
| The conformance warnings `nonconforming-name`, `nonconforming-default-branch` and `missing-minimum-file`, and the errors `missing-audience` and `mismatched-audience` | exist                                                  |
| The versions, from the history alone; every merge into `main` a full release                                                                                          | exist                                                  |
| Pre-releases                                                                                                                                                          | not yet: no version the history gives is a pre-release |
| `nebensound-repo init`, its `TODO(init)` markers, the finding `check` gives for a remaining marker, and the list of what needs judgement that `init` ends with        | #26, not yet                                           |
| A documented way to get the tool, and a tool that warns when it is older than the pin                                                                                 | #94, not yet; until then step 2                        |
| A release of the pipeline to pin                                                                                                                                      | none yet: pin a full commit SHA of `main`              |

When one of these lands, the reference at the pin describes it: read
`README.md`, `docs/declaration.md`, `docs/calling.md`,
`docs/repository-settings.md` and `tools/nebensound-repo/README.md` at the
pinned commit, `https://github.com/Nebensound/pipeline/blob/<SHA>/<path>`,
before you rely on a detail.

## Rules for the whole run

- **Ask one question at a time.** Give the options, the value the tool
  recognised as the default, and your recommendation. Never ask several
  questions in one message.
- **Settle only what is certain.** You decide alone only what `init` or phase 0
  states as certain, and the judgements of language and naming (step 9).
  Everything else is the user's answer.
- **Always ask** about these five, even where `init` offers a value:
  publication, audience, `minimum-version`, components, and the reason of
  every exclusion.
- **The pull request fixes only what phase 0 needs.** Every other piece of work
  becomes an issue of its own in the adopted repository (step 9), linked from
  the pull request. A list in the description alone is not enough.
- **Confirm before you remove.** No existing file is deleted or overwritten
  without one confirmation for that file.
- **Write English**: files, commit messages, the pull request, issues.
- **Mind the visibility.** In a public repository, write nothing that names a
  private repository, an internal host or an internal document.

## 1. Look at the repository

Read only; change nothing yet.

```sh
gh repo view --json nameWithOwner,visibility,isArchived,isFork,defaultBranchRef
git status --short
git fetch origin --tags
```

Stop and say why when the directory is not a Git checkout, the working tree is
not clean, the repository is archived, it is a fork, or it is
`Nebensound/pipeline` itself. A fork belongs to someone else's project, so the
caller, the declaration and the deleted workflows would land in that project's
tree; `docs/repository-settings.md` leaves forks out for the same reason.

Collect, for the questions to come:

- **The old standard.** Every workflow under `.github/workflows/` whose job
  calls a reusable workflow outside `Nebensound/pipeline`
  (`uses: <owner>/<repo>/.github/workflows/<file>@<ref>` with another
  `<owner>/<repo>`), and every other workflow there.
- **An existing declaration** at `.nebensound/repo.toml`.
- **The highest version ever given out per unit**, pre-releases included: tags
  that read as versions (`v1.4.3`, `<component>-v1.4.3`, `…-pre.2`), GitHub
  releases (`gh release list`), versions the manifests carry or a registry
  published. Tags are told apart by their prefix: `v…` belongs to the
  repository's own unit, `<component>-v…` to that component. Keep one highest
  version for each prefix, not one for the whole repository.
- **Open issues and milestones** (`gh issue list`,
  `gh api repos/<owner>/<repo>/milestones`), for the follow-up issues.

## 2. Get the tool at the pin

Provisional until #94 lands: then take the tool the way its README says. The
tool and the pin are always the same commit.

1. Take the current tip of `main` as the pin, the full 40-character SHA, and
   its commit date as `git log -1 --format=%cs <SHA>` prints it (`YYYY-MM-DD`,
   in the commit's own time zone, as `docs/calling.md` asks), read from a
   shallow fetch into a folder of its own outside the repository:

   ```sh
   git ls-remote https://github.com/Nebensound/pipeline refs/heads/main
   git init -q <folder>/pin
   git -C <folder>/pin fetch -q --depth=1 https://github.com/Nebensound/pipeline <SHA>
   git -C <folder>/pin log -1 --format=%cs FETCH_HEAD
   ```

2. Install the tool at exactly that commit into a folder of its own outside the
   repository, so that no other copy of another version is used:

   ```sh
   cargo install --locked --git https://github.com/Nebensound/pipeline \
     --rev <SHA> nebensound-repo --root <folder>/<SHA>
   ```

   Call it as `<folder>/<SHA>/bin/nebensound-repo`, called `<tool>` below.
   The build needs a Rust toolchain at least as new as the tool's
   `rust-version`. When the build fails on an older one, say so and let the
   user update the toolchain; do not change it yourself.

## 3. Start the branch

```sh
git switch -c ci/adopt-central-pipeline origin/<default branch>
```

## 4. Run `init`

`init` comes with #26. Run it without a terminal, so that it asks nothing:

```sh
<tool> -C . init </dev/null
```

When the tool answers that `init` is an unknown command (exit code `2`), stop
here: tell the user that the tool at this pin has no `init` yet, that #26 brings
it, and write nothing by hand.

Otherwise read its report: the files it wrote, what it recognised, every
`TODO(init)` marker, the old workflows the caller replaces, the next step, and
the list of what needs judgement. A second run changes nothing.

Check the caller it wrote, `.github/workflows/nebensound.yml`, against
`README.md#calling-the-pipeline` and `docs/calling.md` at the pin. It is the
caller the README shows there, apart from its name: today the triggers
`pull_request` and `push` to `main`, `permissions: contents: read` for the
workflow and the job, one job, no inputs, and the reference pinned to the SHA
of step 2 with the `YYYY-MM-DD` date of step 2 as a comment behind it. Where it
differs from the README, the README at the pin is right. A pin that differs
from step 2 is set to it, since the tool and the pin are the same commit.

## 5. Replace the old standard

`init` writes only where a file is missing and deletes nothing. For what it
leaves alone, each after one confirmation, all in this pull request:

- **An existing caller** at `.github/workflows/nebensound.yml` that calls a
  reusable workflow outside `Nebensound/pipeline`: replace it with `init`'s
  template. Remove the old file, run `init` again so that it writes the
  template, and check the pin as in step 4.
- **The workflows `init` lists as replaced**: delete each. Show the user the
  file, what it ran, and what replaces it.
- **Any other workflow** stays. Where it runs a check of this repository alone,
  that check belongs in the repository's xtask (`README.md#a-repositorys-own-checks`
  at the pin): a follow-up issue, not this pull request.
- **An existing, older declaration** stays and is fixed in step 7 until `check`
  passes.

## 6. Resolve every `TODO(init)`

Go through the markers one by one. For each, either settle it, because `init`
or phase 0 states the answer as certain, or ask the user. Edit
`.nebensound/repo.toml` by hand: replace the marker with the answer and keep the
`#:schema` line and the header `init` wrote.

Always asked, one question each:

| Question                          | Field                               | Default to offer                                                                                                                                                                                                                                        |
| --------------------------------- | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Is anything published, and where? | `publish` of each unit's `pipeline` | `false`. A public target is chosen only by an explicit answer and is never the default.                                                                                                                                                                 |
| For whom is it meant?             | `audience` in `[repo]` (required)   | Suggest from the visibility on GitHub: `public` for a public repository, `internal` or `customer` for a private one. Never write it silently.                                                                                                           |
| From which version on?            | `minimum-version`                   | Per unit, from the highest version of its own prefix (step 1): the next patch above a release (`1.4.3` gives `1.4.4`), the release itself above a pre-release (`1.5.0-pre.2` gives `1.5.0`). Without any old version of that unit, leave the field out. |
| Which parts are components?       | `[component.<name>]`                | The candidates `init` recognised.                                                                                                                                                                                                                       |
| Why is this path excluded?        | `reason` of `[[exclude]]`           | Propose a wording; the user confirms or rewrites it.                                                                                                                                                                                                    |

`minimum-version` is three numbers, `"X.Y.Z"`, with no `v` and no pre-release
part such as `-pre.1`; anything else stops phase 0
(`docs/declaration.md#releases`). It matters here because the history alone
gives the versions: no tag is read, and a unit's count starts from `0.0.0`, so
without the field a unit could be given a version it already had.

## 7. Run `check` until phase 0 passes

Commit the declaration first: the versions are read from the history, and a
change not committed yet reaches them only in part.

```sh
git add -A
git commit -m "ci: switch to the central pipeline"   # later rounds: --amend
<tool> -C . check --name <name> --default-branch <default branch> \
  --visibility <visibility>
```

`<visibility>` is the visibility from step 1 in lowercase (`public`, `private`
or `internal`), as the job `nebensound / Declare` passes it; with it, `check`
holds the declared `audience` against it.

Repeat until `check` reports no error and no remaining `TODO(init)`.

- **An error**: its entry in `docs/declaration.md#findings` at the pin names the
  way out. Take it alone where it is certain; where it is a choice from the
  table of step 6 (the audience, `minimum-version`, a component, an exclusion
  and its reason), ask. For `mismatched-audience`, ask whether the value or the
  visibility is wrong. A wrong value you correct; a wrong visibility is a
  repository setting: stop, and tell the user that changing it is an owner
  step. For `conflicting-tag`, an old tag at or above the unit's version, the
  finding names the `minimum-version` that passes it: offer that value when
  you ask again.
- **A warning about a statement of the declaration** (`pointless-exclusion`,
  `pointless-inclusion`, `pointless-suppression`, `pointless-watch`,
  `unwatched-manifest`, `childless-generator`,
  `release-field-without-version`): fix it, since this pull request writes the
  declaration.
- **Any other warning** (`nonconforming-name`, `nonconforming-default-branch`,
  `missing-minimum-file`, `missing-lockfile`): leave the repository as it is. A
  rename or a new default branch is an owner step; the rest becomes a
  follow-up issue.
- **`unconventional-commit`** needs no work, since history is not rewritten for
  it: name the commits in the pull request description as a note (they count
  as patches) and write no issue for it.

## 8. Read the repository settings

Read every setting that `docs/repository-settings.md` at the pin lists under
"Every repository of the organization". Today these are the merge methods and
the titles and messages of the commits they leave, which decide the version a
merge into `main` gets, and the deletion of head branches after merge:

```sh
gh api repos/<owner>/<repo> \
  --jq '{allow_squash_merge, squash_merge_commit_title,
         squash_merge_commit_message, allow_merge_commit,
         merge_commit_title, merge_commit_message, delete_branch_on_merge}'
# expected: allow_squash_merge true, squash_merge_commit_title "PR_TITLE",
#           squash_merge_commit_message "PR_BODY", allow_merge_commit true,
#           merge_commit_title "PR_TITLE", merge_commit_message "PR_BODY",
#           delete_branch_on_merge true
```

Where the document at the pin lists other settings or other values, the
document is right. GitHub leaves these fields out for an account without
enough rights on the repository. A field that comes back `null` could not be
read: say so in the pull request description, as a setting the owner checks,
not as a difference. A value that differs from the expected one is an owner
step for the pull request description, with the call the document gives to set
them for one repository. Never run that call.

## 9. Open the pull request and write the follow-up issues

1. Push the branch and open the pull request against the default branch, titled
   `ci: switch to the central pipeline`, with a description that holds:
   - **What changed**: the files written, replaced and deleted.
   - **Decisions asked**: every question of steps 5 to 7 with the answer given.
   - **Settled without asking**: what `init` or phase 0 stated as certain, and
     the judgements below.
   - **Phase 0**: the summary line of the last local `check`, and the commits
     `unconventional-commit` names, as a note that they count as patches.
   - **Owner steps**: settings that differ or could not be read, a rename, and
     a new default branch.
   - **The pin**: the SHA and its date; raised by hand until the pipeline has
     a release.
   - **Follow-up issues**: links, added in step 3 of this list.
2. Wait for the run (`gh pr checks <number> --watch`). `nebensound / Declare`
   has to pass; when it fails, fix the branch and push again. What
   `nebensound / Check` or `nebensound / Build` find is not fixed here: it
   becomes follow-up issues. The step `Not written yet` fails for a check or
   build the pipeline has no building block for; that gap is the pipeline's,
   so name it in the description instead of writing an issue for it.
3. Write the follow-up issues in the adopted repository. Sources: the findings
   of `Check` and `Build`; the warnings left in step 7 apart
   from `unconventional-commit`; the workflows kept in step 5; and your
   judgement of the list `init` ends with, which names what
   only a person or an assistant can tell: the language of comments and
   messages, naming, documentation. Judge language and naming yourself without
   asking, and the other points where the case is simple; put the rest to the
   user. Fix none of them here.

   Each issue is written so that one agent can do it in one pull request: what
   is wrong, where, and when it is done. Search for an open issue on the same
   thing first and extend that instead. Assign a fitting open milestone where
   one exists. Show the user the list of titles, create the issues after one
   confirmation, and link every one from the pull request description.

Finish by telling the user the link of the pull request, the result of
`nebensound / Declare`, the decisions asked, the owner steps and the follow-up
issues. The pull request is theirs to review and merge. Its merge into `main`
is a full release of every unit that carries a version, at the version
`nebensound / Version` shows for the pull request without `-pr.<number>`, as
long as its title and description stay as they are; nothing is tagged or
published yet.
