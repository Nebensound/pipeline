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

Every block checks one unit: the files Git tracks below the unit's path whose
extension the register gives that check. A file nobody committed is not judged,
and a committed one is not missed. Each finding is an annotation on its file.

### `check-json`

[`.github/actions/check-json`](.github/actions/check-json) checks every
`.json`, `.jsonc` and `.geojson` file of the unit:

- `.json` and `.geojson` are strict JSON, exactly one value per file, judged by
  `jq`.
- `.jsonc` is strict JSONC as [jsonc.org](https://jsonc.org) defines it: JSON
  plus comments, trailing commas allowed. Biome judges it as a parser only, with
  the action's own configuration and no lint rule; a repository's Biome
  configuration is not read.
- All three are formatted the way Prettier writes them, with the repository's
  Prettier configuration.

Every step runs even when one before it failed, so one run shows every finding.

### `check-toml`

[`.github/actions/check-toml`](.github/actions/check-toml) checks every `.toml`
and `.lock` file of the unit with Taplo 0.10.0, the release binary pinned by
SHA-256:

- Taplo reads each file from standard input. Given a path, it silently skips
  every `.lock` file and every file the repository's `taplo.toml` excludes, and
  reports success.
- `.toml` files are formatted the way Taplo writes them, with the repository's
  `taplo.toml`. Lockfiles are not format-checked: the tool that writes them
  (Cargo, uv) decides their layout.
- Every file passes `taplo lint`, run without the repository's configuration so
  that it cannot switch the schema validation off.
- Schemas are assigned by the action, by file name, so far `Cargo.toml`. They
  are copies of SchemaStore's schemas that lie beside the action with their
  licence notice (see [their README](.github/actions/check-toml/schemas/README.md)),
  so the check makes no request to SchemaStore. A file with its own `#:schema`
  directive keeps its schema. A schema that cannot be read fails the check
  instead of being skipped.

### Exclusions, for now

For now, the blocks honour the exclusions a repository gives its tools, such as
`.prettierignore`. That is temporary: such an exclusion needs no reason and can
turn a check green that checked nothing. Once the plan carries the
`[[exclude]]` entries of the declaration, `repo.toml` is the only place where a
file is taken out of a check ([#10](https://github.com/Nebensound/pipeline/issues/10)). `exclude` in `taplo.toml` already takes
nothing out of `check-toml`.

## Licence

Licensed under the [European Union Public Licence v. 1.2](LICENSE) (EUPL-1.2).
