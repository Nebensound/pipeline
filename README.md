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

## Licence

Licensed under the [European Union Public Licence v. 1.2](LICENSE) (EUPL-1.2).
