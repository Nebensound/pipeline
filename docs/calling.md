# Calling the workflow

What a repository needs to know when it starts calling `nebensound.yml`, and
that the workflow's reference does not state elsewhere. The caller file itself
is in [the README](../README.md#calling-the-pipeline); what runs and under which name is
in [the phases](phases.md).

## Pinning before `v0.1.0`

The reference in the caller file is pinned to an exact version. This
repository releases every merge to `main`, as `v0.0.N` until `v0.1.0`, the
first version meant for callers. Until then, pin the full 40-character SHA of a
commit on `main` and write the commit's date behind it as a comment, so that a
reader sees how old the pin is without looking the commit up:

```yaml
jobs:
  nebensound:
    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@<full commit SHA> # 2026-11-19
    permissions:
      contents: write
      pull-requests: read
      id-token: write
      packages: write
      attestations: write
      deployments: write
```

The date is the commit's, as `git log -1 --format=%cs <SHA>` prints it, not the
day of the pin. A shortened SHA or a branch name is not a pin. Raise the pin by
hand, to the then current commit of `main`. From `v0.1.0` on, pin the release
tag, which Dependabot raises as a pull request.

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

A repository's own Markdownlint or yamllint configuration is laid over the
pipeline's default rule by rule, so it names only the rule it wants different;
nothing of the default has to be repeated. [Linter rules](../README.md#linter-rules)
in the README states how, with an example of a one-rule configuration file.
The same holds for Stylelint, html-validate and Ruff, whose configuration
`check-python` lays over the default through Ruff's own `extend`.
`nebensound-repo check` shows each rule a repository's configuration sets and
how it stands to the default.

For a single place in a single file, the tool's own inline directive is still
the smaller way and works in the check:

```markdown
<!-- markdownlint-disable-next-line MD033 -->
<details>
```

`<!-- markdownlint-disable MD033 -->` and `<!-- markdownlint-enable MD033 -->`
switch a rule off and on again around a stretch of a file, and yamllint reads a
comment such as `# yamllint disable-line rule:truthy`. What takes a file out of
a check is a different matter: only an `[[exclude]]` entry does that (see
[Exclusions](../README.md#exclusions)).
