# nebensound-repo

The tool for the tasks of a repository: it reads a `.nebensound/repo.toml`,
resolves it and checks it against what the repository holds — **phase 0**. The
same binary in development and in CI: two ways would be two answers to one
question.

## Layout

```text
Cargo.toml      the tool, the package at the root of the repository, and the workspace
src/, tests/    its code and its tests
register.toml   the register, compiled in
defaults.toml   what every repository declares without writing it down
rules/          the pipeline's defaults for the linters
run/, schemas/  what `run` hands the programs, compiled in
xtask/          the xtask of this repository, the workspace's other member
```

The package lies at the root, so that the repository unit is the tool: the
version writer `cargo` writes the repository's version into the root
`Cargo.toml`, and the program built from it carries the version of the
pipeline. The workspace has one lockfile, and the tool is its one default
member, so a Cargo command at the root without `--package` or `--workspace`
builds the tool alone. The test cases of the building blocks below
`.github/actions` are Cargo projects of their own and stay out of the
workspace: the root manifest excludes that folder.

The tool is one package, a library and the program: one name, one version. The
part of the library that understands the format deliberately has no input and
no output; it is handed a declaration, the vocabulary and a list of paths, and
answers with a tree. Everything that finds that tree and shows it lives beside
it in the same package.

What only the program needs is the feature `program`, which is on by default
and which the binary requires: the downloading and unpacking of `setup`, the
programs `run` starts, and the JSON Schema of `schema`. Building and installing
the tool take no flag. What builds on the library alone switches the default
features off and compiles none of it.

