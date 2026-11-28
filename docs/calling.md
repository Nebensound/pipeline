# Calling the workflow

What a repository needs to know when it starts calling `nebensound.yml`, and
that the workflow's reference does not state elsewhere. The caller file itself
is in [the README](../README.md#calling-the-pipeline); what runs and under which name is
in [the phases](phases.md).

## Pinning

The reference in the caller file is pinned to an exact version: the tag of a
release of this repository, `vX.Y.Z`. Every merge to `main` is a release, as
`v0.0.N` until `v0.1.0`. A release cannot be changed once it is published, and
the organization's ruleset [`version-tags`](organization/README.md#version-tags)
keeps its tag from being moved or deleted, so the tag names one commit for good:

```yaml
jobs:
  nebensound:
    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@v0.0.83
    permissions:
      contents: write
      pull-requests: read
      id-token: write
      packages: write
      attestations: write
      deployments: write
```

Every job of the run fetches the binary of the release on the commit the tag
stands on and verifies it (see [the tool](tool.md#layout)). A branch name or a
shortened SHA is not a pin. Dependabot's entry for `github-actions` keeps the
pin out of its group, so that every raise of it is a pull request of its own
(see [the Dependabot policy](register.md#dependabot-policy)).

`nebensound-repo init` writes the tag of the release it belongs to: a binary
from the archive of a release prints that release's version (see
[The tool on a computer](../README.md#the-tool-on-a-computer)). A tool built
from source prints `0.0.0`, and the build of a pull request or a pre-release
carries more than three numbers; neither belongs to a release, so `init` writes
in place of the tag a reference that no branch, tag or commit can have, with a
`TODO(init)` marker as the comment of the line (see <!-- nebensound:allow(todo) names the marker init writes -->
[the tool](tool.md#switching-a-repository-to-the-pipeline)). The run fails on it
at once, and `check` reports it as
[`unresolved-init-marker`](declaration.md#unresolved-init-marker).

A caller pinned to the full 40-character SHA of a commit keeps working as it is:
its jobs fetch the binary of the release on that commit in the same way, and
build the tool from source where the commit carries no release. Nothing changes
for it until its pin is raised; raised to the tag of the same release, it runs
the same.

## When it runs

The caller file starts the workflow on a pull request when it is opened,
synchronized, reopened or edited, and on a push to `main`, `pre` or `pre-*`:

```yaml
on:
  pull_request:
    types:
      - opened
      - synchronize
      - reopened
      - edited
  push:
    branches:
      - main
      - pre
      - pre-*
```

`edited` is there for two reasons. The version a pull request shows is read
from its title and description, so a run follows every change of them. And
when a pre-branch is deleted at its landing, GitHub moves every open pull
request into it onto `main` as an edit of its base, which starts no run without
`edited`. The branches `pre` and `pre-*` are named from the start, so that the
caller file does not change when the versions and pre-releases of pre-branches
are built. Until then a push to one is checked as a push to `main` is, shows no
version, builds nothing and publishes nothing (see
[the phases](phases.md#pull-request-and-push)).

## Permissions

The caller file keeps `contents: read` for the workflow and grants its job
`contents: write`, `pull-requests: read`, `id-token: write`,
`packages: write`, `attestations: write` and `deployments: write`: everything
the planned parts of the pipeline need, and more than today's jobs ask for.
Every job of `nebensound.yml` asks only for what it needs, so a permission no
built job needs is asked for by none. Today only the job `Publish` asks for
more than a read token, `contents: write` to write tags and releases after a
push to `main`; every other job, and every job of a pull request, keeps a read
token (see [the phases](phases.md#pull-request-and-push)).

The caller grants more because the two directions do not cost the same. A
called workflow never gets more than its caller grants, and when one of its
jobs asks for more, GitHub refuses every run of that caller, pull requests
included. A permission added to the caller later would therefore mean a change
in every repository that calls the workflow, while one granted too much can be
taken out later without breaking a caller.

## The schema line runs ahead of the pin

The `#:schema` line of `.nebensound/repo.toml` points at `main`, so an editor
completes and marks keys by the newest schema, which can be ahead of the
workflow the repository pins. A key the newest schema knows can therefore be
fine in the editor and an error in the run. The check does not follow the line:
`check-toml` and phase 0 always use the schema of the pinned version, whether
the line is there or not. When the editor and the run disagree, the run is
right for this repository; raise the pin to use the key. See
[the declaration](declaration.md#the-file).

## Changing one linter rule

A repository's own Markdownlint configuration is laid over the
pipeline's default rule by rule, so it names only the rule it wants different;
nothing of the default has to be repeated. [Linter rules](../README.md#linter-rules)
in the README states how, with an example of a one-rule configuration file.
The same holds for Stylelint, whose one file at the root the tool reads the
same way, for html-validate, and for Ruff, whose configuration `check-python`
lays over the default through Ruff's own `extend`.
`nebensound-repo check` shows each rule a repository's configuration sets and
how it stands to the default.

For a single place in a single file, the tool's own inline directive is still
the smaller way and works in the check:

```markdown
<!-- markdownlint-disable-next-line MD033 -->
<details>
```

`<!-- markdownlint-disable MD033 -->` and `<!-- markdownlint-enable MD033 -->`
switch a rule off and on again around a stretch of a file. The YAML check has
no rules of a linter to switch off: what it judges beyond Prettier's format is
whether the file is valid YAML. What takes a file out of a check is a
different matter: only an `[[exclude]]` entry does that (see
[Exclusions](../README.md#exclusions)).
