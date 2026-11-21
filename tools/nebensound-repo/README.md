# nebensound-repo

The tool for the tasks of a repository: it reads a `.nebensound/repo.toml`,
resolves it and checks it against what the repository holds — **phase 0**. The
same binary in development and in CI: two ways would be two answers to one
question.

## Layout

```text
tools/nebensound-repo/  the tool — Cargo.toml, src/, tests/
  register.toml         the register, compiled in
  defaults.toml         what every repository declares without writing it down
  rules/                the pipeline's defaults for Markdownlint and yamllint
```

The tool is one package, a library and the program: one name, one version. The
part of the library that understands the format deliberately has no input and
no output; it is handed a declaration, the vocabulary and a list of paths, and
answers with a tree. Everything that finds that tree and shows it lives beside
it in the same package.

What only the program needs is the feature `program`, which is on by default
and which the binary requires: the downloading and unpacking of `setup`, and
the JSON Schema of `schema`. Building and installing the tool take no flag. What
builds on the library alone switches the default features off and compiles
neither.

`rules/` holds the defaults that `check-markdown` and `check-yaml` lay a
repository's own linter configuration over. The blocks read them from there,
and the tool compiles them in to show how a repository's rules stand to them
([The rules](#the-rules)), so both speak of the same files, and a change to
one of them is a change to the tool, which builds it again.

The tool itself is not to be published: whoever needs it is to get it through
the composite action under `.github/actions/phase0`. A second distribution
channel would be a second version that can fall behind.

## Calls

```text
nebensound-repo [-C <directory>] [check [--name <name>] [--default-branch <branch>] [--visibility <public|private|internal>] | tree | explain <path> | dependabot | plan [--pull-request <number> --message <message>] | version [--markdown] [--pull-request <number> --message <message>] | files [-z] <check> <unit> | excluded-packages | write-versions [--plan <file>] [--workspace <path>] | setup [--pinned] [--with-runtimes] [--check] | schema | register | ui]
```

| Call                | What it does                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `check`             | Runs phase 0 and prints all of it: the resolved configuration, the linter rules laid over the defaults, the tree, the findings (annotations in a workflow run) and the counts. Warns where the name or default branch (`--name`, `--default-branch`, else Git) or a minimum file departs from the conventions. With `--visibility`, how GitHub shows the repository, it holds the declared audience against it, as `phase0` does; without it nothing is compared. The default; `phase0` calls it. |
| `tree`              | The annotated tree and nothing else.                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `explain <path>`    | What holds for one path and why: for whom the repository is meant, which units contain it, how each of them is released, which commit types count like `feat`, which checks are in force, how its coverage stands.                                                                                                                                                                                                                                                                                |
| `dependabot`        | The `.github/dependabot.yml` the declaration and the organization's [Dependabot policy](../../docs/register.md#dependabot-policy) give: one entry per watched ecosystem, listing the folders that hold its manifests. Reads no history, and prints nothing but a sentence when phase 0 finds an error. `check` warns where the file in the tree departs from it ([`stale-dependabot`](../../docs/declaration.md#stale-dependabot)).                                                               |
| `plan`              | Which unit runs which check and builds what, and which version each unit carries, as one line of JSON for a matrix to read. With `--pull-request <number> --message <message>`, for a pull request, as `version` reads it. See [The plan](#the-plan).                                                                                                                                                                                                                                             |
| `version`           | The version of every unit that carries one, as a table: release mode, current version, the version of the commit at hand and, in two stages, the commits waiting for the next promotion. `--markdown` prints it as the job `Version` writes it. See [The versions](#the-versions).                                                                                                                                                                                                                |
| `files`             | The files one check covers in one unit, less what `[[exclude]]` takes out: exactly what its building block checks. One per line, or each ended by a zero byte with `-z`.                                                                                                                                                                                                                                                                                                                          |
| `excluded-packages` | The Rust packages `[[exclude]]` takes out as a whole, each with its reason, as one line of JSON: what `check-rust` leaves out of every step, and what the composite action under `.github/actions/phase0` names in the summary of the run.                                                                                                                                                                                                                                                        |
| `write-versions`    | Writes the version of every unit with the version writer `cargo` into its `Cargo.toml`, the path dependencies on it and `Cargo.lock`, in the working tree; never committed. With `--plan <file>`, takes the versions from a printed plan instead of the history; with `--workspace <path>`, starts from the units of the workspace at that path.                                                                                                                                                  |
| `setup`             | Finds or installs the programs that the checks the plan starts call, as the register pins them: by default it uses those on the computer and installs the pinned version of what is missing; `--pinned` installs every one into its own folder; `--check` changes nothing. See [Setting up the programs](#setting-up-the-programs).                                                                                                                                                               |
| `schema`            | The JSON Schema of `.nebensound/repo.toml`, derived from the types phase 0 reads it with. Reads no repository. `check-toml` assigns the copy committed beside it; [its README](../../.github/actions/check-toml/schemas/README.md#nebensound-declarationjson) says how to write that copy again.                                                                                                                                                                                                  |
| `register`          | The register as Markdown, exactly as [`docs/register.md`](../../docs/register.md) holds it, which a test compares with this output. Reads no repository.                                                                                                                                                                                                                                                                                                                                          |
| `ui`                | The same result to walk through. Only in a build with the `ui` feature.                                                                                                                                                                                                                                                                                                                                                                                                                           |

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
  "audience": "public",
  "checks": [
    { "check": "json", "kind": "repository", "name": null, "path": "." },
    { "check": "rust", "kind": "component", "name": "tool", "path": "tool" }
  ],
  "previous_audience": "public",
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
      "current": "0.3.2",
      "kind": "repository",
      "name": null,
      "path": ".",
      "prefix": "v",
      "release": "single-stage",
      "version": "0.3.2",
      "writers": []
    },
    {
      "current": "1.5.0-pre.2",
      "kind": "component",
      "name": "tool",
      "path": "tool",
      "prefix": "tool-v",
      "release": "two-stage",
      "version": null,
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
unfold a list inside one. The repository has no name; its records carry only
the prefix `v`.

`versions` is one entry per unit that carries a version, in the order of
`units`: `prefix` is what the names of its records, its tags and releases,
start with, `release` how it is released, `current` its current version, the
highest one the history gives a commit of the line up to the commit at hand, a
pre-release included (`null` before the first commit that touches the unit),
`version` the version of the commit at hand, `null` when the commit does not
touch the unit, and `writers` the version writers the unit selects. `current`
is what the version writer `cargo` requires of a path dependency whose unit
the commit does not touch, where a package that gets a version needs it. How
they are resolved from the history is described in
[the reference of the declaration](../../docs/declaration.md#versions).

`audience` is for whom the repository is meant, as the declaration at hand
says (see [the reference](../../docs/declaration.md#audience)), and
`previous_audience` what the declaration of the commit before it on the line
says, `null` when that commit names none or there is none. On a pull request
into `main` that is the tip of `main`, so Declare names a change of the value in
the summary of the run from the two. On a pull request from `main` into
`release` it is the commit of `main` the merge brings in, so a promotion names
no change: the change was named on its pull request into `main`.

`--pull-request <number> --message <message>` builds the plan for a pull
request, reading `HEAD` the way [`version`](#the-versions) does: every `version`
is then the one the squash merge would get, with `-pr.<number>` behind it, such
as `1.5.0-pre.3-pr.17`, and so is the `current` of a unit the pull request
touches, which is that version. The `current` of a unit it does not touch
carries no suffix. Declare builds the plan this way on a pull request, so that
the version writers write what a merge would publish.

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

## The versions

`version` prints the same versions `plan` lists, as a table to be read:

```text
Versions

  Unit            Path  Release       Current      This commit  Waiting
  the repository  .     single-stage  0.3.2        0.3.2
  component tool  tool  two-stage     1.5.0-pre.2  none         2
```

`Current` is the highest version the history gives a commit of the line up to
the commit at hand, a pre-release included, after a promotion the full version,
`none` before the first commit that touches the unit; `This commit` the version
of the commit at hand, `none` when the commit does not touch the unit;
`Waiting`, for a unit released in two stages, the commits since its last
full release that touch it, the commit at hand included, which is what its next
full release takes in. In one stage every commit that touches the unit is a
release of its own, and the cell stays empty. `--markdown` prints the same rows
as a Markdown table with a short legend, which the job `Version` writes into
the summary of the run. The repository always carries a version, so the table
is never empty.

`--pull-request <number> --message <message>` reads `HEAD` as the merge commit
GitHub builds for a pull request, which touches no unit itself. It stands for
the commit the squash merge would put on the base branch instead: one parent,
the first parent of `HEAD`, the changes of `HEAD` against it, and the message
given, which the job takes from the title and the description of the pull
request. Every version is then the one that merge would get, with
`-pr.<number>` behind it, and so is the current version of a unit the pull
request touches; the column is called `This pull request`. A merge into
`release` is read as it is, since such a pull request is merged with a merge
commit. Both options go together.

A repository with errors gets no table: the sentence that there are no versions,
every finding, and exit code `1`, as with the plan.

## Writing the versions

`write-versions` is the version writer `cargo`. No manifest in the tree carries
a version, and Cargo cannot take one from outside, so the version of the plan is
written into the working tree before something builds with it, and never
committed. Every unit of the plan with the writer `cargo` that the commit at
hand touches gets the version of the commit, `-pr.<number>` behind it in a plan
built for a pull request. A unit the commit does not touch gets a version only
where a package that gets one depends on it by path, followed through the path
dependencies of each unit that gets one: `current`, its current version. No
tag is read for it, so a repository without any tag gets its versions written
from its first commit on. With that version it sets `package.version` in the
unit's `Cargo.toml`, gives every path dependency on that package anywhere in the
repository that version as its requirement, and sets the package's entry in the
workspace's `Cargo.lock`, together with every entry that names it with a
version. Every other workspace that reaches the package through path
dependencies, directly or through another package, gets its `Cargo.lock` brought
in line the same way: one that excludes the package, one beside it, or a `fuzz/`
folder with a workspace of its own. Nothing else changes, and
`cargo build --locked` passes afterwards.

Any other unit with the writer gets nothing: its `Cargo.toml` stays as it is,
apart from the entries of the `[workspace.dependencies]` it holds as the root
of a workspace that a member that gets a version inherits, and without a
version the package test of `check-rust` does not package it.

The path dependencies followed are those in `[dependencies]`,
`[dev-dependencies]` and `[build-dependencies]`, also per platform or through
`workspace = true`: `cargo package` refuses a dependency without a version, or,
for a dependency of its tests, drops it from the package, so that the packaged
tests that use it would not compile. A path dependency on a package that
carries no version at all, a unit with `versioning = "none"` or one that does
not select the writer `cargo`, is left to Cargo: the call raises nothing of its
own, and the package test fails with Cargo's message. So is one on a unit that
no commit has touched yet, which has no current version to be written.

With `--workspace <path>`, only a touched unit that belongs to the workspace
Cargo finds at that path gets the version of the commit on its own account. A
workspace of its own in a folder below, one the workspace excludes or never
names, does not belong to it. Their path dependencies are followed wherever they
lie: a unit they reach gets its current version, which is the version of the
commit when the commit touches it. The package test of
`check-rust` starts from its workspace this way, so that what another workspace
of the repository needs cannot fail it. A path that is not one inside the
repository ends the call with exit code `2`.

The workspace is found the way Cargo finds it: the package's own folder when
its manifest has a `[workspace]`, else the folder `package.workspace` names,
else the nearest folder above it whose `Cargo.toml` has a `[workspace]` that
does not exclude it or names a folder in `package.workspace`, else the
package's own folder. A `[workspace]` that excludes the package does not end
the search, and an entry of `members` that names the package keeps it in
although `exclude` covers it. The dependents are all `Cargo.toml` files Git
tracks, inside that workspace or outside it; one that is not TOML is passed
over. Every change goes through `toml_edit`, so comments, blank lines and the
order of keys stay, and a file whose text does not change is not written: a
second call changes nothing.

```text
component app: the package app at crates/app gets 1.4.1 (workspace root: .).
component lib is not touched by this commit: the package lib at crates/lib gets its current version 1.2.0 (workspace root: .).
component log is not touched by this commit and no commit has touched it yet, so it has no version to be written.
component cli is not touched by this commit, and no package that gets a version depends on it by path, so nothing is written for it.
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

## Setting up the programs

The checks call external programs: formatters, linters, a link checker. The
register pins each of them to one version, with a download per operating system
and architecture and its digest, and names the runtime a program written in
JavaScript or Python needs, with the lowest version of it the program runs on
([Programs](../../docs/register.md#programs)).
`setup` gets a computer ready to run the checks of a repository:

```bash
nebensound-repo setup                  # use what the computer has, install what it lacks
nebensound-repo setup --pinned         # install the pinned version of every program
nebensound-repo setup --with-runtimes  # the same, and a missing runtime without asking
nebensound-repo setup --check          # change nothing; say what is missing or differs
```

It sets up the programs of the checks the plan starts, so it reads the
repository like `plan` does, and a repository with errors gets nothing set up,
for the same reason it gets no plan. It does not read the history, though, like
`files` and `excluded-packages`: which checks the plan starts never depends on a
version, so a shallow clone or an error about a release does not keep anyone
from setting up the programs.

- **By default** it uses a program that is on the computer already, found on
  the `PATH`, and says where its version differs from the pinned one. It
  installs the pinned version of what is missing, and nothing that is present.
- **`--pinned`** installs the pinned version of every program into its own
  folder, whatever the computer has, and leaves the computer's programs alone.
  This is the mode the gate is to run in once its blocks call `setup` (#73).
- **`--check`** changes nothing. It lists every program, and every runtime the
  default mode would need, with where it was found, what is missing, and which
  version differs from the pinned one.

A download is refused unless it has the digest the register pins, and nothing
of it is used then. Downloads go through `curl`, which macOS, Linux and Windows
bring; a program on a runtime is installed by that runtime's installer: npm
with `--ignore-scripts`, or pip into a virtual environment of its own, taking
only wheels, so that no build code of a dependency runs.

A **runtime** (Node.js, Python) is needed only for the programs the run
installs on it or starts through it: a program that is to be installed, since
the runtime's installer puts it in place, and a pinned copy installed with npm,
whose command is a script that calls the runtime. A pinned copy installed with
pip starts with the Python of its own environment, and a program on the
computer that the default mode leaves as it is starts the way it was installed;
for neither of them does `setup` look for the runtime or ask for it.

A runtime is never installed unasked. One on the computer is used as it is,
whatever its version, as long as it prints one and can run the programs that
need it, in every mode. The search goes on past one that prints no version
(such as the Windows Store's `python3.exe` stand-in) and past one older than one
of those programs needs: the register names the lowest runtime version each
program on a runtime runs on, as its package states it. When none is left, the
runtime counts as missing, and `setup` says which version the computer has.
When one is missing, `setup` lists what it would install and asks for a yes or
no in a terminal; `--with-runtimes` installs it without asking. Without a
terminal nothing is asked, and without `--with-runtimes` nothing at all is
installed then: not the runtime, and not the programs that do not need one.

The exit code is `0` when every program, and every runtime they need, is there
afterwards, and `1` when one is missing, a download or an installation failed,
or the repository has errors. A version that differs from the pinned one is
said and stops nothing.

Not every program has a build for every platform: on Windows there is none of
`dockerfmt`, and on Windows on ARM none of `hadolint`, `jq`, `lychee` and
`trivy`. There the default mode uses the computer's copy, which then has to be
on the `PATH`, and `--pinned` cannot install them: it says so for each and
exits with `1`, even when the computer has them.

### The folder

The pinned copies live in the tool's folder in the user's cache directory:

| Operating system | Folder                                                             |
| ---------------- | ------------------------------------------------------------------ |
| macOS            | `~/Library/Caches/nebensound-repo`                                 |
| Linux            | `$XDG_CACHE_HOME/nebensound-repo`, else `~/.cache/nebensound-repo` |
| Windows          | `%LOCALAPPDATA%\nebensound-repo`                                   |

Inside it, every program and runtime has a folder per version:
`programs/<name>/<version>/` and `runtimes/<name>/<version>/`. An installation
counts once its folder holds the file `.nebensound-installed`, which is written
last; a folder without it is installed again. `downloads/` holds a download
only while it is checked and unpacked.

Nothing outside the folder is changed: no `PATH`, no shell profile, no program
of the computer. To remove everything `setup` installed, delete the folder:

```bash
rm -rf ~/Library/Caches/nebensound-repo                      # macOS
rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/nebensound-repo"    # Linux
```

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\nebensound-repo"   # Windows
```

Before the tool was called `nebensound-repo`, the folder was called
`nebensound`. Nothing reads that folder any more, and it can be deleted the
same way.

A program installed with pip keeps the Python it was installed with. When that
Python is gone, `setup` installs the program again. On Windows, where the
environment holds a launcher that starts that Python, `setup` reads which one
it is from the environment's `pyvenv.cfg`.

### For the other calls

A call that runs a check asks where a program is with
`nebensound_repo::setup::find::locate`: the pinned copy when it exists, else the
one on the computer, together with the runtime it runs on, found the same way. A
runtime on the computer that is too old for the program is not given.

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

## The rules

Markdownlint and yamllint run with a default of the pipeline, which their
blocks lay a repository's own configuration over rule by rule (see
[Linter rules](../../README.md#linter-rules)). `check` shows, per check, every
file that is laid over the default and every rule it sets, with how the rule
stands to the default:

```text
Rules

  markdown                                              Markdownlint: the default, with these laid over it
      .github/.markdownlint-cli2.jsonc                  laid over the default and what lies above it, rule by rule
          MD041                                         false (the pipeline's file does not name it; Markdownlint's own default holds)
  yaml                                                  yamllint: the default, with these laid over it
      .yamllint.yaml                                    laid over the default rule by rule
          comments                                      {"require-starting-space":false} (over the default {"min-spaces-from-content":1})
          line-length                                   "disable" (the same as the default)
```

A rule the repository sets to what the default already gives it is `the same
as the default`; any other one stands `over the default`, with the default's
value. A rule the pipeline's file does not name keeps what lies beneath that
file, which the tool does not compile in: Markdownlint's own default for the
rule, and for yamllint what the file's `extends` sets, yamllint's own default.
The report says so (`the pipeline's file does not name it; Markdownlint's own
default holds`, ``…; what its `extends: default` sets holds``). Markdownlint
rules are matched by any of their names and tags, in any case, so
`line-length` stands over the default's `MD013`; a tag over rules the default
does not give one value says so. For yamllint, a rule's options are laid over
the default's options of that rule, as yamllint does, so writing one option of
a rule the default sets is a difference only where that option differs.

Read are, in every folder, the options file and the file of rules only that
markdownlint-cli2 reads there, and yamllint's configuration at the root.
markdownlint-cli2 reads the files of the folders that hold what it lints and of
the folders above them, so a folder's files are shown only where the markdown
check covers a file in that folder or below it: a folder that an `[[exclude]]`
takes out as a whole shows none. yamllint's configuration is read as yamllint
reads it, as YAML 1.1, so a plain `no` or `off` in it is `false`. A file in
JavaScript is named, not read: phase 0 runs no code of the repository.
What an `extends` names is named, not read. A `.markdownlint.*` file below the
root is shown `in place of everything above it, the default included`, which
is how markdownlint-cli2 reads it; the `config` of a `.markdownlint-cli2.*`
beside it in the same folder is shown as not read, since markdownlint-cli2
takes the file of rules only instead.

## Where the file list comes from

From `git ls-files`, not from a directory walk. What a repository holds is
decided by Git. A walk over the file system would count `target/`,
`node_modules/` and yesterday's half-finished file as well, and phase 0 would
judge things the repository does not have.

`git add` is enough; a commit is not needed, because `ls-files` reads the index.

The versions come from Git as well: the line of `main` (the first-parent line
of `HEAD`, or at a merge into `release` that of the commit of `main` it
merges), the declaration and the files each commit of that line held, and the
branches `origin/release` and `origin/main`. The tags are read only to be held
against that history: no version comes from them. Where a unit lived and how
it was released is read at each commit from its own declaration, so a unit
that moves or switches between one stage and two keeps its earlier releases,
and a declaration written for an edition this tool does not know stops the
call, wherever it stands on the line.
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

`nebensound-repo ui` shows the same tree to walk through. The tree on the left,
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

Every command runs inside `tools/nebensound-repo/`, where the toolchain is
declared.

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