`rules/` holds the defaults that the CSS, HTML and Markdown checks and
`check-python` lay a repository's own linter configuration over. The tool
compiles the ones of Stylelint, html-validate and Markdownlint in,
both to hand them to the linters in `run` and to show how a repository's rules
stand to them ([The rules](#the-rules)), so both speak of the same files, and a
change to one of them is a change to the tool, which builds it again. The
default of Ruff is compiled in the same way, for the Python check of `run` and
to show it. What else `run` hands the programs
is compiled in as well: `schemas/`, the JSON Schemas the TOML check assigns and
the one the YAML check holds the Dependabot configuration against
([their README](../schemas/README.md)), and the page rules of the HTML
check, html-validate rules in JavaScript that stand in `src/run/html.rs` as
text.

The tool is published to no registry. Each release of this repository carries
its binary for the targets of the build `cargo-release` as assets, built from
the commit of the release and printing its version, so it is the same tool in
the same version, not a second one that can fall behind. The composite action
under `.github/actions/tool` still builds the tool from the source of the
commit a caller pins; fetching the binary of the release instead is #24.

## Calls

```text
nebensound-repo [-C <directory>] [check [--name <name>] [--default-branch <branch>] [--visibility <public|private|internal>] | tree | explain <path> | dependabot | plan [--pull-request <number> --message <message>] | version [--markdown] [--pull-request <number> --message <message>] | files [-z] <check> <unit> | run [--plan <file>] <check> <unit> | build [--plan <file>] <build> <unit> | excluded-packages | write-versions [--plan <file>] [--workspace <path>] | setup [--pinned] [--with-runtimes] [--check] [<check or build> ...] | declare | report-versions | unwritten <checks|build|writers|targets> … | gather <checks|builds> … | xtask | release [--assets <folder>] | schema | register | ui]
```

| Call                | What it does                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `check`             | Runs phase 0 and prints all of it: the resolved configuration, the linter rules laid over the defaults, the tree, the findings (annotations in a workflow run) and the counts. Warns where the name or default branch (`--name`, `--default-branch`, else Git) or a minimum file departs from the conventions. With `--visibility`, how GitHub shows the repository, it holds the declared audience against it, as `declare` does; without it nothing is compared. The default; `declare` runs it.                                                               |
| `tree`              | The annotated tree and nothing else.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `explain <path>`    | What holds for one path and why: for whom the repository is meant, which units contain it, the minimum version of each of them, which commit types count like `feat`, which checks are in force, how its coverage stands.                                                                                                                                                                                                                                                                                                                                        |
| `dependabot`        | The `.github/dependabot.yml` the declaration and the organization's [Dependabot policy](register.md#dependabot-policy) give: one entry per watched ecosystem, listing the folders that hold its manifests. Reads no history, and prints nothing but a sentence when phase 0 finds an error. `check` warns where the file in the tree departs from it ([`stale-dependabot`](declaration.md#stale-dependabot)).                                                                                                                                                    |
| `plan`              | Which unit runs which check and builds what, and which version each unit carries, as one line of JSON for a matrix to read. With `--pull-request <number> --message <message>`, for a pull request, as `version` reads it. See [The plan](#the-plan).                                                                                                                                                                                                                                                                                                            |
| `version`           | The version of every unit that carries one, as a table: current version and the version of the commit at hand. `--markdown` prints it as the job `Version` writes it. See [The versions](#the-versions).                                                                                                                                                                                                                                                                                                                                                         |
| `files`             | The files one check covers in one unit, less what `[[exclude]]` takes out: exactly what its building block checks. One per line, or each ended by a zero byte with `-z`.                                                                                                                                                                                                                                                                                                                                                                                         |
| `run`               | Runs one check over the files `files` names for it in one unit, with the programs `setup` finds or installs, the way its building block runs it: for the checks `css`, `dockerfile`, `html`, `json`, `markdown`, `python`, `rust`, `toml` and `yaml`. The rust check writes the versions of `--plan <file>` before its package test, or, without it, the versions resolved from the history. Every finding names its file, and its line and column where the program gives them; in a workflow run it is an annotation. See [Running a check](#running-a-check). |
| `build`             | Runs one build of one unit the way its building block runs it, for the builds `cargo-package`, `cargo-release` and `docker`, and names the artifact it leaves as outputs of the step. The builds `docker` and `cargo-release` take the unit's version of `--plan <file>`, or, without it, the version resolved from the history: `docker` labels the image with it, `cargo-release` holds the binary's `--version` to it. A unit the plan starts no such build for is refused. See [Running a build](#running-a-build).                                          |
| `excluded-packages` | The Rust packages `[[exclude]]` takes out as a whole, each with its reason, as one line of JSON: what `check-rust` leaves out of every step, and what `declare` names in the summary of the run.                                                                                                                                                                                                                                                                                                                                                                 |
| `write-versions`    | Writes the version of every unit with the version writer `cargo` into its `Cargo.toml`, the path dependencies on it and `Cargo.lock`, in the working tree; never committed. With `--plan <file>`, takes the versions from a printed plan instead of the history; with `--workspace <path>`, starts from the units of the workspace at that path.                                                                                                                                                                                                                 |
| `setup`             | Finds or installs the programs that the checks the plan starts call, or those of the checks and builds named, as the register pins them: by default it uses those on the computer and installs the pinned version of what is missing; `--pinned` installs every one into its own folder; `--check` changes nothing. See [Setting up the programs](#setting-up-the-programs).                                                                                                                                                                                     |
| `declare`           | Phase 0 as the job `Declare` runs it, and the plan and the matrices it hands on as outputs of the step. See [The calls of a workflow run](#the-calls-of-a-workflow-run).                                                                                                                                                                                                                                                                                                                                                                                         |
| `report-versions`   | The versions as the job `Version` reports them: the table into the log and as Markdown into the summary of the run; after a push to a pre-branch the sentence why there are none. See [The calls of a workflow run](#the-calls-of-a-workflow-run).                                                                                                                                                                                                                                                                                                               |
| `unwritten`         | Fails by name for every check, build, version writer or publish target the run asks for that has no building block yet, as the steps `Not written yet` do. See [The calls of a workflow run](#the-calls-of-a-workflow-run).                                                                                                                                                                                                                                                                                                                                      |
| `gather`            | The one result of the checks or of the builds, from the results of the jobs the closing job `Check` or `Build` waits for. See [The calls of a workflow run](#the-calls-of-a-workflow-run).                                                                                                                                                                                                                                                                                                                                                                       |
| `xtask`             | The repository's own checks, `cargo xtask check --all`, with the toolchain the xtask pins, as the last step of `Check repository`. See [The calls of a workflow run](#the-calls-of-a-workflow-run).                                                                                                                                                                                                                                                                                                                                                              |
| `release`           | Writes the records of the versions of the commit at hand, as the job `Publish` does after a push to `main`: for every unit the commit touches, a tag on it and a full GitHub Release, with the archives its build handed on attached first where it publishes to `release-assets` (`--assets <folder>`). Takes the repository, the token and the summary of the run from the variables of a workflow run. See [The release stage](#the-release-stage).                                                                                                           |
| `schema`            | The JSON Schema of `.nebensound/repo.toml`, derived from the types phase 0 reads it with. Reads no repository. The TOML check assigns the copy in `schemas/`, which `run` compiles in; [their README](../schemas/README.md#nebensound-declarationjson) says how to write that copy again.                                                                                                                                                                                                                                                                        |
| `register`          | The register as Markdown, exactly as [`docs/register.md`](register.md) holds it, which a test compares with this output. Reads no repository.                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `ui`                | The same result to walk through. Only in a build with the `ui` feature.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |

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
      "targets": [],
      "version": "0.3.2",
      "writers": []
    },
    {
      "current": "1.5.2",
      "kind": "component",
      "name": "tool",
      "path": "tool",
      "prefix": "tool-v",
      "targets": ["crates.io"],
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
start with, `current` its current version, the highest one the history gives a
commit of the line up to the commit at hand (`null` before the first commit
that touches the unit),
`version` the version of the commit at hand, `null` when the commit does not
touch the unit, `writers` the version writers the unit selects, and `targets`
the publish targets its artifact goes to, `[]` for `publish = false`. `current`
is what the version writer `cargo` requires of a path dependency whose unit
the commit does not touch, where a package that gets a version needs it. The
release stage on `main` releases each unit whose `version` is not `null` in
full under its `prefix`, and `Check` fails for every entry of `targets`
that has no building block. How they are resolved from the history is
described in [the reference of the declaration](declaration.md#versions).

`audience` is for whom the repository is meant, as the declaration at hand
says (see [the reference](declaration.md#audience)), and
`previous_audience` what the declaration of the commit before it on the line
says, `null` when that commit names none or there is none. On a pull request
into `main` that is the tip of `main`, so Declare names a change of the value in
the summary of the run from the two.

`--pull-request <number> --message <message>` builds the plan for a pull
request, reading `HEAD` the way [`version`](#the-versions) does: every `version`
is then the one the squash merge would get, with `-pr.<number>` behind it, such
as `1.5.3-pr.17`, and so is the `current` of a unit the pull request
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

  Unit            Path  Current  This commit
  the repository  .     0.3.2    0.3.2
  component tool  tool  1.5.2    none
```

`Current` is the highest version the history gives a commit of the line up to
the commit at hand, `none` before the first commit that touches the unit;
`This commit` the version of the commit at hand, `none` when the commit does
not touch the unit. `--markdown` prints the same rows
as a Markdown table with a short legend, which the job `Version` writes into
the summary of the run. The repository always carries a version, so the table
is never empty.

`--pull-request <number> --message <message>` reads `HEAD` as the merge commit
GitHub builds for a pull request, which carries a message of GitHub's and
contains the commits of the branch. It stands for the commit the squash merge
would put on the base branch instead: one parent,
the first parent of `HEAD`, the changes of `HEAD` against it, and the message
given, which the job takes from the title and the description of the pull
request. Every version is then the one that merge would get, with
`-pr.<number>` behind it, and so is the current version of a unit the pull
request touches; the column is called `This pull request`. Both options go
together.

For a pull request the call also holds what its merge would publish to a
registry against the records. For every unit the pull request gives a version
that publishes to a target with `dependencies-by-version` in the register, the
units its package requires by path are found as `write-versions` finds them,
followed through, without writing anything. Each one the pull request does not
touch must have a tag `<prefix><current>`; otherwise the error
`unreleased-requirement` names the package, the unit, the version and the way
out, and the call exits with `1`. A manifest it reads that is not TOML ends the
call with exit code `1` and the reason, as with `write-versions`. Without
`--pull-request` nothing is held to it.

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

## The release stage

`nebensound-repo release [--assets <folder>]` is the release stage on `main`,
all of it: the job `Publish` checks the repository out with its whole history,
builds the tool, fetches the artifacts of the run's builds into a folder, each
artifact in a folder of its own named after it, and makes this one call with
that folder. It reads the version of every unit from the history, as phase 0
does, and every unit the commit at hand touches gets its records: a
lightweight tag on the commit, named with the unit's prefix and its version,
and a GitHub Release of it, a full one. The release of the repository is the
latest, a component's never is. A unit the commit does not touch gets nothing.

It works in this order, and writes nothing before it has decided for every
unit:

1. A tag of the version that stands anywhere but on the commit at hand stops
   the call with exit code `1`, named, before anything is written: a version
   belongs to exactly one commit.
2. A newer record of the unit, a tag with a higher version on a commit that
   contains the commit at hand, leaves the unit out with a sentence; the call
   stays green.
3. For every unit to release that publishes to `release-assets` and whose
   build the plan starts, the artifacts in the folder are held against what
   its release needs, and anything else stops the call with exit code `1`,
   each named, before anything is written:
   - every target the register lists for the build has its artifact, named as
     the build names it, `<kind>-<path>@<version>@<commit>@<target>` (a build
     without targets has one, without the last part);
   - every artifact of the unit's build, one whose name starts with
     `<kind>-<path>@`, belongs to the version released and to the commit at
     hand, and is for a target of the build;
   - each is a folder that holds exactly one plain file, a link not followed,
     whose name names its target and holds no control character: the archive;
   - no two archives carry the same name.

   Artifacts of other units and of other kinds are passed over. Without
   `--assets`, or with a folder that does not exist, nothing was handed on,
   and every archive is missing. When the artifacts of a unit are refused,
   the releases are read, still writing nothing: a release published already
   takes no archive, so its refusal stops nothing, and a re-run after the
   run's artifacts expired (the builds keep them for 7 days) leaves it as it
   is. A draft left behind still needs them.

4. For every unit to release the tag, unless it stands on the commit already,
   and the release as a draft titled `<unit> <version>` (the repository by its
   name), with the notes GitHub generates against the previous record, the
   unit's tag with the highest version below on a commit the commit at hand
   contains. A first release carries the sentence "The first release of
   `<unit>`, so there is no earlier release to list the changes against."
   instead. A draft an earlier attempt left is taken up, and a release that is
   published already is left as it is.
5. The publish targets, while the releases are drafts that still take files:
   the archives of `release-assets` attached to the unit's draft, each under
   the name of its file. An asset of that name the draft holds with the same
   SHA-256 is kept; one with another, or whose upload broke off, is deleted
   and the archive uploaded again. An upload that GitHub does not hold under
   that name, uploaded and with that digest, fails the call (an answer without
   a digest as well), and so does any
   call that fails here: no release is published then. Other assets of the
   draft are left as they are.
6. Every draft published, with `prerelease: false` and the mark of the latest
   release in the same call. A published release that is not immutable gets a
   warning.

A tag is a unit's record when what follows the unit's prefix reads as a version
as strictly as phase 0 reads it; any other tag is passed over. No version is
read from a tag.

It reads from the variables of the run `GITHUB_REPOSITORY` (`owner/name`),
`GITHUB_API_URL`, `GITHUB_TOKEN`, the token that may write, which the workflow
hands to this one step, and `GITHUB_STEP_SUMMARY`, the file the summary goes
to. Without one of the first three the call is wrong (exit code `2`). Phase 0
errors release nothing.

It talks to the API through curl, as `setup` downloads: the tool links no HTTP
or TLS library. curl is called as `curl --disable --config -` and reads the
whole request, the token's header among it, from its standard input, so the
token stands on no command line a process list shows and in no log. An archive
is uploaded the same way, curl reading the file named in its configuration, to
the address GitHub gives the draft for its assets (`upload_url`). A request
may use HTTPS only. Every call is made once: a failed call fails the stage, and
a re-run takes up what it left. Everything goes into the log, a finding as an
annotation in a workflow run, and the summary of the run lists every unit with
what it got.

## The calls of a workflow run

A step of the workflow or of a building block is one call of the tool. Five
calls exist for the steps of the workflow that are no check: they read the run
from the variables the step hands them through its `env`, never inside a
script, and from those the runner sets for every step. Each writes its
findings as annotations in a workflow run and as lines elsewhere.

| Variable                                                                | What it says                                                                              |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `NEBENSOUND_PRE_BRANCH`                                                 | `true` after a push to a pre-branch, as the workflow decides it for the whole run.        |
| `NEBENSOUND_PULL_REQUEST`, `NEBENSOUND_TITLE`, `NEBENSOUND_BODY`        | The pull request of the run, empty when there is none. A number that is none is a misuse. |
| `NEBENSOUND_NAME`, `NEBENSOUND_DEFAULT_BRANCH`, `NEBENSOUND_VISIBILITY` | What the event says of the repository.                                                    |
| `GITHUB_OUTPUT`, `GITHUB_STEP_SUMMARY`, `GITHUB_REF_NAME`               | Set by the runner: where the outputs and the summary go, and the branch.                  |
| `GITHUB_API_URL`, `GITHUB_REPOSITORY`, `GITHUB_TOKEN`                   | For `declare` to ask GitHub for the visibility when the event names none.                 |

- **`declare`** runs phase 0 as `check` does, with the name, the default branch
  and the visibility of the event; one the event does not carry is answered by
  the API (`GET /repos/<owner>/<name>`, through curl as the release stage
  calls it, with the read token of the run). When phase 0 passes it prints the
  plan, for the squash merge of the pull request if there is one, names a
  change of the audience on a pull request as a notice and in the summary,
  names every excluded package with its reason as a notice, and prints the two
  matrices: the units that run a check, and the builds of this run, each with
  the version writers its unit selects, the name of its job (`job`) and its
  runner (`runs-on`). A build whose register entry lists targets stands once
  per target, with the target (`target`), the runner the register names for
  it, and the target in brackets behind the name of its job, as in
  `Build repository (x86_64-unknown-linux-musl)`; any other build stands once,
  on `ubuntu-latest`. After a push to a pre-branch none builds. The plan and the two matrices are its outputs `plan`, `units` and
  `builds`, one line of JSON each. With phase 0 errors nothing is handed on.
- **`report-versions`** prints the table of `version` into the log and its
  Markdown into the summary, for the pull request of the run, and fails with
  phase 0 errors. After a push to a pre-branch it reads no repository and says
  why no version is resolved, as a notice and in the summary.
- **`unwritten checks --unit=<path> --planned <json> --written <json>`** and
  **`unwritten build --unit=<path> --planned <build> --written <json>`** fail
  for every check, or for the build, of a unit's matrix entry that the list of
  written blocks does not name; **`unwritten writers`** and
  **`unwritten targets`**, with `--plan <file> --written <json>`, for every
  version writer or publish target the plan names. The steps hand the unit in
  with `=`, so a path that starts with `-` is not read as an option. An empty
  plan, as after phase 0 failed, has nothing to look at. The list of written
  blocks stays in the step, where `cargo xtask check` holds it to the blocks
  that exist.
- **`gather checks --declare <result> --checks <result>`** and
  **`gather builds --declare <result> --check <result> --builds <result> --planned <json>`**
  turn the results GitHub gives the jobs (`success`, `failure`, `cancelled`,
  `skipped`) into the one result a ruleset can require: the checks pass when
  phase 0 passed and every started check passed or none started; the builds
  pass when nothing builds in this run, and otherwise only when the checks and
  every planned build passed.
- **`xtask`** runs `cargo xtask check --all` at the repository root with the
  toolchain the xtask pins: the `rust-toolchain.toml` (or `rust-toolchain`) in
  `xtask/`, else the one at the root, installed through rustup. An xtask that
  pins none fails before anything runs. rustup and Cargo start under
  [the rule for the Rust toolchain](#starting-the-rust-toolchain).

The plan stays an input where a step takes it today: the version writers, the
Rust check, the steps that hold the version writers and publish targets in
`Check`, and the matrices. That every job works out what it needs from the
repository itself follows with the cut of the jobs.

## Running a build

`build <build> <unit>` carries out one build of one unit the way its building
block runs it, in the unit's job of the Build phase, after the version writers
the unit selects: the builds `cargo-package`, `cargo-release` and `docker`. The unit has to be
one the plan starts that build for; a repository with errors builds nothing. As
for `run`, the gate decides, and every call of `build` outside a workflow run
opens with the same line. A temporary folder a build makes lies in the folder
`RUNNER_TEMP` names, else in the system's temporary directory.

`build cargo-package` builds the one Cargo package whose `Cargo.toml` stands at
the unit's path, with the programs on the `PATH` and in the unit's folder, in
these steps, each in a group of the log, up to the first that fails:

1. The toolchain the workspace pins, `rustup toolchain install` and
   `rustc --version`.
2. The package of the unit, from `cargo metadata --locked --no-deps`: the one
   whose manifest is the unit's own `Cargo.toml`. A unit without a
   `Cargo.toml`, or whose `Cargo.toml` is a workspace without a package of its
   own, fails. The artifact is named `crate-<path>@<version>@<sha>`, the path
   percent-encoded as jq's `@uri` writes it and the commit `HEAD`.
3. The local packages it reaches through its dependencies, from `cargo
metadata --locked --all-features`, each patched in from its folder for
   crates.io.
4. `cargo package --locked --allow-dirty --package <name>` with those patches,
   and the SHA-256 of the `.crate` printed.
5. The `.crate` unpacked into a temporary folder outside the source tree and
   tested there with `cargo +<toolchain> test --all-features` and the same
   patches, the toolchain being the one rustup names for the workspace. A
   failure names the two usual causes: a file the package leaves out, a
   dev-dependency without a version.

Then it names the artifact and the file as the outputs `artifact` and `crate`
of the step, for `actions/upload-artifact`.

rustup, rustc and Cargo start under
[the rule for the Rust toolchain](#starting-the-rust-toolchain).

`build cargo-release` builds the one binary of the Cargo package whose
`Cargo.toml` stands at the unit's path, for one target, with the programs on
the `PATH` and in the unit's folder. The register lists the targets of the
build, each with the runner it is built on (see
[Builds](register.md#builds)), and `declare` gives each target a job of its
own on that runner. The tool builds for the one target whose processor and
operating system are those of the machine it runs on: the first part of the
target's name, and Linux for a target with `linux` in its name or macOS for
one that ends in `-apple-darwin`. Only a machine of the target's platform can
start the binary it built. The register holds that rule when it is read: it
refuses a target of any other platform, two targets of one platform, targets
on a build that builds once, and `cargo-release` without targets. The version
is the unit's version of the run, taken from `--plan <file>`, as the block
hands in the plan Declare printed, or, without it, resolved from the history
as `plan` resolves it. Its steps,
each in a group of the log, up to the first that fails:

1. The target of this machine, named in the log; a machine the register lists
   no target for, or more than one, is refused by name. Then the toolchain
   the workspace pins, `rustup toolchain install`, the target added to it with
   `rustup target add <target>`, and `rustc --version`.
2. The binary of the unit, from `cargo metadata --locked --no-deps`: the one
   binary of the package whose manifest is the unit's own `Cargo.toml`. A unit
   without a `Cargo.toml` fails, and so does one whose `Cargo.toml` is a
   workspace without a package of its own, naming the packages in it that
   have a binary, and a package with no binary or with more than one, naming
   them. The plan has to give the unit a version of this run. The artifact is
   named `executable-<path>@<version>@<sha>@<target>`, the path
   percent-encoded as for a crate and the commit `HEAD`.
3. `cargo build --release --locked --target <target> --package <name> --bin
<binary>`, with the package's default features.
4. The binary started with `--version`, under the same rule as the toolchain,
   with what it prints on both outputs in the log: it has to end within 30
   seconds, with success, and print the version of the run as a word of its
   own, on a pull request with `-pr.<number>` behind it.
5. The archive `<binary>-<target>.tar.gz` beside the binary in Cargo's
   `target/<target>/release`, holding the binary (mode `755`) and the licence
   of the unit (mode `644`), each under its own name at the top. The licence is
   every file whose name starts with `LICENSE`, `LICENCE` or `COPYING`, in any
   case, in the first folder from the unit's own up to the repository root
   that holds one; without one the build fails. The same files give the same
   bytes: a tar archive in the ustar format, the binary first and then the
   licence in the order of its file names, each with the modification time 0
   and the owner and group 0 without names, compressed with gzip at the best
   level with neither a time, a file name nor an operating system in its
   header. Its SHA-256 is printed.

Then it names the artifact and the file as the outputs `artifact` and
`archive` of the step, for `actions/upload-artifact`.

The target `x86_64-apple-darwin` is built on GitHub's Intel image of macOS.
GitHub ends its Intel images of macOS in 2027; what happens to the target then
is ruled when it comes.

`build docker` builds the image of the unit's `deploy/Dockerfile`, with the
build context at the repository root, and runs only on an `x86_64` Linux
machine with Docker and its Buildx on the `PATH` and a daemon it reaches: it
builds and starts images for `linux/amd64` on the machine itself and reads
the user of the container from the host's `/proc`. Its version is the unit's
version of the run, taken from `--plan <file>`, as the block hands in the plan
Declare printed, or, without it, resolved from the history as `plan` resolves
it; the title is the unit's name, for the repository itself the name
`GITHUB_REPOSITORY` gives, else that of its folder. Its steps, each in a group
of the log, up to the first that fails:

1. The image of the unit: `deploy/Dockerfile` below the unit's path, which has
   to be there, the version, which the plan has to give, and the commit
   `HEAD`. The artifact is named `container-<path>@<version>@<sha>`, the path
   percent-encoded as for a crate. A machine other than an `x86_64` Linux one
   is refused here by name.
2. A builder of its own: `docker version`, `docker buildx version`, and
   `docker buildx create --driver docker-container` with the BuildKit the
   register pins as the image `buildkit`, by its digest.
3. `docker buildx build` for `linux/amd64` with `--no-cache`,
   `--provenance=false` and `--sbom=false`, the labels
   `org.opencontainers.image.version`, `.revision`, `.source` (in a workflow
   run, from `GITHUB_SERVER_URL` and `GITHUB_REPOSITORY`) and `.title`, into an
   OCI layout, `image.tar` in a folder of the build.
4. The layout unpacked beside the tar and held to its digests: one image
   manifest in `index.json`, and the manifest, every layer and the
   configuration the files their SHA-256 names. The configuration has to be for
   `linux/amd64` and carry the version as its label. The manifest digest goes
   into the log and into the summary of the run.
5. A `HEALTHCHECK` in the configuration, else the image is not started.
6. `docker load` of the tar, and `docker image inspect` of what it loaded: its
   ID has to be the digest of the configuration or of the manifest.
7. A fresh volume (`docker volume create`) at every path the configuration
   declares with `VOLUME`, then `docker create` and `docker start` without
   anything else.
8. `docker inspect` of the container's state, once a second, until it is
   healthy, unhealthy or stopped, for at most five minutes; when it is not
   healthy, its log (`docker logs`) and what its last health check printed.
9. The real, effective and saved user ID of its first process, from
   `/proc/<pid>/status`, taken back into its user namespace through
   `/proc/<pid>/uid_map`; 0 fails.
10. Trivy, the pinned copy in the tool's folder or the one on the computer,
    `trivy image --input <layout>` with `--config ""`, `--ignorefile ""`, a
    cache folder of the build, `--scanners vuln`, `--severity HIGH,CRITICAL`,
    `--ignore-unfixed` and a JSON report; every vulnerability in the report is
    a finding on the Dockerfile.

Then it names the artifact and the file as the outputs `artifact` and `image`
of the step, for `actions/upload-artifact`. Whatever happened, a last group
removes what the build started: the container, its volumes, the loaded image,
the builder, and every file of the build's folder but `image.tar`, which goes
as well when the build failed. Docker, its Buildx and Trivy start under
[the rule for Docker and Trivy](#starting-docker-and-trivy).

Trivy's database of vulnerabilities is not part of the tool or the register.
Trivy fetches it in every run into the build's cache folder, from where it
fetches it by default: the OCI artifact `trivy-db` from `mirror.gcr.io`, else
from `ghcr.io`, under the tag of the database schema it reads. It is checked
only as an OCI artifact is pulled, against the digests of the manifest the
registry serves for that tag at that moment; nothing pins it, and no signature
of it is checked. An image that holds Java archives makes Trivy fetch its Java
database, `trivy-java-db`, the same way.

## Starting the Rust toolchain

Wherever the tool starts the Rust toolchain (rustup, Cargo, rustc, rustfmt,
and with them what Cargo starts), it does so under one rule, and one place in
the tool builds every such command: in [the Rust check](#the-rust-check), in
[the builds `cargo-package` and `cargo-release`](#running-a-build) and in
[`xtask`](#the-calls-of-a-workflow-run). Every such program runs without the
variables of the computer whose names start with `CARGO` or `RUST`, other than
`CARGO_HOME` and `RUSTUP_HOME`, which only say where Cargo and the toolchains
are: so no setting that comes as a variable changes what a check finds or what
a build hands on. A variable the tool sets itself for a call, such as the
toolchain it names in `RUSTUP_TOOLCHAIN`, is set after the others are taken
out. Settings that come as files still apply, and the tool does not hunt them
down: a directory override rustup keeps in its settings file
(`rustup override set`), which ranks above `rust-toolchain.toml`, and Cargo's
configuration files in the folders above the workspace and in `CARGO_HOME`. A
hosted runner has neither, so they are ways a local result can differ from the
gate's, which is the one that counts.

## Starting Docker and Trivy

The image build starts Docker, its Buildx and Trivy under one rule, the same
kind as the one for the Rust toolchain: every such program runs without the
variables of the computer whose names start with `DOCKER_`, `BUILDX_`,
`BUILDKIT_` or `TRIVY_`, and without `SOURCE_DATE_EPOCH`, which Buildx hands
the build. Kept are `DOCKER_HOST`, `DOCKER_CONTEXT`, `DOCKER_CONFIG`,
`DOCKER_CERT_PATH`, `DOCKER_TLS`, `DOCKER_TLS_VERIFY`, `DOCKER_API_VERSION` and
`BUILDX_CONFIG`, which only say where the daemon and the files of Docker and
Buildx are and how to reach the daemon; the build names the builder it starts,
so no builder chosen in Buildx's files is used. So no setting that comes as a
variable, such as `DOCKER_DEFAULT_PLATFORM`, `BUILDX_GIT_LABELS` or
`TRIVY_SKIP_DIRS`, changes what the build hands on or what the scan finds.
Settings that come as files still apply, and the tool does not hunt them down:
Docker's `config.json`, which can hand a build proxies, and the daemon's own
configuration. A hosted runner has neither, so they are ways a local result can
differ from the gate's, which is the one that counts.

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
`ignores` of markdownlint-cli2, `exclude_path` of lychee,
`ignore` of rustfmt, `include`, `ignore` and `exclude` of Pyright, `omit` of
coverage.py, `exclude`, `extend-exclude`, `include`, `lint.exclude` and
`format.exclude` of Ruff, through its `extend` as well, a Python file's
`# ruff: noqa`, `.stylelintignore` and `ignoreFiles` of Stylelint,
`.htmlvalidateignore` of html-validate — and fails on every file one of them
takes out that no `[[exclude]]` entry does (`unexcused-ignore`). The checks
read none of them except lychee's `exclude_path`, which `run markdown` and
`run html` apply themselves, matched against the path from the root as phase 0
matches it, and rustfmt's `ignore`, which takes effect on a nightly toolchain,
and Ruff's `# ruff: noqa`, which the file holds itself; run by hand, the tools
read them all.

## Setting up the programs

The checks call external programs: formatters, linters, a link checker; the
image build calls a scanner. The register pins each of them to one version, with
a download per operating system and architecture and its digest, and names the
runtime a program written in JavaScript or Python needs, with the lowest version
of it the program runs on ([Programs](register.md#programs)). `setup`
gets a computer ready to run the checks of a repository:

```bash
nebensound-repo setup                  # use what the computer has, install what it lacks
nebensound-repo setup --pinned         # install the pinned version of every program
nebensound-repo setup --with-runtimes  # the same, and a missing runtime without asking
nebensound-repo setup --check          # change nothing; say what is missing or differs
nebensound-repo setup --pinned toml    # only the programs of the check toml
nebensound-repo setup --pinned docker  # only the programs of the build docker
```

It sets up the programs of the checks the plan starts, or of the checks and
builds it is named, as the register names them (a name that names a check and a
build, such as `typst`, sets up both); a building block names its own
check or build, so that it installs only what that calls: `build-docker` calls
`setup --pinned docker` for Trivy. It reads the repository like
`plan` does, and a repository with errors gets nothing set up,
for the same reason it gets no plan. It does not read the history, though, like
`files` and `excluded-packages`: which checks the plan starts never depends on a
version, so a shallow clone or an error about a version does not keep anyone
from setting up the programs.

- **By default** it uses a program that is on the computer already, found on
  the `PATH`, and says where its version differs from the pinned one. It
  installs the pinned version of what is missing, and nothing that is present.
- **`--pinned`** installs the pinned version of every program into its own
  folder, whatever the computer has, and leaves the computer's programs alone.
  This is the mode the gate runs in: the blocks of the checks that run from the
  tool call `setup --pinned --with-runtimes <check>`.
- **`--check`** changes nothing. It lists every program, and every runtime the
  default mode would need, with where it was found, what is missing, and which
  version differs from the pinned one.

A download is refused unless it has the digest the register pins, and nothing
of it is used then. Downloads go through `curl`, which macOS, Linux and Windows
bring; a program on a runtime is installed by that runtime's installer: npm
with `--ignore-scripts`, or pip into a virtual environment of its own, taking
only wheels, so that no build code of a dependency runs. The installer gets the
program's companions with it, the packages the register pins beside it that
are no command (Stylelint's: the configurations the pipeline's default extends
and the syntax it reads SCSS with), each held against its digest as well.

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
`dockerfmt`, and on Windows on ARM none of `hadolint`, `lychee` and `trivy`.
There the default mode uses the computer's copy, which then has to be on the
`PATH`, and `--pinned` cannot install them: it says so for each and
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

## Running a check

`run` carries out the checks for CSS, Dockerfiles, HTML, JSON, Markdown,
Python, Rust, TOML and YAML the way their building blocks run them, on any
computer the programs run on:

```bash
nebensound-repo setup --pinned --with-runtimes   # once, or after a change of the register
nebensound-repo run markdown .                   # the Markdown check of the repository
```

The gate decides: what counts is the result of the run on GitHub Actions. A
run anywhere else comes as close to it as is sensible but can differ, so every
call of `run` outside a workflow run opens with a line that says so, also one
that ends in a refusal or an error before the check starts: `Not running in
GitHub Actions, so the result can differ from the gate's, which is the one that
counts.` A workflow run's log does not get that line.

It takes exactly the files `files` names for the check and the unit, and a
pair the plan does not start is refused, as there. A repository with errors
gets nothing checked. It starts the programs the register names for the check,
found as [for the other calls](#for-the-other-calls), with the flags that set
their own ignore mechanisms aside, at the repository root, where the
repository keeps their configuration. It installs nothing: a program that is
neither pinned nor on the computer fails the check and names `setup`, and the
parts of the check whose programs are there still run. Three
programs are run through their internals, so that the repository's rules are
laid over the pipeline's default ([The rules](#the-rules)): html-validate,
markdownlint, a library with no command, and Stylelint, on Node.js. Of those
three only the pinned copy is taken, since only its internals are known, and
the check fails with a pointer to `setup --pinned` otherwise. markdownlint gets
the files and, for each group of files with the same rules, the configuration
the tool reads ([The configuration of a linter](#the-configuration-of-a-linter)),
and reads nothing else. What the tool judges itself starts no program: whether
a JSON, JSONC or YAML file parses, which a missing program never keeps from
running, and whether the Dependabot configuration matches the schema compiled
in. Stylelint
gets, for each stylesheet, the configuration nearest to it as the last of what
its configuration extends, after the default, with `ignoreFiles` set aside and
an empty ignore file in place of `.stylelintignore`, and finds only the pinned
packages beside it. html-validate gets, for each page, the `.htmlvalidate.*`
files of its folder and the folders above it, up to the repository root, laid
over the default, and the page rules over both, written into the folder of what
`run` hands the programs and loaded from there; a flat configuration
(`html-validate.config.*`) at the root fails the check. The link check of the
HTML check resolves a link from the root of a site, such as `/login`, below an
empty folder, and a second configuration behind the one the tool writes
leaves out every address below it, written with the folder's path without
links. lychee, the link check of the Markdown and the HTML check, is the one
program that does not start at the repository root: it starts in the folder of
what `run` hands the programs, with the configuration the tool writes from the
repository's `lychee.toml` and `.lycheeignore` and the files as absolute
paths, so that it reads nothing of the repository's configuration itself. The
keys the tool carries over and the ones it refuses by name stand in
[`src/lychee_config.rs`](../src/lychee_config.rs). actionlint uses the
ShellCheck and pyflakes on the `PATH`, where there are any, and the output says
whether it found them. Hadolint gets the repository's `.hadolint.yaml` or
`.hadolint.yml` at the root, or an empty configuration, and none of the
`HADOLINT_*` variables of the computer; `run` reads the failure threshold from
what Hadolint prints with `--verbose` and judges every finding of its JSON
report against it.

Every part of a check runs even when one before it failed, so one run reports
every finding; the Rust check is the exception ([below](#the-rust-check)), and
the Python check leaves out what needs a step that failed
([below](#the-python-check)). What the programs print stands in the output. A
finding names
its file, and its line and column where the program gives them: in a workflow
run (`GITHUB_ACTIONS=true` without a terminal) as an annotation, elsewhere as a
line such as `README.md:3: error: MD001/heading-increment …`. A finding that
fails nothing, such as a Stylelint or html-validate warning, a Hadolint
finding below the failure threshold or dockerfmt's warning about a setting of `.editorconfig` it
cannot use, is a warning: `::warning` in a workflow run, `Dockerfile:3:1: warning: …` elsewhere.
In an annotation the file name and the message are escaped the way the runner reads
them, so a name holding `,` or `:` points at its file. Every annotation of the
tool, those of phase 0 included, writes its `file=` in the runner's own
escapes (`%25`, `%0D`, `%0A`, `%3A`, `%2C`), so a name with a line break
points at its file too. The runner cannot be handed three kinds of name as
they stand, and none of them starts a command: a name that starts with `=`
(the runner drops the `=`, so the annotation sits on the name without it), a name
that ends in white space when `file=` comes last (the runner trims it), and
a name with a control character it has no escape for, which the tool writes
as `\t` or `\u{1b}`, as the report shows it (the annotation sits on no file).
A finding of phase 0 names the file in its message as well; a check's finding
does where the program's message does.

Only the tool writes workflow commands. In a workflow run everything else it
prints, what the programs print and every text from the repository, stands in
a stretch in which the runner processes no command: the tool writes
`::stop-commands::<token>` before it and `::<token>::` after it, with a token
of 16 random bytes from the operating system for each stretch, which no
program it starts is given and which the runner masks in the log. An
annotation waits until the tool writes its next other command, such as the
end of a group, or until the call ends, so that what a program printed stands
as one stretch and its findings follow it. No program's own annotation is
passed through: the scripts it runs for Stylelint and html-validate hand
theirs over as data, one JSON object per line of a file, which the tool
writes; the script for markdownlint writes what markdownlint's library returns,
which the tool reads and writes, a line such as
`README.md:3 error MD001/heading-increment …` and a finding on its line. This holds
for every call in a workflow run, so `plan`, `files` and the other calls that
print data print it inside such a stretch there too. Outside a workflow run
nothing is a command, and text goes out as it is. Prettier names
no position for a file it finds unformatted, and the schema check of the
Dependabot configuration a JSON path instead of a line; there the finding
names the file alone.

### The Rust check

`run rust` checks the Cargo workspace at the unit's path, and unlike the other
checks it runs code of the repository. It runs these steps in this order, each
in a group of the log (`::group::` in a workflow run, its name elsewhere), up to
the first that fails, which it names below the group as `<step>: failed`:

1. Install the toolchain the workspace pins: `rustup toolchain install` from its
   `rust-toolchain.toml`, with rustfmt and Clippy.
2. Find the packages: `cargo metadata --locked` with all features, each local
   package held against the packages `[[exclude]]` takes out as a whole
   ([Excluded packages](#excluded-packages)). A member that is not out is
   checked; a path dependency that is no member and not out is formatted only;
   what is out is named with its reason and left out of every step.
3. Formatting: `cargo fmt --package` for the members, rustfmt for the other
   path dependencies, one call per edition, all with `--check` and
   `disable_all_formatting=false`. A file rustfmt names is a finding on it;
   the differences follow.
4. Clippy with all features and with no default features, `--no-deps` and
   `-D warnings`.
5. Tests with all features, the doc tests included, and with no default
   features.
6. Documentation with `RUSTDOCFLAGS=-D warnings`.
7. A release build.
8. The oldest `rust-version` a checked package claims, compared as numbers,
   installed with the minimal profile, and the workspace built with it.
9. Coverage: the nightly toolchain the register pins for the check
   (`coverage-toolchain`), installed with `llvm-tools-preview`, and
   cargo-llvm-cov as the register pins it, found as for every other program,
   at 100% of regions, lines and functions. An excluded package's folder is
   left uncounted, or its own `.rs` files one by one where another package
   lies inside it.
10. The package test: the versions written as `write-versions --workspace
<unit>` writes them, from the plan `--plan` names or, without it, resolved
    from the history; every published package that is checked packaged in one
    `cargo package`, each copied out of the workspace and tested there with its
    siblings' copies and the other local packages it reaches in place of
    crates.io. Every `Cargo.toml` and `Cargo.lock` Git tracks is put back as
    it was, byte for byte, so a developer's working tree keeps what it held.

A workspace whose members are all excluded is only formatted. Every program
runs in the workspace's folder, under
[the rule for the Rust toolchain](#starting-the-rust-toolchain). What the
programs print stands in the output as it comes; the one finding with a file
is the formatting's.

### The Python check

`run python` checks the uv project at the unit's path, and like the Rust check
it runs code of the repository. It runs these steps in this order, each in a
group of the log, and names every step that fails below its group as `<step>:
failed`. Every step runs even when one before it failed, so one run reports
every finding; only the first two end the check when they fail, and the steps
from Pyright on need the project installed:

1. Find uv, Ruff, Pyright and coverage.py: the first three as the register
   pins them and `setup` finds them, as for every other check; coverage.py as
   the wheel the register pins, fetched into the check's folder and held to its
   digest, since it is laid over the project's environment and not started as a
   program of its own.
2. Install the project's Python: the `uv.lock` at the unit or the nearest folder
   above it, the `.python-version` at the unit or beside that lockfile, the
   interpreter it names installed with `uv python install`, which has to be one
   coverage.py runs on, and Ruff's configuration for the unit laid over the
   pipeline's default ([The rules](#the-rules)).
3. Install the project: `uv sync --locked`.
4. Formatting: `ruff format --check`, in the folder of Ruff's configuration,
   with exactly the files of the list and `--no-force-exclude`.
5. Ruff: `ruff check --no-fix` the same way.
6. Pyright in strict mode, with the repository's configuration laid over it,
   its `ignore` and `exclude` set aside and exactly the files of the list
   handed over by name; a lower mode in its configuration or in a file, and a
   `# type: ignore` without rules before a file's first statement, are
   findings of their own. Where Pyright analysed fewer files than it was
   handed, as it does with a file in a folder whose name starts with a dot,
   each file is run alone, and each one Pyright leaves out is a finding.
7. The tests: the project's pytest, run by coverage.py laid over the project's
   environment with `uv run --with`, measuring lines and branches below the
   unit; a test that fails is a finding on its file and line.
8. Coverage: every line and branch of every `.py` file of the list.
9. The wheel: `uv build --wheel` with the placeholder version `0.0.0`, written
   into `pyproject.toml` for the build and put back afterwards, installed with
   the locked runtime dependencies into a clean environment, where each
   command answers `--help` within 60 seconds and each package imports.

Python itself runs in three places, each because only the interpreter can
answer: whether pytest imports from the project's environment, whether a file
coverage.py did not measure holds a statement, as Python's own parser reads it,
and whether a package of the wheel imports.

uv keeps its cache, the interpreter, the links it would put on a `PATH`, the
project's environment and the clean one in the check's folder in the system's
temporary directory, which `run` removes again; it takes only a Python it
downloads itself, and prints no progress bars. Every program runs without
`PYTHONPATH`, `PYTHONHOME`, `PYTHONSTARTUP`, `VIRTUAL_ENV` and `CONDA_PREFIX`,
and with `PYTHONDONTWRITEBYTECODE=1`. Other settings still apply, and `run`
does not hunt them down: the variables of uv it does not set, such as an index,
and uv's configuration files in the user's folders. A hosted runner has none,
so they are ways a local result can differ from the gate's.

### What it hands the programs, and its exit code

What `run` hands the programs (an empty ignore file, the list of files, the
configurations, the schemas with their own location, the page rules) goes into a folder of its
own in the system's temporary directory, which it removes again. The exit code
is `0` when nothing was found, `1` when something was or a program is missing,
and `2` for a check `run` does not carry out.

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

Markdownlint, Stylelint, html-validate and Ruff run with a default of
the pipeline, which their blocks lay a repository's own configuration over rule
by rule (see
[Linter rules](../README.md#linter-rules)). `check` shows, per check, every
file that is laid over the default and every rule it sets, with how the rule
stands to the default:

```text
Rules

  markdown                                              Markdownlint: the default, with these laid over it
      .markdownlint-cli2.jsonc                          laid over the default rule by rule; it names no rule
      .markdownlint-cli2.jsonc                          for `.github/**`: laid over the default and the rules above, rule by rule
          MD041                                         false (the pipeline's file does not name it; Markdownlint's own default holds)
```

A rule the repository sets to what the default already gives it is `the same
as the default`; any other one stands `over the default`, with the default's
value. A rule the pipeline's file does not name keeps what lies beneath that
file, which the tool does not compile in: Markdownlint's own default for the
rule, and for Stylelint and html-validate what the file's `extends` sets.
The report says so (`the pipeline's file does not name it; Markdownlint's own
default holds`, ``…; what its `extends: stylelint-config-standard` sets
holds``). Markdownlint rules are matched by any of their names and tags, in any
case, so `line-length` stands over the default's `MD013`; a tag over rules the
default does not give one value says so. The defaults of Stylelint and
html-validate name no rule of their own, only what they extend (`stylelint-config-standard`; html-validate's presets
`recommended`, `a11y` and `prettier`), so every rule a repository sets for them
is shown as one the pipeline's file does not name.

Read are, for Markdownlint, the one file the Markdown check reads
([The configuration of a linter](#the-configuration-of-a-linter)), with each
difference by pattern shown under it. The Markdown check prints the same lines
for Markdownlint before it lints, so what the report shows is what runs. Every
file of Markdownlint's configuration that is not read and every key that is not
handed over is shown at its file, with why. A file in JavaScript is named, not
read: phase 0 runs no code of the repository. What an `extends` names is named,
not read.

Stylelint and html-validate are shown only where their check covers a file.
For Stylelint, the configuration nearest to each stylesheet is read: the first
of the names Stylelint looks for in its folder or the nearest folder above it,
`package.json` only where it holds a `stylelint` key. Below the root it is
shown `in place of any configuration above it`, as the block lays only the
nearest one over the default. For html-validate, every `.htmlvalidate.*` of a
folder that holds a page or holds one below it is read, each laid over the
default and what lies above it, up to a folder whose `.htmlvalidate.json` says
`root: true`: that folder's files are shown `in place of any configuration
above it`, and one above it is shown only where a page outside that folder
reaches it.
A `root` in a JavaScript file is not read.

Ruff is shown only where the python check covers a file, too. Read is, for
each unit the check runs at, the configuration `check-python` lays over the
default: the first of `.ruff.toml`, `ruff.toml` and a `pyproject.toml` with a
`[tool.ruff]` table in the unit's folder or the nearest folder above it, and
every file its `extend` names, each shown beneath the one that names it:

```text
  python                                                Ruff: the default, with these laid over it
      pyproject.toml                                    laid over the default rule by rule
          D100                                          off by `lint.ignore` (the default's `ignore` does not name it; its `select` is ["ALL"])
          CPY001                                        on by `lint.extend-select` (over the default, whose `ignore` names it)
          T201                                          off in "scripts/*" by `lint.per-file-ignores` (the default names no such pattern)
```

Ruff's rules are no map from rule to value: `select`, `extend-select`,
`ignore`, `extend-ignore`, `per-file-ignores` and `extend-per-file-ignores`
name rules and prefixes of them (`D`, `PLR2`, `ALL`), which Ruff weighs by how
specific each one is. The report does not resolve a prefix into the rules it
stands for: each entry is shown as the file writes it, and compared with the
same entry of the default, so `D` is `on` or `off` as written, with whether the
default's `select` or `ignore` names `D` itself. An `extend-select` entry
switches back on what the default's `ignore` names under it, so the report
names those entries too: `ALL` names every entry of that `ignore`, and `E`
names `E111`, `E114` and `E117`. An entry names the codes it begins only where
Ruff's linter prefix ends, so `E` does not name `ERA001` and `PT` does not
name `PTH123`; `PL` names pylint's `PLC`, `PLE`, `PLR` and `PLW`. A `select`
is shown as one list, since it starts over: the `select` and `ignore` beneath it, the
default's `select = ["ALL"]` included, no longer hold. A `per-file-ignores`
starts over as well, and the report names the default's patterns that no
longer hold. The settings are read in the `lint` table, or at the top where
`lint` does not name one. An `extend` that leaves the repository or that Ruff
would expand (`~`, `$`) is named, not read.

## The configuration of a linter

A reading check starts no code of the repository, so a linter under this rule
never looks for its configuration itself: the tool reads it, in one place
([`src/linter_config.rs`](../src/linter_config.rs)), and hands the linter the
configuration it wrote. The rules report and phase 0 read through the same
place, so what the report shows is what runs. markdownlint is the one linter
under the rule today; the link check's lychee gets its configuration from the
tool the same way ([`src/lychee_config.rs`](../src/lychee_config.rs)). The
others still find and read the repository's configuration themselves
([what does not hold yet](principles.md#what-does-not-hold-yet)).

1. **A repository's file is optional.** Without one, the pipeline's default
   holds. With one, its rules are laid over the default's rule by rule, and the
   rules report shows every difference, a rule switched off or loosened as well
   ([The rules](#the-rules)).
2. **One file per linter, at the root of the repository**, under one of the
   names the register gives the program, in a format of data, so that an editor
   reads the same file ([the register](register.md#configuration-of-the-programs)).
   The tool reads the first of them that stands at the root. A difference for a
   folder or a unit is written in that file, as a pattern.
3. **The tool carries over only what it knows**: rules, options that are data,
   and differences by pattern. Everything else fails the check by name: a key it
   does not know, a key that names code or another file, and a file it does not
   read. Phase 0 names every file it does not read as well
   (`unread-configuration`): another of the names at the root, one in a folder
   below it, and one written in JavaScript, wherever it lies. A file an
   `[[exclude]]` entry takes out is not named.

For markdownlint the file is an options file of markdownlint-cli2
(`.markdownlint-cli2.jsonc` or `.markdownlint-cli2.yaml`), whose rules stand
under `config`, or a file of rules only (`.markdownlint.jsonc`, `.json`,
`.yaml` or `.yml`). The keys the tool carries over, sets aside and refuses by
name stand in
[`src/linter_config/markdownlint.rs`](../src/linter_config/markdownlint.rs):

- Carried over: `config`, `overrides`, `frontMatter` and `noInlineConfig`.
  Every key of the rules is a rule, an alias or a tag of the pinned
  markdownlint, in any case, or `default`. `frontMatter` is a pattern of
  JavaScript; one that JavaScript does not read fails the check at its file,
  and markdownlint skips the front matter it knows by default. Empty text is
  no pattern and leaves that default in place, as with markdownlint-cli2.
- Set aside, since the check decides them itself: `globs`, `ignores` and
  `gitignore` (the files are those `files` names, and phase 0 holds `ignores`
  against the declaration, see [Exclusions](../README.md#exclusions)),
  `noBanner`, `noProgress`, `showFound` and `$schema`.
- Refused by name: `customRules`, `markdownItPlugins`, `outputFormatters` and
  `modulePaths`, which name code, `fix`, which would write the files, and
  `extends` in the rules, which names another file.

A difference by pattern is an entry of `overrides`, as markdownlint-cli2 writes
it:

```jsonc
{
  "overrides": [
    {
      "filter": [".github/**"],
      "combine": "merge",
      "config": { "MD041": false },
    },
  ],
}
```

`filter` holds patterns over the path from the repository root, where `*` stops
at a `/`, `**` does not, a leading `!` takes a match back, and a leading `./`
names the root and is taken off; the first entry whose `filter` matches a file
counts for it. `merge` lays its rules over the file's own, `replace` takes
their place; the pipeline's default lies beneath either way. markdownlint's
library knows no patterns, so the tool works out the rules of each file and
hands the files in groups of the same rules.

markdownlint reads the keys of the rules in order, and where two keys cover the
same rule, such as a tag and one of its rules, the later one counts. So the
tool keeps the keys in the order the file writes them, in JSONC as in YAML, and
hands them on in that order. It lays the layers as markdownlint-cli2 laid them,
with JavaScript's spread of one object over another: first the default's keys
in their order, then the file's, then those of the entry of `overrides` that
counts for the file (over the default alone for `replace`). A key that is
already there takes the new value in its place; a new key comes after the
others, in the order written. With `headings: false` and then `MD025: true`,
`MD025` holds; with `single-title: true` and then `headings: false`, it does
not.

## Where the file list comes from

From `git ls-files`, not from a directory walk. What a repository holds is
decided by Git. A walk over the file system would count `target/`,
`node_modules/` and yesterday's half-finished file as well, and phase 0 would
judge things the repository does not have.

`git add` is enough; a commit is not needed, because `ls-files` reads the index.

The versions come from Git as well: the line of `main` (the first-parent line
of `HEAD`) and the declaration and the files each commit of that line held.
The tags are read only to be held against that history: no version comes from
them. Where a unit lived is read at each commit from its own declaration, so a
unit that moves keeps its earlier releases, and a declaration written for an
edition this tool does not know stops the call, wherever it stands on the line.
A repository without a commit has no version yet; a shallow clone is an error,
because the history it lacks could hold the last release. Every option the person's Git configuration could change, such as
rename detection, is given on the command line, so the answer does not depend
on who runs it.

Unlike the file list, the versions need a commit. They are resolved for `HEAD`,
and each commit is read with the declaration it holds, so a change to
`.nebensound/repo.toml` that is not committed yet reaches them only in part; see
[what the history must hold](declaration.md#what-the-history-must-hold).

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
cargo run --features ui -- -C . ui            # on this repository
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

Every command runs at the root of the repository, where the toolchain is
declared.

```bash
cargo run --quiet -- -C . check    # phase 0 on this repository itself
cargo run --quiet -- -C . plan     # and the plan a run would start

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

Together these are the gate for the tool and the xtask: formatting, Clippy
with `-D warnings` in both builds, the tests in both builds, the doc tests,
rustdoc and full coverage of lines, regions and functions in both builds.
`--workspace` takes in both packages, as the Rust check of the repository does.

Raw mode, the alternate screen and the blocking read live in `src/terminal.rs`
and are checked from inside a **pseudo terminal** (`tests/pty.rs`) — the same
path a person takes. No line needs an exemption from coverage.
