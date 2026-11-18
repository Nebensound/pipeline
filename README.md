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

Every block checks one unit, and exactly the files `nebensound files <check>
<unit path>` names: the files Git tracks below the unit's path whose extension
the register gives that check, less what an `[[exclude]]` entry of the
declaration takes out. A file nobody committed is not judged, and a committed
one is not missed. Each finding is an annotation on its file.

The unit job gets the tool the same way `Declare` does, from the cache phase 0
fills ([`.github/actions/tool`](.github/actions/tool)); it builds the tool
only when the cache misses. The list does not travel through a job output,
which is limited in size.

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
  Prettier configuration and without its ignore files.

Every step runs even when one before it failed, so one run shows every finding.

### `check-markdown`

[`.github/actions/check-markdown`](.github/actions/check-markdown) checks every `.md` and `.markdown` file of the unit:

- Formatted the way Prettier writes it.
- No Markdownlint finding. Where the repository brings no Markdownlint
  configuration, the one beside the action applies (`MD013` off, since Prettier
  does not wrap prose). Its rules hold; its `ignores`, a negated `globs` entry
  and `gitignore` take nothing out.
- Every link into the repository leads to a file and a heading that exist
  (lychee, `--offline`). Links to the outside are not checked in a pull
  request: it should not turn red because someone else's server does not
  answer. lychee reads no `lychee.toml` or `.lycheeignore` of the repository.

### `check-rust`

[`.github/actions/check-rust`](.github/actions/check-rust) checks one Cargo
workspace, the unit's path. The toolchain is the one the workspace pins in its
`rust-toolchain.toml`; the oldest supported version is the `rust-version` its
packages claim. Neither is an input, so no repository can tell the check to look
away:

- Formatted the way `cargo fmt` writes it.
- Clippy without a warning, with all features and with no default features.
- Tests pass with all features and with no default features.
- Documentation builds without a warning.
- A release build succeeds.
- The workspace builds with the oldest `rust-version` any package claims.
- Regions, lines and functions are 100% covered (`cargo-llvm-cov`, on a pinned
  nightly toolchain, where `#[coverage(off)]` takes effect).
- Every published package builds and passes its tests from its own packaged
  contents, outside the source tree.

A `.rs` file that `[[exclude]]` takes out is neither held to the formatting nor
counted for coverage. Clippy, the tests, the documentation and the builds work
on the crates as Cargo compiles them; a crate cannot be compiled without one of
its files, so an excluded file a crate compiles is compiled with it.

### `check-toml`

[`.github/actions/check-toml`](.github/actions/check-toml) checks every `.toml`
and `.lock` file of the unit with Taplo 0.10.0, the release binary pinned by
SHA-256:

- Taplo reads each file from standard input. Given a path, it silently skips
  every `.lock` file and every file the `include` or `exclude` of the
  repository's `taplo.toml` leaves out, and reports success.
- `.toml` files are formatted the way Taplo writes them, with the repository's
  `taplo.toml`. Lockfiles are not format-checked: the tool that writes them
  (Cargo, uv) decides their layout.
- Every file passes `taplo lint`, run without the repository's configuration so
  that it cannot switch the schema validation off.
- Schemas are assigned by the action, so far to `Cargo.toml` by file name and
  to `.nebensound/repo.toml` by path (below). The schemas for `Cargo.toml` are
  copies of SchemaStore's schemas that lie beside the action with their
  licence notice (see [their README](.github/actions/check-toml/schemas/README.md)),
  so the check makes no request to SchemaStore. Apart from the declaration, a
  file whose leading comments hold its own `#:schema` directive with an
  absolute URL keeps that schema. A schema that cannot be read fails the check
  instead of being skipped.
- `.nebensound/repo.toml` is always checked against the schema of the
  declaration that lies beside the action, the one of the same pipeline
  version as phase 0, directive or not. `nebensound schema` derives it from
  the types phase 0 reads the file with, so an unknown key fails with its line
  and column. The schema is looser than phase 0, never stricter.

The schema of the declaration on `main` has a public address, for editors:

```text
https://raw.githubusercontent.com/Nebensound/pipeline/main/.github/actions/check-toml/schemas/nebensound-declaration.json
```

