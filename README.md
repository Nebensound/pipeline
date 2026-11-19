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

## Reference

- [The declaration](docs/declaration.md): every field of
  `.nebensound/repo.toml`, its default and effect, and every finding phase 0
  reports.
- [The register](docs/register.md): the presets, checks, builds, publish
  targets, version writers and ecosystems a declaration can name, generated
  from the register compiled into the tool.
- [The phases](docs/phases.md): what runs, under which name, on a pull request
  and on a push.

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
`.json`, `.jsonc` and `.geojson` file of the unit, and every `composer.lock` and
`flake.lock`:

- `.json` and `.geojson` are strict JSON, exactly one value per file, judged by
  `jq`.
- `.jsonc` is strict JSONC as [jsonc.org](https://jsonc.org) defines it: JSON
  plus comments, trailing commas allowed. Biome judges it as a parser only, with
  the action's own configuration and no lint rule; a repository's Biome
  configuration is not read.
- `composer.lock` and `flake.lock` are strict JSON like `.json`, judged by `jq`,
  but not format-checked: the tool that writes them decides their layout
  (Composer indents with four spaces).
- The three extensions are formatted the way Prettier writes them, with the
  repository's Prettier configuration, without its ignore files, and with
  `requirePragma`, `checkIgnorePragma`, `rangeStart` and `rangeEnd` set aside,
  which would pass a whole file unchecked.

Every step runs even when one before it failed, so one run shows every finding.

### `check-markdown`

[`.github/actions/check-markdown`](.github/actions/check-markdown) checks every `.md` and `.markdown` file of the unit:

- Formatted the way Prettier writes it, run as in `check-json`.
- No Markdownlint finding. Where the repository brings no Markdownlint
  configuration, the one beside the action applies (`MD013` off, since Prettier
  does not wrap prose). Its rules hold; its `ignores`, a negated `globs` entry
  and `gitignore` take nothing out.
- Every link into the repository leads to a file and a heading that exist
  (lychee, `--offline`). Links to the outside are not checked in a pull
  request: it should not turn red because someone else's server does not
  answer. lychee reads the repository's `lychee.toml` and `.lycheeignore` at
  the root, so excluded addresses, accepted status codes and timeouts stay the
  repository's configuration. An `exclude_path` there is held against the
  declaration by phase 0, like every setting that takes files out (see
  [Exclusions](#exclusions)). The block names `lychee.toml` as the
  configuration, so a lychee table in `Cargo.toml`, `pyproject.toml` or
  `package.json`, which lychee would otherwise read where there is no
  `lychee.toml`, is not read.

### `check-rust`

[`.github/actions/check-rust`](.github/actions/check-rust) checks one Cargo
workspace, the unit's path. The toolchain is the one the workspace pins in its
`rust-toolchain.toml`; the oldest supported version is the `rust-version` its
packages claim. Neither is an input, so no repository can tell the check to look
away:

- Formatted the way `cargo fmt` writes it: every member, and every path
  dependency that is no member, as `cargo fmt --all` reaches it. Such a
  dependency is formatted only; the steps below never reached it.
- Clippy without a warning, with all features and with no default features.
- Tests pass with all features and with no default features.
- Documentation builds without a warning.
- A release build succeeds.
- The workspace builds with the oldest `rust-version` any package claims.
- Regions, lines and functions are 100% covered (`cargo-llvm-cov`, on a pinned
  nightly toolchain, where `#[coverage(off)]` takes effect).
- Every published package builds and passes its tests from its own packaged
  contents, outside the source tree.

Rust code is excluded by the package, never by the file. Phase 0 fails on an
`[[exclude]]` entry that takes a `.rs` file out of a package without taking out
the folder that holds its `Cargo.toml` (`partial-package-exclusion`), and names
the language's own means instead: `#[rustfmt::skip]`, `#[coverage(off)]` with a
justification, `#[allow(...)]`. A package excluded as a whole, code of others
such as a vendored copy, is out of every step: not formatted, not
linted, not tested, not documented, not counted for coverage, and its
`rust-version` and `publish` play no part. It is only compiled where another
package depends on it, and rustc still prints its own warnings for it then,
which fail nothing. That holds for a workspace member and for a path dependency
outside the workspace alike: Clippy runs with `--no-deps`, since it would
otherwise lint a member it compiles as a dependency of another one, and it never
lints a path dependency that is no member. Every excluded package is named in
the summary of the run with its reason, as a notice. Each package is judged on
its own: one that lies inside an excluded package and that the declaration
keeps in with an `[[include]]` is checked in every step, coverage included.

### `check-toml`

[`.github/actions/check-toml`](.github/actions/check-toml) checks every `.toml`
file of the unit, and `Cargo.lock` and `uv.lock`, with Taplo 0.10.0, the
release binary pinned by SHA-256:

- Taplo reads each file from standard input. Given a path, it silently skips
  every `.lock` file (the lockfiles are fed through standard input all the
  same) and every file the `include` or `exclude` of the repository's
  `taplo.toml` leaves out, and reports success.
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

- Formatted the way Prettier writes it, run as in `check-json`.
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
such a file as excluded, with the reason. Rust code is excluded only as a whole
package (see [`check-rust`](#check-rust)).

No block lets a tool's own way of leaving files out take a file out: an ignore
file needs no reason and can turn a check green that checked nothing.
`.gitignore` takes nothing out either. The blocks read none of these settings,
except lychee's `exclude_path`, which comes along with the `lychee.toml`
`check-markdown` reads, and rustfmt's `ignore`, which comes along with the
repository's rustfmt configuration and takes effect on a nightly toolchain.
Settings in a tool's configuration that pass a whole file unchecked are set
aside on the command line: Prettier's `requirePragma`, `checkIgnorePragma`,
`rangeStart` and `rangeEnd`, and rustfmt's `disable_all_formatting`. Run by
hand, the tools read all of them, so phase 0
holds them against the declaration and fails on every file one of them takes
out that no `[[exclude]]` entry takes out (`unexcused-ignore`). For lychee,
that also keeps `exclude_path` from taking a file out in the block: phase 0
matches it against the path from the root, such as `docs/a.md`, the string the
block hands lychee. A setting that takes out less is fine. Phase 0 reads:

| Tool              | Setting                                                                                     |
| ----------------- | ------------------------------------------------------------------------------------------- |
| Prettier          | `.prettierignore` at the root                                                               |
| Taplo             | `include` and `exclude` in `.taplo.toml` or `taplo.toml` at the root                        |
| markdownlint-cli2 | `ignores` in `.markdownlint-cli2.jsonc` or `.markdownlint-cli2.yaml`, in every folder       |
| yamllint          | `ignore` and `ignore-from-file` in `.yamllint`, `.yamllint.yaml` or `.yamllint.yml` at root |
| lychee            | `exclude_path` in `lychee.toml` at the root                                                 |
| rustfmt           | `ignore` in `.rustfmt.toml` or `rustfmt.toml`, in every folder                              |

Only the files the tool reads in a block count: a `.prettierignore` that takes
out a Rust file takes out nothing Prettier would have looked at. Taplo's
`include` counts only against `.toml` files, the ones Taplo reads without it; a
`Cargo.lock` or `uv.lock` is taken out of Taplo only by `exclude`. Where
yamllint's file sets neither `ignore` nor `ignore-from-file`, phase 0 reads them
in the file its `extends` names, as yamllint does. rustfmt's `ignore` is read in
every folder, relative to that folder, since which configuration rustfmt applies
to a file depends on where it runs; a `.rs` file it takes out cannot be excused
by an `[[exclude]]` entry of its own, only by one that takes out its whole
package. A setting phase 0 cannot read fails as well (`unreadable-ignore-file`). A JavaScript options file of markdownlint-cli2
(`.markdownlint-cli2.cjs` or `.mjs`) is not judged, since phase 0 cannot read it
without running it; the block sets its `ignores` aside all the same. A setting
that switches a rule off for some files, such as yamllint's per-rule `ignore`
or markdownlint-cli2's `overrides`, takes no file out of the tool and stays the
repository's rule configuration. lychee's `.lycheeignore` leaves out addresses,
not files, so phase 0 does not judge it.

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
- Beside `check`, an xtask offers `setup` as its second standard task:
  `cargo xtask setup` calls `nebensound setup`, which finds or installs the
  programs the register's checks call
  ([the tool's README](tools/nebensound/README.md#setting-up-the-programs)), and
  then adds what only that repository needs, such as a program its own checks
  call.
- A repository without `xtask/Cargo.toml` gets the step skipped and nothing
  else.
- The xtask is Rust code like any other: phase 0 demands a unit that runs the
  `rust` check over `xtask/`.

## Test cases of the building blocks

A wrong block is wrong in every repository at once, so every block is held to
fixed test cases on every pull request here, through this repository's own
xtask in `xtask/` and the step above.

Each block brings its cases beside its `action.yml`: at least one under
`tests/pass/` and one under `tests/fail/`. A case is a folder that holds a small
repository of its own, with its own `.nebensound/repo.toml`. A failing case also
holds a file `expected-finding`, which names what the block must report: every
line that is not empty and does not start with `#` must match a whole line the
block prints, with `*` standing for any text, for example
`::error file=data.json::not formatted the way Prettier writes it`. That file is
not part of the case's repository.

`cargo xtask check --all` first holds the blocks, the register and
`nebensound.yml` to each other:

- every directory under `.github/actions/` is a block that belongs to a register
  entry (`check-<name>`, `version-<writer>`, `build-<name>`,
  `publish-<target>`), or the workflow's own machinery (`phase0`, `tool`);
- every block is called from a step of `nebensound.yml` that is named after its
  entry and switched on it, as `contains(matrix.checks, '<name>')`;
- the step `Not written yet` counts exactly the checks that have a block as
  written;
- every block has a passing and a failing case.

For now the xtask knows only how a check is called and tested, so a block of
another category is reported until the xtask learns that kind. The first
`version-`, `build-` or `publish-` block teaches it, together with its cases.

Then it runs every block against its cases. It builds the tool through
[`.github/actions/tool`](.github/actions/tool), copies each case into a fresh
Git repository, runs [`.github/actions/phase0`](.github/actions/phase0) on it,
and runs the block once for every unit the plan starts the block's check for,
just as a unit job does. A passing case needs the block to succeed everywhere. A
failing case needs it to fail and to print every finding the case names, so a
block that fails for another reason does not count.

The xtask keeps no copy of what a block does. It reads every step from the
block's `action.yml` and runs it the way the runner of GitHub Actions does: it
decides each `if:`, fills in every `${{ … }}`, hands each `run:` to bash and
reads back what the step writes to `GITHUB_OUTPUT`, `GITHUB_ENV` and
`GITHUB_PATH`. A composite action a block calls, such as lychee's, is fetched at
the ref the block names and run the same way. An action that runs on Node.js
cannot run like that; for the ones the blocks call, the xtask stands in:

| Action                                                               | What stands in                                                                                |
| -------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `actions/setup-node`, `actions/setup-go`                             | The tool already on the PATH, in the version the step asks for; anything else fails the step. |
| `Swatinem/rust-cache`, `actions/cache/restore`, `actions/cache/save` | Nothing: a cache changes how fast a step runs, not what it finds.                             |

Any other such action fails the xtask until it has a stand-in. In the gate, the
blocks that ran before the xtask in `Check repository` have put Node.js and Go
on the PATH. One step does not run: `Make room on the runner` of `check-rust`,
which deletes software of the machine with `sudo`; on a developer's machine,
that software would be theirs. A step the xtask leaves out has to exist, so
renaming it fails the xtask instead of letting the step run.

What a step prints stands in the log behind a `|`, so that the findings of a
failing case do not become annotations of the gate. What the xtask itself
finds is an annotation.

The blocks are written for the Linux runners of GitHub Actions, and so for now
the cases run on Linux only, in the gate on every pull request and locally on
Linux. Elsewhere the xtask holds the blocks, the register and `nebensound.yml`
to each other, says in one sentence that the cases run only on Linux for now,
and exits with `1`, since the cases did not run. Running them on macOS comes
with the checks that run from the tool (#73, #74).

`cargo xtask setup` calls `nebensound setup` for this repository with the tool
of this commit, built from `tools/nebensound/`, and hands it every option
unchanged (`--pinned`, `--with-runtimes`, `--check`). It adds nothing of its
own: the blocks fetch their programs themselves when the cases run them, and
what they expect of the machine stays the machine's: Node.js and Go on the
`PATH` in the versions their steps ask for, Python, and rustup.

The cases are
taken out of this repository's own checks with `[[exclude]]` and a reason in
[`.nebensound/repo.toml`](.nebensound/repo.toml). A case of `check-rust` is a
whole Cargo package, the only way Rust code can be excluded.

## Licence

Licensed under the [European Union Public Licence v. 1.2](LICENSE) (EUPL-1.2).
