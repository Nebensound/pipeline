# pipeline

Nebensound's central pipeline. A repository describes itself once, in
`.nebensound/repo.toml`, and calls the pipeline; the pipeline checks, builds and
publishes what that declaration says.

## What it is

- **One reusable workflow**, which a repository calls.
- **Its building blocks.** Each block performs one step and is named after its
  entry in the register, with the category in front: `check-<name>`,
  `build-<name>`, `publish-<target>`.
- **The tool `nebensound`.** It reads a repository's declaration, checks it
  against the tree and tells the workflow which blocks to run for which part of
  the repository.

## Status

This is a prototype. Releases are on `0.0.x`. The code arrives piece by piece;
the open issues and milestones show what comes next.

## Calling the pipeline

A repository describes itself in `.nebensound/repo.toml` and adds one caller
file, for example `.github/workflows/gate.yml`. This is the whole file:

```yaml
name: gate

on:
  pull_request:
  push:
    branches:
      - main

permissions:
  contents: read

jobs:
  nebensound:
    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@<full commit SHA>
    permissions:
      contents: read
```

One job, no inputs: what the repository is stands in its declaration, and the
workflow reads it there. The reference is pinned to an exact version, so that a
change to the pipeline never reaches a repository as a silent switch. Until the
first release there is no version, so it is pinned to the full 40-character SHA
of a commit on `main` and raised by hand; from then on to a release tag such as
`v0.0.1`, which Dependabot raises as a pull request.

A run reports as `nebensound / Declare` (phase 0: the declaration read and
checked against the tree), one job per unit that runs a check
(`nebensound / Check repository`, `nebensound / Check component <name>`) and
`nebensound / Check`, which sums the units up and is the one name a ruleset
requires. A check the register knows but that has no building block yet fails
by name in the step `Not written yet` of its unit.

This repository calls the workflow the same way, with one difference: its own
[`gate.yml`](.github/workflows/gate.yml) calls the workflow at the commit that
is running, so a change to the pipeline is checked by the pipeline it changes.

## Building blocks

Each block lives under `.github/actions/<block>` and takes one input, the
unit's path from the plan of phase 0. It checks the files Git tracks below that
path, with its tools pinned and installed outside the checkout. Every step runs
even when one before it failed, so one run shows every finding, each as an
annotation on its file.

`check-markdown` checks every `.md` and `.markdown` file of the unit:

- Formatted the way Prettier writes it.
- No Markdownlint finding. Where the repository brings no Markdownlint
  configuration, the one beside the action applies (`MD013` off, since Prettier
  does not wrap prose).
- Every link into the repository leads to a file and a heading that exist
  (lychee, `--offline`). Links to the outside are not checked in a pull
  request: it should not turn red because someone else's server does not
  answer.

`check-yaml` checks every `.yml` and `.yaml` file of the unit:

- Formatted the way Prettier writes it.
- No yamllint finding. Where the repository brings no yamllint configuration,
  the one beside the action applies: the defaults, less what contradicts
  Prettier or GitHub Actions.
- Every workflow directly under `.github/workflows` passes actionlint, with
  ShellCheck reading every `run:` script. actionlint 1.7.12 does not know `$/`
  yet and reports every such call as invalid; the block drops exactly these
  two messages, and only for a `uses:` that begins with `$/`.
- `.github/dependabot.yml` matches the Dependabot schema (`check-jsonschema`,
  pinned, with the schema it brings along). An unknown key fails the check
  with the file and the key named, instead of surfacing only after the merge.

**Transition rule for exclusions:** for now, the blocks honour the exclusions a
repository gives its tools: `.prettierignore`, its own Markdownlint or yamllint
configuration, `lychee.toml`. That is temporary: such an exclusion needs no
reason and can turn a check green that checked nothing. Once the plan carries
the `[[exclude]]` entries of the declaration, `repo.toml` is the only source
([#10](https://github.com/Nebensound/pipeline/issues/10)).

## Licence

Licensed under the [European Union Public Licence v. 1.2](LICENSE) (EUPL-1.2).