An editor that reads Taplo's directive finds it through this line before the
first key or table of `.nebensound/repo.toml`:

```toml
#:schema https://raw.githubusercontent.com/Nebensound/pipeline/main/.github/actions/check-toml/schemas/nebensound-declaration.json
```

### `check-yaml`

[`.github/actions/check-yaml`](.github/actions/check-yaml) checks every `.yml` and `.yaml` file of the unit:

- Formatted the way Prettier writes it.
- No yamllint finding. Where the repository brings no yamllint configuration,
  the one beside the action applies: the defaults, less what contradicts
  Prettier or GitHub Actions. Its rules hold; its `ignore` and
  `ignore-from-file` take nothing out.
- Every workflow directly under `.github/workflows` passes actionlint, with
  ShellCheck reading every `run:` script. actionlint 1.7.12 does not know `$/`
  yet and reports every such call as invalid; the block drops exactly these
  two messages, and only for a `uses:` that begins with `$/`.
- `.github/dependabot.yml` matches the Dependabot schema (`check-jsonschema`,
  pinned, with the schema it brings along). An unknown key fails the check
  with the file and the key named, instead of surfacing only after the merge.

### Exclusions

`.nebensound/repo.toml` is the only place where a repository takes a file out
of a check, with an `[[exclude]]` entry and its reason. `nebensound tree` shows
such a file as excluded, with the reason.

No block reads a tool's own way of leaving files out: an ignore file needs no
reason and can turn a check green that checked nothing. `.gitignore` takes
nothing out either. Run by hand, though, the tools still read these settings,
so phase 0 holds them against the declaration and fails on every file one of
them takes out that no `[[exclude]]` entry takes out (`unexcused-ignore`). A
setting that takes out less is fine. Phase 0 reads:

| Tool              | Setting                                                                                     |
| ----------------- | ------------------------------------------------------------------------------------------- |
| Prettier          | `.prettierignore` at the root                                                               |
| Taplo             | `include` and `exclude` in `.taplo.toml` or `taplo.toml` at the root                        |
| markdownlint-cli2 | `ignores` in `.markdownlint-cli2.jsonc` or `.markdownlint-cli2.yaml`, in every folder       |
| yamllint          | `ignore` and `ignore-from-file` in `.yamllint`, `.yamllint.yaml` or `.yamllint.yml` at root |
| lychee            | `exclude_path` in `lychee.toml` at the root                                                 |

Only the files the tool reads in a block count: a `.prettierignore` that takes
out a Rust file takes out nothing Prettier would have looked at. Taplo's
`include` counts only against `.toml` files, the ones Taplo reads without it; a
`.lock` file is taken out of Taplo only by `exclude`. Where yamllint's file sets
neither `ignore` nor `ignore-from-file`, phase 0 reads them in the file its
`extends` names, as yamllint does. A setting phase 0 cannot read fails as well
(`unreadable-ignore-file`). A JavaScript options file of markdownlint-cli2
(`.markdownlint-cli2.cjs` or `.mjs`) is not judged, since phase 0 cannot read it
without running it; the block sets its `ignores` aside all the same.

## A repository's own checks

Checks that belong to one repository alone run through its xtask, inside
`Check`, without a workflow or a ruleset entry of their own:

- The xtask is a Rust program whose manifest lies at `xtask/Cargo.toml` in the
  repository root, reached from the root as `cargo xtask` (usually through an
  alias in `.cargo/config.toml`).
- When `xtask/Cargo.toml` exists, the last step of
  `nebensound / Check repository` runs `cargo xtask check --all` from the
  repository root, with the toolchain the xtask pins: the `rust-toolchain.toml`
  in `xtask/`, else the one in the repository root. An xtask that pins none
  fails the step.
- A non-zero exit code fails the step, and with it `nebensound / Check`. What
  the xtask prints stands in the log of that step.
- A repository without `xtask/Cargo.toml` gets the step skipped and nothing
  else.
- The xtask is Rust code like any other: phase 0 demands a unit that runs the
  `rust` check over `xtask/`.

## Licence

Licensed under the [European Union Public Licence v. 1.2](LICENSE) (EUPL-1.2).
