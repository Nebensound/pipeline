# nebensound

The tool that reads a `.nebensound/repo.toml`, resolves it and checks it against
what the repository holds — **phase 0**. The same binary in development and in
CI: two ways would be two answers to one question.

## Layout

```text
tools/nebensound/            the tool — Cargo.toml, src/, tests/
  crates/
    nebensound-declaration/  the library, which can be published on its own
```

The tool **is** the root of the workspace, not a package among equals. Below
`crates/` lies only what carries a version of its own and can be published
separately — today exactly one thing: the library that understands the format.
It deliberately has no input and no output; it is handed a declaration, the
vocabulary and a list of paths, and answers with a tree. Everything that finds
that tree and shows it lives up here.

The tool itself is not to be published: whoever needs it is to get it through
the composite action under `.github/actions/phase0`. A second distribution
channel would be a second version that can fall behind.

## Calls

```text
nebensound [-C <directory>] [check | tree | explain <path> | plan | files [-z] <check> <unit> | excluded-packages | write-versions [--plan <file>] | schema | register | ui]
```

| Call                | What it does                                                                                                                                                                                                                                                                                     |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `check`             | Runs phase 0 and prints everything: the resolved configuration, the tree, the findings and a summary line with the counts. The default, and what the composite action under `.github/actions/phase0` calls.                                                                                      |
| `tree`              | The annotated tree and nothing else.                                                                                                                                                                                                                                                             |
| `explain <path>`    | What holds for one path and why: which units contain it, how each of them is released, which checks are in force, how its coverage stands.                                                                                                                                                       |
| `plan`              | Which unit runs which check and builds what, and which version each unit carries, as one line of JSON for a matrix to read.                                                                                                                                                                      |
| `files`             | The files one check covers in one unit, less what `[[exclude]]` takes out: exactly what its building block checks. One per line, or each ended by a zero byte with `-z`.                                                                                                                         |
| `excluded-packages` | The Rust packages `[[exclude]]` takes out as a whole, each with its reason, as one line of JSON: what `check-rust` leaves out of every step, and what the composite action under `.github/actions/phase0` names in the summary of the run.                                                       |
| `write-versions`    | Writes the version of every unit with the version writer `cargo` into its `Cargo.toml`, the path dependencies on it and `Cargo.lock`, in the working tree; never committed. With `--plan <file>`, takes the versions from a printed plan instead of the history.                                 |
| `schema`            | The JSON Schema of `.nebensound/repo.toml`, derived from the types phase 0 reads it with. Reads no repository. `check-toml` assigns the copy committed beside it; [its README](../../.github/actions/check-toml/schemas/README.md#nebensound-declarationjson) says how to write that copy again. |
| `register`          | The register as Markdown, exactly as [`docs/register.md`](../../docs/register.md) holds it, which a test compares with this output. Reads no repository.                                                                                                                                         |
| `ui`                | The same result to walk through. Only in a build with the `ui` feature.                                                                                                                                                                                                                          |

Exit code: `0` when there is nothing to report; `1` when the repository has a
problem or cannot be read; `2` when the call itself was wrong. A **warning stops
nothing** — it says that a statement is untrue, not that something is broken.

Everything goes to standard output, including what went wrong. The result is a
log, and a log that stands in two places is read together by nobody; whether
something went wrong is what the exit code is for.

## The plan

`plan` turns the answer of phase 0 around: not what holds for a file, but what
a run has to start, and where.

```json
{
  "checks": [
    { "check": "json", "kind": "repository", "name": null, "path": "." },
    { "check": "rust", "kind": "component", "name": "tool", "path": "tool" }
  ],
  "units": [
    {
      "build": null,
      "checks": ["json", "markdown", "toml", "yaml"],
      "kind": "repository",
      "name": null,
      "path": "."
    },
    {
      "build": "cargo-package",
      "checks": ["rust"],
      "kind": "component",
      "name": "tool",
      "path": "tool"
    }
  ],
  "versions": [
    {
      "base": "0.3.1",
      "kind": "repository",
      "name": null,
      "path": ".",
      "prefix": "v",
      "release": "single-stage",
      "version": "0.3.2",
      "writers": []
    },
    {
      "base": "1.4.0",
      "kind": "component",
      "name": "tool",
      "path": "tool",
      "prefix": "tool-v",
      "release": "two-stage",
      "version": "1.5.0-pre.3",
      "writers": ["cargo"]
    }
  ]
}
```

Printed as one line, with the keys in alphabetical order; broken here to be
read, and `checks` shortened. It is written by `serde_json`, so a path holding
quotes or control characters cannot break the JSON. `units` is
every unit phase 0 resolved, repository first. `checks` is the same thing flat —
one entry per check that is started — because a matrix takes a list and cannot
unfold a list inside one. The repository has no name; its tags carry only the
prefix `v`.

`versions` is one entry per unit that carries a version, in the order of
`units`: `prefix` is what its tags start with, `release` how it is released,
`base` its last full release (`0.0.0` without one), `version` the version
of the commit at hand, `null` when the commit does not touch the unit, and
`writers` the version writers the unit selects. How they
are resolved from tags and history is described in
[the reference of the declaration](../../docs/declaration.md#versions).

A check runs **once, at the outermost unit it is in force for**. The base set is
in force everywhere, so it runs at the repository; a check a unit declares
covers everything below it, so a unit inside that names it again starts no
second run over the same files. A unit can therefore stand in the plan with no
checks of its own.

A build is never shared: `build` names the one build a unit starts, or is `null`.
Only a unit that carries a version of its own builds, because its artifact is
released under that version. A unit with `versioning = "none"` builds nothing,
and one that generates children leaves the building to them — also with
`self-and-child-*`, where it carries a version beside theirs, since its build
would cover their files a second time.

A repository with errors gets no plan, only a sentence and exit code `1`: a plan
read off a broken declaration would start the wrong things.

## Writing the versions

`write-versions` is the version writer `cargo`. No manifest in the tree carries
a version, and Cargo cannot take one from outside, so the version of the plan
is written into the working tree before something builds with it, and never
committed. For every unit of the plan with a version and the writer `cargo`, it
sets `package.version` in the unit's `Cargo.toml`, gives every path dependency
on that package inside its workspace that version as its requirement, and sets
the package's entry in the workspace's `Cargo.lock`, together with every entry
that names it with a version. Nothing else changes, and `cargo build --locked`
passes afterwards.

The workspace is found the way Cargo finds it: the folder `package.workspace`
names, else the nearest folder at or above the package whose `Cargo.toml` has
a `[workspace]` that does not exclude it, else the package's own folder. The
dependents are the `Cargo.toml` files Git tracks below it; one that is not TOML
is passed over. Every change goes through `toml_edit`, so comments, blank lines
and the order of keys stay, and a file whose text does not change is not
written: a second call changes nothing.

```text
component lib: the package lib at crates/lib gets 1.4.1 (workspace root: .).
component app carries no version at this commit, so none is written for it.
Wrote Cargo.lock.
Wrote crates/app/Cargo.toml.
Wrote crates/lib/Cargo.toml.
```

Without `--plan` the versions are resolved from the history, as `plan` resolves
them. A job after Declare checks out the commit at hand and nothing before it,
so the building blocks hand in the plan Declare printed with `--plan <file>`.
A repository with errors gets nothing written, only a sentence and exit code
`1`. A manifest or lockfile that cannot be read, is not TOML or cannot be
written back, and a unit whose `Cargo.toml` holds no package with a name, end
the call with exit code `1` and the reason.

## The files of a check

`files <check> <unit>` names the files a building block checks: below the
unit's path, every file the register gives that check, less what an
`[[exclude]]` entry takes out. Each block takes its list from this call and
checks exactly those files, so `.nebensound/repo.toml` is the only place where
a file is taken out of a check.

The pair has to be one the plan starts: `files rust .` in a repository whose
Rust code is a component of its own is refused, with exit code `1`, rather than
answered with a list nobody checks. So is a check the register does not know,
and so is a repository with errors, for the same reason it gets no plan.

Phase 0 also holds the tools' own ways of leaving a file out against the
declaration — `.prettierignore`, `include` and `exclude` in `taplo.toml`,
`ignores` of markdownlint-cli2, `ignore` of yamllint, `exclude_path` of lychee,
`ignore` of rustfmt — and fails on every file one of them takes out that no
`[[exclude]]` entry does (`unexcused-ignore`). The building blocks read none of
them except lychee's `exclude_path`, which phase 0 matches against the path
from the root, the string `check-markdown` hands lychee, and rustfmt's
`ignore`, which takes effect on a nightly toolchain; run by hand, the tools
read them all.

## Excluded packages

Rust code is excluded by the package, never by the file: an `[[exclude]]` entry
that takes a `.rs` file out of a package without taking out the folder that
holds its `Cargo.toml` fails phase 0 (`partial-package-exclusion`), at the
entry's line and column in `.nebensound/repo.toml`. A package is a folder with a
`Cargo.toml` that has a `[package]` table, and a `.rs` file belongs to the
innermost one around it.

`excluded-packages` lists the packages taken out as a whole:

```json
[
  {
    "path": "vendor/parser",
    "reason": "a vendored copy, kept as upstream wrote it"
  }
]
```

Printed as one line, in path order. It names every such package of the
repository, inside a Rust unit or not, so that a path dependency outside the
workspace is listed as well. A repository with errors gets no list, only a
sentence and exit code `1`, for the same reason it gets no plan.

## Where the file list comes from

From `git ls-files`, not from a directory walk. What a repository holds is
decided by Git. A walk over the file system would count `target/`,
`node_modules/` and yesterday's half-finished file as well, and phase 0 would
judge things the repository does not have.

`git add` is enough; a commit is not needed, because `ls-files` reads the index.

The versions come from Git as well: the line of `main` (the first-parent line
of `HEAD`, or at a merge into `release` that of the commit of `main` it
merges), the declaration and the files each commit of that line held, the tags
and the branches `origin/release` and `origin/main`. Where a unit lived and how
it was released is read at each commit from its own declaration, so a unit
that moves or switches between one stage and two keeps its earlier releases.
At a merge into `release`, `origin/main` also tells whether it brings in the tip
of `main`: only the tip is promoted, and any other merge is the error
`stale-promotion`. A repository without a commit has no version yet; a shallow clone is an error,
because the history it lacks could hold the last release. Every option the person's Git configuration could change, such as
rename detection, is given on the command line, so the answer does not depend
on who runs it.

Unlike the file list, the versions need a commit. They are resolved for `HEAD`,
and each commit is read with the declaration it holds, so a change to
`.nebensound/repo.toml` that is not committed yet reaches them only in part; see
[what the history must hold](../../docs/declaration.md#what-the-history-must-hold).

## What the output says

```text
Tree

  .                                                     [the repository]
    Cargo.lock                                          ✓ toml
    LICENSE                                             – **/LICENSE: legal text, taken as it is
    doc/                                                [component docs]
      handbook.typ                                      ✓ typst [component handbook]
    src/
      main.rs                                           × rust — nobody runs it here
          error[uncovered-file] at src/main.rs: …
```

| Mark | What it means                                                                                                                                                |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `✓`  | A check covers the file, and a unit containing it runs that check.                                                                                           |
| `×`  | A check **would** cover it, and no unit containing it runs that check. That is an error.                                                                     |
| `?`  | The register knows no check for this kind of file. Also an error — the gap belongs closed centrally, if need be with a check that fails until it is written. |
| `–`  | An entry takes it out, and behind it stands which one and why.                                                                                               |

In square brackets a unit stands **where it begins**, and not again at every
line below it. A finding hangs at the path it is about; what hangs at none — a
check without a find, an unreachable target, a path the repository does not have
— stands under `Findings`.

## The interface

The interface is a Cargo feature, `ui`, and it is **off by default**. The build
the composite action runs is the one without it: phase 0 in CI prints a
report and needs no terminal library, and this repository is the most trusted
code in the organization — it compiles nothing it does not call. In a build
without the feature `ui` is not a call at all; it is refused like any other
unknown word, with exit code `2`.

```bash
cargo run --features ui -- -C ../.. ui        # on this repository
cargo run --features ui -- -C <directory> ui
```

`nebensound ui` shows the same tree to walk through. The tree on the left,
on the right the same answer `explain` gives for the selected node — literally
the same one, so that two answers to one question do not have to be maintained.

| Key              | What it does                                           |
| ---------------- | ------------------------------------------------------ |
| `j` `k`, `↓` `↑` | select                                                 |
| `l` `→`, `Enter` | unfold                                                 |
| `h` `←`          | fold up; when nothing is open here, step out one level |
| `g`              | to the next gap, unfolding the way there               |
| `f`              | show only the gaps                                     |
| `?`              | the key list                                           |
| `q`, `Esc`       | quit                                                   |

On start the way to every gap is already open: what is missing should be visible
without looking for it.

The interface makes **no decision of its own**. It shows the same analysis, and
the exit code afterwards is the one `check` would have reported. Without a
terminal it does not start at all but says that `check` is the command for that;
otherwise it would write control codes into a log.

Where the output goes is passed in rather than asked for. Under `cargo test` the
standard output is the terminal the run was started from, so asking would be
answered truthfully — and a check of `ui` would take that terminal over.

## Developing

Every command runs inside `tools/nebensound/`, where the toolchain is declared.

```bash
cargo run --quiet -- -C ../.. check    # phase 0 on this repository itself
cargo run --quiet -- -C ../.. plan     # and the plan a run would start

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy --workspace --all-targets --no-default-features --locked -- -D warnings
cargo test --workspace --locked --all-features
cargo test --workspace --locked --all-features --doc
cargo test --workspace --locked --no-default-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo llvm-cov --workspace --all-features --locked \
  --fail-under-lines 100 --fail-under-regions 100 --fail-under-functions 100
cargo llvm-cov --workspace --no-default-features --locked \
  --fail-under-lines 100 --fail-under-regions 100 --fail-under-functions 100
```

Together these are the gate for the tool: formatting, Clippy with
`-D warnings` in both builds, the tests in both builds, the doc tests, rustdoc
and full coverage of lines, regions and functions in both builds.

Raw mode, the alternate screen and the blocking read live in `src/terminal.rs`
and are checked from inside a **pseudo terminal** (`tests/pty.rs`) — the same
path a person takes. No line needs an exemption from coverage.
