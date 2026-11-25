# pipeline

Nebensound's central pipeline. A repository describes itself once, in
`.nebensound/repo.toml`, and calls the pipeline; the pipeline checks, builds and
publishes what that declaration says.

## What it is

- **One reusable workflow**, which a repository calls.
- **Its building blocks.** Each block performs one step and is named after its
  entry in the register, with the category in front: `version-<writer>`,
  `check-<name>`, `build-<name>`, `publish-<target>`.
- **The tool `nebensound-repo`.** It reads a repository's declaration, checks
  it against the tree and tells the workflow which blocks to run for which part
  of the repository.

The commands of `nebensound-repo` are limited to the tasks of a repository. The
tool has no external subcommands and never starts a program because a
subcommand's name matched it. Each program it starts is one of these, started
only by the calls named beside it:

| What                                                                                         | Who may start it                                                                                                 |
| -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Git, curl                                                                                    | every call of the tool                                                                                           |
| A program of the register                                                                    | only the check or build the register names it for                                                                |
| The Rust toolchain that rustup provides (rustup, Cargo, rustc, rustfmt)                      | only the check for Rust and a build of Rust (and `nebensound-repo xtask`, which runs the repository's own xtask) |
| The Python that uv provides                                                                  | only the check for Python                                                                                        |
| Docker with its Buildx                                                                       | only the image build                                                                                             |
| Code of the repository (its tests, its build scripts, the commands of the package it builds) | only executing checks and builds                                                                                 |

A reading check starts no toolchain of a language and no code of the
repository. uv is pinned in the register; rustup and Docker come from the
machine, on GitHub Actions from the runner image.

## Status

This is a prototype. Releases are on `0.0.x`. The code arrives piece by piece;
the open issues and milestones show what comes next.

## The tool on a computer

Every release of this repository carries the tool as a binary, one archive per
system, holding the binary and the licence: `nebensound-repo-<target>.tar.gz`,
with `<target>` one of `x86_64-unknown-linux-musl` (Linux on x86_64, linked
statically), `aarch64-apple-darwin` and `x86_64-apple-darwin`. The archive of
the newest release has a fixed address:

```text
https://github.com/Nebensound/pipeline/releases/latest/download/nebensound-repo-<target>.tar.gz
```

To verify a download against the attestation of its release, name the release
it came from and check it with gh 2.93.0 or later, logged in:

```sh
tag=v0.0.79
target=aarch64-apple-darwin
curl -fsSLO "https://github.com/Nebensound/pipeline/releases/download/$tag/nebensound-repo-$target.tar.gz"
gh release verify-asset "$tag" "nebensound-repo-$target.tar.gz" --repo Nebensound/pipeline
tar -xzf "nebensound-repo-$target.tar.gz" nebensound-repo
./nebensound-repo --version
```

Verify against the tag, not against the newest release: the newest can change
between the download and the check. The check proves that the file is an asset
of exactly that release and that its tag stood on its commit, not what the
binary was built from. The binary prints the version of the pipeline it
belongs to. On macOS it carries only the linker's ad-hoc signature: curl and gh
set no quarantine flag, so it starts, while one downloaded with a browser does
not until the flag is removed.

## Reference

- [The principles](docs/principles.md): the six rules every change is held
  against, what the pipeline protects, and what does not hold yet.
- [The declaration](docs/declaration.md): every field of
  `.nebensound/repo.toml`, its default and effect, and every finding phase 0
  reports.
- [The register](docs/register.md): the presets, checks, builds, publish
  targets, version writers and ecosystems a declaration can name, generated
  from the register compiled into the tool.
- [The phases](docs/phases.md): what runs, under which name, on a pull request
  and on a push.
- [Calling the workflow](docs/calling.md): how to pin, when the caller
  starts the workflow, which permissions it grants and why, why the
  schema line can run ahead of the pin, and how to change one linter rule in
  one place.
- [The tool](docs/tool.md): every call of `nebensound-repo`, the package at the
  root of this repository, and how to build and run it.

## Calling the pipeline

A repository describes itself in `.nebensound/repo.toml` and adds one caller
file, for example `.github/workflows/gate.yml`. This is the whole file:

```yaml
name: gate

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

permissions:
  contents: read

jobs:
  nebensound:
    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@<release tag>
    permissions:
      contents: write
      pull-requests: read
      id-token: write
      packages: write
      attestations: write
      deployments: write
```

One job, no inputs: what the repository is stands in its declaration, and the
workflow reads it there. The reference is pinned to an exact version, so that a
change to the pipeline never reaches a repository as a silent switch: the tag of
a release of this repository, `vX.Y.Z`, which names one commit for good. A
caller pinned to the full SHA of a commit keeps working as it is (see
[Calling the workflow](docs/calling.md#pinning)).

`nebensound-repo init` writes the first draft of a repository's switch: the
declaration from what it recognises in the tree, this file as
`.github/workflows/nebensound.yml`, named `nebensound` and pinned to the tag of
the release the tool belongs to, and the Dependabot configuration. It writes
each file only where it is missing and marks every decision it cannot take with
`TODO(init)`, which `check` reports until it is taken (see
[the tool](docs/tool.md#switching-a-repository-to-the-pipeline)).

The file already holds what the parts planned after `v0.1.0` need, pre-branches
among them, so that they need no change to it. It starts the workflow on every
pull request, also when its title or description is edited, since the version
shown is read from them, and on every push to `main`, `pre` and `pre-*`. A
push to a pre-branch is checked as one to `main` is, but versions and
pre-releases of pre-branches are not built yet: such a run shows no version,
builds nothing and publishes nothing, and says so (see
[the phases](docs/phases.md#pull-request-and-push)).

The job grants `contents: write`, `pull-requests: read`, `id-token: write`,
`packages: write`, `attestations: write` and `deployments: write`: what the
planned parts of the pipeline need, not only what today's jobs ask for. Today
only the job `Publish` asks for more than a read token, `contents: write` to
write tags and releases after a push to `main`; every other job, and every job
of a pull request, keeps a read token. Why the caller grants more is in
[Calling the workflow](docs/calling.md#permissions).

A run reports as `nebensound / Declare` (the preparing job: phase 0, the
declaration read and checked against the tree, and the version of every unit,
as a table in the summary of the run; when it fails, nothing after it starts),
one job per unit that runs a check
(`nebensound / Check repository`, `nebensound / Check component <name>`),
`nebensound / Check`, which sums the units up, one job per unit that builds
and has a version of the run, that is, one the commit touches
(`nebensound / Build repository`, `nebensound / Build component <name>`), for a
build with targets one per target with the target behind the name, and
`nebensound / Build`, which sums the builds up. After a push to `main`,
`nebensound / Publish` tags and releases every unit with a version of the run,
and attaches the archives of its build to the release of a unit that publishes
to `release-assets` before that release is published (see
[the phases](docs/phases.md#publish)). A ruleset requires
`nebensound / Check` and `nebensound / Build`. A check
or a build the register knows but that has no building block yet fails by name
in the step `Not written yet` of its unit's job, and a version writer or a
publish target without one in the step `Version writers not written yet` or
`Publish targets not written yet` of `nebensound / Check`.

This repository calls the workflow the same way, with one difference: its own
[`gate.yml`](.github/workflows/gate.yml) calls the workflow at the commit that
is running, so a change to the pipeline is checked by the pipeline it changes.
The tool `nebensound-repo` is the package at the root of this repository, and
the xtask the other member of its one Cargo workspace, so the repository unit is
the tool and its version the tool's: its declaration selects the version writer
`cargo`, the Rust check over the workspace, the build `cargo-release` and
Dependabot for Cargo, the parts of the preset `rust-binary` that have a
building block. Its tags `vX.Y.Z` are the only ones. Every pull request and
every push to `main` builds the tool for each target of `cargo-release`, and
each binary has to print the version of the run. Every merge to `main` touches
the repository and is released in full, as `vX.Y.Z`, the latest release, with
nothing attached yet. The build `cargo-package` is held to its test cases
instead.

## Building blocks

Every check block checks one unit, and exactly the files `nebensound-repo files
<check> <unit path>` names: the files Git tracks below the unit's path whose
extension the register gives that check, less what an `[[exclude]]` entry of
the declaration takes out. A file nobody committed is not judged, and a
committed one is not missed. Each finding is an annotation on its file. A
version writer writes the versions of the plan, and a build block builds one
unit (see [`build-cargo-package`](#build-cargo-package),
[`build-cargo-release`](#build-cargo-release) and
[`build-docker`](#build-docker)).

The unit job gets the tool the same way `Declare` does
([`.github/actions/tool`](.github/actions/tool)): the verified binary of the
release on the pinned commit, or, where there is none, a build from source that
comes from the cache phase 0 fills and is built only when the cache misses. So
do the build jobs and the closing jobs `Check` and `Build`, whose steps are
calls of the tool as well. The list does not travel through a job output, which
is limited in size.

The checks for CSS, Dockerfiles, HTML, JSON, Markdown, TOML and YAML run from
the tool. Their blocks do two things:
`nebensound-repo setup --pinned --with-runtimes <check>` puts the pinned
version of every program the check calls into the tool's folder in the cache
directory, with a runtime one of them needs where the runner has none that is
new enough, and `nebensound-repo run <check> <unit path>` runs the
check over exactly the files `files` names. `run` starts the programs the
register pins with the flags described below, each part even when one before it
failed, and reports every finding with its file, and its line and column where
the program gives them: as an annotation in a workflow run, and as a line that
starts with `file:line:col:` elsewhere. A finding that fails nothing, such as a
Hadolint finding below the failure threshold, is a warning. What the programs
print stands in the output as well.

The same call runs a check on a developer's computer, on Linux and macOS alike,
once `nebensound-repo setup` has put the programs in place. `run` installs
nothing: it takes the pinned copy of a program first and the computer's
otherwise, and fails with a pointer to `setup` where there is neither, while
the parts of the check whose programs are there still run.
html-validate, markdownlint and Stylelint are run through their internals, so
that the repository's rules are laid over the pipeline's default, and of those
three `run` takes only the pinned copy; markdownlint is a library with no
command. Whether a JSON or a YAML file parses, and whether the Dependabot
configuration matches its schema, the tool judges itself and starts no program
for it. What `run` hands the programs, the schemas for Taplo and for the
Dependabot configuration, the pipeline's
linter defaults and the page rules of the HTML check, is compiled into the
tool.

### `check-css`

[`.github/actions/check-css`](.github/actions/check-css) checks every `.css`
and `.scss` file of the unit through `nebensound-repo run css`:

- Formatted the way Prettier writes it, run as in `check-json`.
- No Stylelint finding, with the repository's configuration laid over the
  pipeline's default, `stylelint-config-standard`, and for `.scss`
  `stylelint-config-standard-scss` read with `postcss-scss` (see
  [Linter rules](#linter-rules)). Its `ignoreFiles` and `.stylelintignore`
  take nothing out. Every finding is an annotation at its line and column: an
  error fails the check, a warning fails nothing.

Stylelint and the packages the default extends are pinned in the register,
Stylelint's companions beside it, and `setup` installs nothing else: a
configuration that extends or loads another package fails, naming it. `run`
takes only the pinned Stylelint, since it calls Stylelint's internals, and
starts it on its runtime, Node.js. Both parts run even when the other failed,
so one run shows every finding.

### `check-dockerfile`

[`.github/actions/check-dockerfile`](.github/actions/check-dockerfile) checks
every `Dockerfile`, `Containerfile` and `.dockerfile` file of the unit with
dockerfmt 1.0.0 and Hadolint 2.15.1, the release binaries the register pins by
SHA-256, through `nebensound-repo run dockerfile`. Building the image, scanning
it and running it belong to the build.

- Formatted the way dockerfmt writes it (`dockerfmt --check`). dockerfmt has no
  configuration file, and `run` hands it no style flag: the repository's
  `.editorconfig` decides the indentation, tabs, the final newline and
  dockerfmt's own `space_redirects`. An instruction dockerfmt leaves as written
  because it cannot format it safely, which it only warns about, fails the
  check on its line: it was not checked. `# dockerfmt-ignore` on the line
  before an instruction leaves it as written on purpose. A setting of
  `.editorconfig` dockerfmt cannot use, for which it takes its default, is a
  warning.
- No Hadolint finding at or above the failure threshold, by default `info`. A
  finding below it, such as a style finding by default, is a warning and fails
  nothing. The pipeline has no Hadolint default (see
  [Linter rules](#linter-rules)). Hadolint's JSON report is read by the tool,
  and every finding becomes an annotation at its line and column.

Both parts run even when the other failed, so one run shows every finding.

### `check-html`

[`.github/actions/check-html`](.github/actions/check-html) checks every `.html`
and `.htm` file of the unit through `nebensound-repo run html`. HTML that a
build generates is checked like written HTML once it is committed; the build
output of a run is not.

- Formatted the way Prettier writes it, run as in `check-json`.
- No html-validate finding, with the repository's configuration laid over the
  pipeline's default, the presets `recommended`, `a11y` and `prettier` (see
  [Linter rules](#linter-rules)). Its `.htmlvalidateignore` files take nothing
  out. A flat configuration (`html-validate.config.*`), which html-validate
  would read in place of every `.htmlvalidate.*`, is not read and fails the
  check.
- Every page carries a title (`page/title`), a description
  (`<meta name="description">`, `page/description`), exactly one `h1`
  (`page/one-h1`), and alternative text on every image (`page/image-alt`; an
  empty `alt=""` counts, for an image that only decorates). A page is a file
  whose document begins with `<!doctype html>` or has an `<html>` root; any
  other HTML file, such as a fragment or header a generator includes, gets the
  checks above without these. The page rules are html-validate rules compiled
  into the tool
  ([`src/run/html.rs`](src/run/html.rs)), switched on
  after the repository's configuration, so neither that configuration nor an
  inline `html-validate-disable` comment switches them off: only an
  `[[exclude]]` entry takes a page out. Each finding names the page, the line
  and the field. Every html-validate finding is an annotation at its line and
  column: an error fails the check, a warning fails nothing.
- Every link into the repository leads to a file and an `id` that exist,
  lychee run as in `check-markdown`, with the same `lychee.toml` handling: a
  stylesheet or an image counts as much as an anchor. A link from the root of a
  site, such as `/style.css` or `/login`, is not checked, since where the site
  is served is not in the declaration; links to the outside are not checked in
  a pull request. lychee's report stands in the output, and every broken link
  in it is a finding on the page that holds it, at the line and column lychee
  names.

html-validate is pinned in the register, and `setup` installs nothing beside
it: a configuration that names another package, such as a plugin, fails,
naming it. `run` takes only the pinned html-validate, since it calls
html-validate's internals, and starts it on its runtime, Node.js. A template
that does not parse as HTML needs an `[[exclude]]` entry with its reason. Every
part runs even when one before it failed, so one run shows every finding.

### `check-json`

[`.github/actions/check-json`](.github/actions/check-json) checks every
`.json`, `.jsonc` and `.geojson` file of the unit, and every `composer.lock`,
`flake.lock` and `Pipfile.lock`:

- `.json` and `.geojson` are strict JSON, exactly one value per file. The tool
  judges it itself, with the JSON parser it links (`serde_json`), and names
  the line and column where a file stops being JSON. A byte order mark at the
  start is passed over.
- `.jsonc` is strict JSONC as [jsonc.org](https://jsonc.org) defines it: JSON
  plus comments, trailing commas allowed, exactly one value per file. The tool
  judges it with the JSONC parser it links (`jsonc-parser`), which refuses what
  JSON5 would add: unquoted keys, single quotes, missing commas, hexadecimal
  numbers, `NaN`, `Infinity`. It reports the first place a file breaks.
- `composer.lock`, `flake.lock` and `Pipfile.lock` are strict JSON like
  `.json`, but not format-checked: the tool that writes them
  decides their layout (Composer and pipenv indent with four spaces). Every file
  of the check whose name ends in `.lock` is such a lockfile, as the register
  gives `json` no other.
- The three extensions are formatted the way Prettier writes them, with the
  repository's Prettier configuration, without its ignore files, and with
  `requirePragma`, `checkIgnorePragma`, `rangeStart` and `rangeEnd` set aside,
  which would pass a whole file unchecked.

Every part runs even when one before it failed, so one run shows every finding.

### `check-markdown`

[`.github/actions/check-markdown`](.github/actions/check-markdown) checks every `.md` and `.markdown` file of the unit:

- Formatted the way Prettier writes it, run as in `check-json`.
- No Markdownlint finding, with the repository's rules laid over the
  pipeline's default (see [Linter rules](#linter-rules)). markdownlint's library
  gets the files and the configuration the tool reads from one file at the
  root, and reads nothing else. Its `ignores`, `globs` and `gitignore` take
  nothing out.
- Every link into the repository leads to a file and a heading that exist
  (lychee, `--offline`). Links to the outside are not checked in a pull
  request: it should not turn red because someone else's server does not
  answer. Excluded addresses, accepted status codes and timeouts stay the
  repository's configuration, but lychee never reads it itself: the tool reads
  `lychee.toml` at the root key by key and writes the configuration lychee
  gets, carrying over the keys it knows as plain settings. A key that would
  make lychee start a command (`preprocess`), end or pass without checking
  (`dump`, `dump_inputs`, `generate`, `skip_missing`), send a link to an
  address the offline check does not check (`base_url`, `remap`), read or
  write another file (`cache`, `cookie_jar`, `exclude_file`, `files_from`,
  `glob_ignore_case`, `output`, `root_dir`) or ask a web archive (`suggest`)
  fails the check by name, and so does a key the tool does not know; the
  other parts of the check still run. The keys that choose which addresses
  are checked (`exclude`, `include`, `scheme`) are carried over as the
  repository's stated choice of what it leaves out, so they can still leave a
  link into the repository unchecked, as an `[[exclude]]` entry leaves a file
  out. The tool reads `.lycheeignore` at the root as well and adds its
  patterns to `exclude`, and starts lychee in a folder of its own, where it
  finds no `.lycheeignore` to read. It hands
  lychee every file as an absolute path and takes out itself what
  `exclude_path` matches, against the path from the root, as phase 0 matches
  it and holds it against the declaration, like every setting that takes
  files out (see [Exclusions](#exclusions)). A lychee table in `Cargo.toml`,
  `pyproject.toml` or `package.json` is not read. lychee's report stands in
  the output, and every broken link in it is a finding on the file that holds
  it, at the line and column lychee names.

### `check-python`

[`.github/actions/check-python`](.github/actions/check-python) checks one uv
project, the unit's path: its `pyproject.toml` lies there, and its `uv.lock`
there or at the root of the uv workspace above it. The block sets up the
programs the register pins for the check and calls `nebensound-repo run
python`, which runs the steps below in this order, each in a group of the log
(see [the tool](docs/tool.md#the-python-check)). The
interpreter is the one `.python-version` names, at the unit or at that root,
installed by uv from its own downloads and never taken from the machine; like
the toolchain of `check-rust`, it is no input. uv, Ruff, Pyright and
coverage.py are the programs [the register](docs/register.md#programs) pins,
each by its version and digests, and nowhere else; coverage.py is laid over the
project's environment as the wheel the register pins, which the tool fetches
and holds to its digest. pytest and its plugins are the project's own, from its
lockfile, since they import its code. coverage.py runs on Python 3.11 or later,
so the project's interpreter has to be one.

- The project installs as its `uv.lock` pins it (`uv sync --locked`), with its
  default dependency groups. A missing `pyproject.toml`, `uv.lock` or
  `.python-version` fails by name.
- Formatted the way Ruff writes it (`ruff format --check`).
- No Ruff finding, with the repository's configuration laid over the
  pipeline's default (see [Linter rules](#linter-rules)).
- No Pyright error in strict mode, with the repository's configuration laid
  over it (see [Linter rules](#linter-rules)). Pyright is handed every file of
  the list by name, and leaves out one in a folder whose name starts with a dot
  all the same, without a word: the check compares how many files Pyright
  analysed with how many it handed over, and names each one Pyright left out.
- The tests pass: the project's pytest, run by coverage.py. Each failed test is
  an annotation on its file and line, and a project without pytest fails by
  name.
- Every line and every branch is covered, of every Python file of the list,
  tests and scripts included and stubs (`.pyi`) not. A file coverage.py never
  saw, which no test imports, counts as not covered at all; one without a
  statement has nothing to cover. The repository's `# pragma: no cover` and
  `exclude_also` stay its choice, like `#[coverage(off)]`; its `fail_under`
  decides nothing.
- The wheel builds (`uv build --wheel`) and installs into a clean environment of
  the same interpreter, with the project's locked runtime dependencies
  (`uv export --frozen --no-default-groups`, no dependency group); there every
  command of `[project.scripts]` answers `--help` within 60 seconds, and every
  package and module at the top of the wheel imports, each with nothing on its
  standard input. A project without a `[build-system]`, or with
  `tool.uv.package = false`, fails: every Python unit is an installable
  package. No version is stored in the tree, so the wheel gets the placeholder
  version `0.0.0` for this step alone, and `pyproject.toml` is put back as
  committed afterwards. The wheel is never handed on. The build backend
  `[build-system]` names is installed from the package index, as its
  `requires` asks for it, without a lock: `uv.lock` pins no build dependency.

A file an `[[exclude]]` entry takes out is handed to no tool: Ruff and Pyright
get the list, pytest does not collect such a file, and coverage does not judge
it. An entry that takes out the unit's whole folder leaves nothing to check.
Ruff runs with `--no-force-exclude`, so its `exclude` takes no listed file out;
Pyright's `ignore` and `exclude` are set aside; coverage.py's `omit` counts for
nothing, since a file it leaves out counts as not covered.

Every step runs even when one before it failed, so one run shows every finding;
a step that needs the installed project is left out when installing it failed.
uv's cache, the interpreter and the project's environment lie in the tool's
folder in the temporary directory, which it removes again, and Ruff, pytest and
Python write no cache and no bytecode into the checkout.

### `check-rust`

[`.github/actions/check-rust`](.github/actions/check-rust) checks one Cargo
workspace, the unit's path. The block sets up cargo-llvm-cov as the register
pins it and calls `nebensound-repo run rust`, which runs the steps below in this
order, each in a group of the log, up to the first that fails (see
[the tool](docs/tool.md#running-a-check)). The toolchain is
the one the workspace pins in its `rust-toolchain.toml`; the oldest supported
version is the `rust-version` its packages claim; coverage is measured on the
nightly toolchain [the register](docs/register.md#checks) pins for the check.
The tool installs all three through rustup. None is an input, so no repository
can tell the check to look away:

- Formatted the way `cargo fmt` writes it: every member, and every path
  dependency that is no member, as `cargo fmt --all` reaches it. Such a
  dependency is formatted only; the steps below never reached it.
- Clippy without a warning, with all features and with no default features.
- Tests pass with all features and with no default features.
- Documentation builds without a warning.
- A release build succeeds.
- The workspace builds with the oldest `rust-version` any package claims.
- Regions, lines and functions are 100% covered (`cargo-llvm-cov`, on the
  pinned nightly toolchain, where `#[coverage(off)]` takes effect).
- Every published package builds and passes its tests from its own packaged
  contents, outside the source tree. The versions are written first, as
  [`version-cargo`](#version-cargo) writes them, from the plan the block hands
  the tool, and every manifest and lockfile is put back as it is committed when
  the step ends: a package without a version is not published, so only a unit with a
  version is verified here, on a pull request with the version its merge would
  get. The writing starts from the units of the workspace alone, as Cargo
  finds it, and follows their path dependencies wherever they lie, so what
  another workspace of the repository needs cannot fail this one, also a
  workspace of its own in a folder below it. The packages are packaged in one
  call, with each of them in place of crates.io from its folder, and each is
  tested with the packages of its siblings in place of crates.io: a version
  just written is on no registry yet, and a crate of the same name on
  crates.io must not stand in for it. A local package they depend on that is
  not packaged with them, a path dependency that is no member or a member an
  `[[exclude]]` entry takes out, is put in place of crates.io from its folder.

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
file and every `Pipfile` of the unit, and `Cargo.lock`, `uv.lock`,
`poetry.lock` and `pdm.lock`, with Taplo 0.10.0, the release binary the register
pins by SHA-256:

- Taplo reads each file from standard input. Given a path, it silently skips
  every `.lock` file and every `Pipfile` (all of them are fed through standard
  input all the same) and every file the `include` or `exclude` of the
  repository's `taplo.toml` leaves out, and reports success.
- `.toml` files are formatted the way Taplo writes them, with the repository's
  `taplo.toml`, and so is `Pipfile`, TOML without an extension that the register
  gives `toml` by name. Since `taplo format Pipfile` skips the file, a
  `Pipfile` is formatted by hand through standard input,
  `taplo format --stdin-filepath Pipfile - < Pipfile > Pipfile.new`, then moved
  over the original; or the `include` of `taplo.toml` names `**/Pipfile` beside
  `**/*.toml`. Lockfiles are not format-checked: the tool that writes them
  (Cargo, uv, Poetry, PDM) decides their layout. Every file of the check whose
  name ends in `.lock` is such a lockfile, as the register gives `toml` no
  other.
- Every file passes `taplo lint`, run without the repository's configuration so
  that it cannot switch the schema validation off.
- Schemas are assigned by the tool, so far to `Cargo.toml` by file name and
  to `.nebensound/repo.toml` by path (below). The schemas for `Cargo.toml` are
  copies of SchemaStore's schemas that the tool compiles in from
  `schemas/`, where they lie with their licence notice
  (see [their README](schemas/README.md)), so the check
  makes no request to SchemaStore. Apart from the declaration, a
  file whose leading comments hold its own `#:schema` directive with an
  absolute URL keeps that schema. A schema that cannot be read fails the check
  instead of being skipped.
- `.nebensound/repo.toml` is always checked against the schema of the
  declaration the tool compiles in, the one of the same pipeline version as
  phase 0, directive or not. `nebensound-repo schema` derives it from
  the types phase 0 reads the file with, so an unknown key fails with its line
  and column. The schema is looser than phase 0, never stricter.

The schema of the declaration on `main` has a public address, for editors:

```text
https://raw.githubusercontent.com/Nebensound/pipeline/main/schemas/nebensound-declaration.json
```

An editor that reads Taplo's directive finds it through this line before the
first key or table of `.nebensound/repo.toml`:

```toml
#:schema https://raw.githubusercontent.com/Nebensound/pipeline/main/schemas/nebensound-declaration.json
```

### `check-yaml`

[`.github/actions/check-yaml`](.github/actions/check-yaml) checks every `.yml` and `.yaml` file of the unit:

- Formatted the way Prettier writes it, run as in `check-json`.
- Valid YAML, judged by the tool with the YAML parser it links (`yaml-rust2`):
  the first error that stops the parser, such as a syntax error or an alias
  that names no anchor declared before it in its document, and every key that
  a mapping holds twice, each at its line and column. Keys are compared by
  their text, so `1` and `"1"` are the same key; the merge key `<<` may stand
  more than once. The pipeline reads no configuration of yamllint any more:
  phase 0 names a `.yamllint`, `.yamllint.yaml` or `.yamllint.yml` at the root
  as unread (see
  [`unread-configuration`](docs/declaration.md#unread-configuration)).
- Every workflow directly under `.github/workflows` passes actionlint, with
  ShellCheck reading every `run:` script and pyflakes every Python one where
  they are on the search path, as ShellCheck is on GitHub's Linux runners; the
  output says whether each was found. actionlint 1.7.12 does not know `$/`
  yet and reports every such call as invalid; `run` drops exactly these two
  messages, and only for a `uses:` that begins with `$/`. Nor does it know
  the key `queue` of a `concurrency` section, which GitHub documents; `run`
  drops the message about that one key, and every other unknown key of the
  section is still a finding. Nor does it know the properties
  `job.workflow_sha` and `job.workflow_repository`, which GitHub documents;
  `run` drops the message about those two, and every other unknown property
  of `job` is still a finding.
- `.github/dependabot.yml` matches the Dependabot schema, which the tool
  compiles in ([`schemas/dependabot-2.0.json`](schemas/dependabot-2.0.json),
  from SchemaStore) and validates against itself, without a request. Merge keys
  `<<` are applied first, as YAML 1.1 defines them, so a mapping brought in
  with `<<` counts as part of the mapping that merges it. An unknown
  key fails the check with the file and the path of the key named, instead of
  surfacing only after the merge.

### `version-cargo`

[`.github/actions/version-cargo`](.github/actions/version-cargo) is the version
writer `cargo`. No manifest in the tree carries a version: the history is the
truth, and Cargo cannot take a version from outside. So the version is written
into the checkout of the run, immediately before something builds with it, and
never committed. The block is one call: it hands the plan Declare printed to
`nebensound-repo write-versions --plan` on its standard input, since its job
checks out no history to resolve a version from, and an empty plan is refused
by name. Every unit of the plan with the writer `cargo` that
the commit at hand touches gets the version of the commit, on a pull request
the one its merge would get with `-pr.<number>` behind it (such as
`0.4.2-pr.17`). A unit the commit does not touch gets a version only where a
package that gets one depends on it by path, followed through further path
dependencies: its current version, the highest one the history gives a commit
up to the commit at hand, a pre-release included, and on a pull request without
`-pr.<number>`, since the pull request does not touch it. No tag is read for
it, so a repository without any tag passes from its first commit on. With that
version:

- `package.version` in the unit's `Cargo.toml` becomes that version.
- Every path dependency on the package, wherever in the repository it is
  written, gets that version as its requirement, in `[dependencies]`,
  `[dev-dependencies]`, `[build-dependencies]`, their `[target.*]` forms and
  `[workspace.dependencies]`: a package whose dependency names no version
  cannot be published. In the manifest of a unit that gets nothing, only the
  entries of its `[workspace.dependencies]` that a member that gets a version
  inherits.
- The workspace's `Cargo.lock` is brought in line: the package's own entry, and
  every entry that names it with a version. No other entry changes, so
  `cargo build --locked` passes. The same goes for the `Cargo.lock` of every
  other workspace that reaches the package through path dependencies, directly
  or through another package: one that excludes the package, one beside it, or
  the `fuzz/` folder `cargo fuzz init` creates.

Every change keeps the rest of the file as it is written, and a second call
changes nothing. Any other unit with the writer gets nothing, and neither does
one that no commit has touched yet, which has no current version: its manifest
stays as it is, and the package test of `check-rust` does not package it.

The path dependencies followed are those in `[dependencies]`,
`[dev-dependencies]` and `[build-dependencies]`. A dependency of the tests
counts too: Cargo drops one without a version from the package, and the
packaged tests that use it would not compile. A path dependency on a package
that carries no version at all (a unit with `versioning = "none"`, or one that
does not select the writer `cargo`) is left to Cargo: the package test fails
with Cargo's message. The release stage tags and releases only the units the
commit touches, and what a release needs has a release already, or the pull
request fails: a package the pull request would publish to a registry (a
target with `dependencies-by-version` in the register) may require by path,
followed through, only units it touches or whose current version has a tag.
Otherwise the preparing job `nebensound / Declare` fails with
[`unreleased-requirement`](docs/declaration.md#unreleased-requirement), which
names the way out: touch the required unit in the same pull request; no check
or build starts, and `nebensound / Check` and `nebensound / Build` fail. Whether
the registry itself holds the version is for the publish block of that
registry to check before it publishes.
Only `Cargo.toml` and `Cargo.lock` files change. `check-rust` writes the
versions the same way for its package test, starting from the units of its
workspace (`--workspace`). The Build phase calls the block in the job of every
unit that builds in the run and selects the writer, before it builds. A version
writer without a building block (today `asciidoc`) fails by name in
`nebensound / Check`.

### `build-cargo-package`

[`.github/actions/build-cargo-package`](.github/actions/build-cargo-package) is
the build `cargo-package`. It builds one unit, the one Cargo package whose
`Cargo.toml` stands at the unit's path, a member of a workspace or a workspace
of its own, and runs in the unit's job of the Build phase after `version-cargo`
has written the version of the run into the manifest: on a pull request the
one its merge would get, with `-pr.<number>` behind it. The build is one call
of the tool, `nebensound-repo build cargo-package <unit>`, which runs each of
the steps below in a group of the log, up to the first that fails, and names
the artifact and the file as outputs of its step; the block's second step hands
the file on with `actions/upload-artifact`. The tool refuses a unit the plan
starts no `cargo-package` build for.

- The toolchain is the one the workspace pins in its `rust-toolchain.toml`.
- A unit path without a `Cargo.toml`, or one whose `Cargo.toml` is a workspace
  without a package of its own, fails: there is no one crate to build.
- `cargo package --locked` packages the crate once, with `--allow-dirty` for
  the version Git does not have, and builds what it packed, so a file the
  package leaves out that the build needs fails here.
- A local package it depends on by path, directly or through another one, is
  patched in from its folder for crates.io: its dependency names the version
  the writer gave it, which is on no registry yet.
- The `.crate` is unpacked outside the source tree, where neither the workspace
  nor its `rust-toolchain.toml` reaches, and its tests run there with all
  features, with the same toolchain and the same patches. A file the package
  leaves out that a test needs fails the build, and so does a dev-dependency
  with a `path` and no `version`, such as a workspace-only test helper:
  `cargo package` drops it from the manifest. The error names both causes.
- The `.crate` is uploaded as an artifact of the run named
  `crate-<path>@<version>@<sha>`: the unit's path percent-encoded (`.` for the
  repository itself, `tools%2Ffoo` for `tools/foo`), the version the package
  carries and the commit the checkout holds. The artifact holds exactly the
  file `<package>-<version>.crate`. A second attempt of the same job replaces
  it.

Nothing is cached: the artifact is built from nothing in every run, so that
nothing a pull request left in a cache reaches it.

### `build-cargo-release`

[`.github/actions/build-cargo-release`](.github/actions/build-cargo-release) is
the build `cargo-release`. It builds the one binary of one unit for one target
and runs in the job of that target, after `version-cargo` has written the
version of the run into the manifest; it gets the plan to read the unit's
version of the run from it. [The register](docs/register.md#builds) lists the
targets, each with the runner it is built on: `x86_64-unknown-linux-musl` on
`ubuntu-latest`, `aarch64-apple-darwin` on `macos-latest` and
`x86_64-apple-darwin` on `macos-26-intel`, GitHub's Intel image of macOS, which
GitHub ends in 2027; what happens to that target then is ruled when it comes.
Declare gives each target a job of its own on its runner, named after the unit
with the target behind it, such as `nebensound / Build repository
(x86_64-unknown-linux-musl)`, and `nebensound / Build` gathers them. The build
is one call of the tool, `nebensound-repo build --plan <file> cargo-release
<unit>`, which runs each of the steps below in a group of the log, up to the
first that fails, and names the artifact and the file as outputs of its step;
the block's second step hands the file on with `actions/upload-artifact` (see
[the tool](docs/tool.md#running-a-build)). The tool refuses a unit the plan
starts no `cargo-release` build for.

- The target is the one of the register whose platform is the machine's: the
  block takes no target, and a machine the register lists none for is refused
  by name. Linux is linked statically against musl, so that the binary runs on
  every runner image whatever C library it carries.
- The toolchain is the one the workspace pins in its `rust-toolchain.toml`,
  with the target added to it.
- The package is the one whose `Cargo.toml` stands at the unit's path, and it
  has exactly one binary. A workspace without a package of its own there, or a
  package with no binary or with several, fails and names them.
- `cargo build --release --locked` builds that binary for the target, with the
  package's default features.
- The binary is started with `--version` and has to end within 30 seconds,
  with success, and print the version of the run, on a pull request with
  `-pr.<number>` behind it.
- The archive `<binary>-<target>.tar.gz` holds the binary and the licence of
  the unit: the files whose names start with `LICENSE`, `LICENCE` or `COPYING`
  in the first folder from the unit's own up to the repository root that holds
  one. Without a licence the build fails. The same files give the same bytes.
- The archive is uploaded as an artifact of the run named
  `executable-<path>@<version>@<sha>@<target>`, the path percent-encoded as for
  a crate. It holds exactly that file. A second attempt of the same job
  replaces it.

Nothing is cached, for the same reason as for a crate. Attaching the archives
to a release is not built yet.

### `build-docker`

[`.github/actions/build-docker`](.github/actions/build-docker) is the build
`docker`. It builds the image of one unit and runs in the unit's job of the
Build phase, after the version writers the unit selects; it gets the plan to
read the unit's version of the run from it. The block sets up the programs the
register pins for the build (`nebensound-repo setup --pinned docker`), and the
build is one call of the tool, `nebensound-repo build --plan <file> docker
<unit>`, which runs each of the steps below in a group of the log, up to the
first that fails, names the artifact and the file as outputs of its step and
removes what it started; the block's last step hands the file on with
`actions/upload-artifact` (see
[the tool](docs/tool.md#running-a-build)). The tool refuses
a unit the plan starts no `docker` build for.

- The Dockerfile is `deploy/Dockerfile` below the unit's path, and the build
  context is the repository root. Each unit has its own, so several services of
  one workspace each build their own image. Phase 0 refuses a unit without
  `deploy/Dockerfile`
  ([`build-without-file`](docs/declaration.md#build-without-file)). The
  block's first step keeps a check of its own only as a safeguard: it fails by
  name where the checkout cannot read a file phase 0 saw, such as a tracked
  symlink whose target is missing.
- The image is built once, for `linux/amd64` only, on an `x86_64` Linux machine,
  by a BuildKit of its own, the image [the register](docs/register.md#images)
  pins by digest, without a cache, without provenance and without an SBOM, into
  an OCI layout. It carries the labels `org.opencontainers.image.version` (the
  unit's version of the run, on a pull request with `-pr.<number>` behind it),
  `.revision` (the commit), `.source` (the repository's address, in a workflow
  run) and `.title` (the unit's name, the repository's for the repository
  itself).
- Its identity is the digest of its manifest, printed in the log and in the
  summary of the run.
- An image without `HEALTHCHECK` fails before it is started.
- Exactly that layout is loaded and started, as the image configures it: no
  `--user`, no environment, no secret, no published port, the default network,
  and a fresh named volume at every path the image declares with `VOLUME`. The
  block waits until Docker reports it healthy, for at most five minutes; an
  image that becomes unhealthy, stops or is not healthy by then fails with the
  container's log and the output of its last health check.
- Once it is healthy, the user ID of its first process is read from the host;
  an image that runs as root fails, whatever name its user has.
- Trivy, pinned in [the register](docs/register.md#programs), scans the layout
  and fails the build on every vulnerability of severity HIGH or CRITICAL that
  has a fixed version, each named on the Dockerfile. No ignore file and no
  configuration file of Trivy is read, and there is no exception. Its database
  of vulnerabilities is fetched in every run, into a folder of the build, from
  where Trivy fetches it by default; it is pinned nowhere.
- The layout is uploaded as an artifact of the run named
  `container-<path>@<version>@<sha>`, the path percent-encoded as for a crate.
  It holds exactly the file `image.tar`. Nothing is pushed. A second attempt of
  the same job replaces it.

Nothing is cached, for the same reason as for a crate. What the build started
(the container, its volumes, the loaded image and the builder) is removed at
the end, whatever happened, and so is everything it made but the layout it
hands on. Docker, its Buildx and Trivy start without the variables that steer
them (see
[the tool](docs/tool.md#starting-docker-and-trivy)).

### Linter rules

Markdownlint, Stylelint and html-validate run with a default of the
pipeline:
[`rules/markdownlint.jsonc`](rules/markdownlint.jsonc)
(`MD013` off, since Prettier does not wrap prose),
[`rules/stylelint.yaml`](rules/stylelint.yaml)
(`stylelint-config-standard`, for `.scss` `stylelint-config-standard-scss`) and
[`rules/html-validate.json`](rules/html-validate.json)
(the presets `recommended` and `a11y`, less what contradicts Prettier through
html-validate's own preset `prettier`). `run` hands each linter the default
compiled into the tool of the pipeline version that runs. A repository's own
configuration is laid over that default rule by rule: its file names only the
rules it wants different, every rule it does not name keeps the default's value,
and a later change of the default reaches it without a change of its own.
`nebensound-repo check` shows, per check, every rule the repository sets and how
it stands to the default (see
[the tool](docs/tool.md#the-rules)).

A repository whose pull request template begins on a second-level heading
needs one Markdownlint rule changed, for that folder only. Instead of an inline
`<!-- markdownlint-disable-file MD041 -->` in the template, the one file of its
Markdownlint configuration at the root, `.markdownlint-cli2.jsonc`, holds the one
rule for the folder, as a pattern:

```jsonc
{
  "overrides": [
    {
      "filter": [".github/**"],
      "combine": "merge",
      "config": {
        "MD041": false,
      },
    },
  ],
}
```

`MD013` stays off there and everywhere else, as the default has it. Where the
rule is to hold in the whole repository, it stands under `config` in the same
file. markdownlint never looks for its configuration: the tool reads the first
of `.markdownlint-cli2.jsonc`, `.markdownlint-cli2.yaml`, `.markdownlint.jsonc`,
`.markdownlint.json`, `.markdownlint.yaml` and `.markdownlint.yml` at the root,
as data, and hands markdownlint the rules of each file. Another of them at the
root, one in a folder below it and one written in JavaScript are not read, and
fail phase 0 (`unread-configuration`) and the check, unless an `[[exclude]]`
entry takes them out. A key the tool does not know or one that names code or
another file, such as `customRules` or `extends`, fails the check (see
[the configuration of a linter](docs/tool.md#the-configuration-of-a-linter)).
A file may also switch a rule of the default off or loosen it; the rules report
shows every difference. markdownlint is the first linter under this rule, beside
lychee, whose `lychee.toml` the tool reads key by key. The other linters still
find and read the repository's configuration themselves, or follow a file it
names: Prettier, Stylelint, html-validate, Hadolint, actionlint, the
Dockerfile formatter, Ruff, Taplo's formatter and Pyright (see
[what does not hold yet](docs/principles.md#what-does-not-hold-yet)).

Stylelint reads, for each stylesheet, the configuration nearest to it: the
first of the names it looks for (`package.json` with a `stylelint` key,
`.stylelintrc`, `.stylelintrc.json`, `.stylelintrc.yaml`,
`stylelint.config.mjs` and the others) in the file's folder or the nearest
folder above it. `run css` hands it that configuration as the last of what
its configuration extends, after the default, so Stylelint itself lays the one
over the other rule by rule, and what the repository's `extends` names comes
along over the default as well. An `overrides` entry matches its `files` from
the repository root. html-validate reads the `.htmlvalidate.json` (`.mjs`,
`.cjs`, `.js`) of a page's folder and of every folder above it, up to one that
says `root: true`; `run html` lays them over the default, the nearer over
the farther. The page rules lie over everything and cannot be switched off.
Both checks find only the pinned packages, so a configuration that names
another one, such as a plugin, fails with the package named.

Ruff runs with a default of the pipeline as well:
[`rules/ruff.toml`](rules/ruff.toml),
every rule (`ALL`) less the ones Ruff names as conflicting with its formatter,
one of each of two pairs of docstring rules that contradict each other, the
copyright notice in every file (`CPY001`), since the licence stands in the
`LICENSE` every repository holds, and `S101` in tests, since pytest asserts
with `assert`. The repository's own
configuration is the one Ruff finds for the unit: `.ruff.toml`, `ruff.toml` or
the `[tool.ruff]` table of a `pyproject.toml`, in the unit's folder or the
nearest one above it. `check-python` lays it over the default the way Ruff
lays a configuration over the one its `extend` names: its `ignore`,
`extend-select` and `extend-per-file-ignores` change single rules and keep the
rest of the default, its `select` and `per-file-ignores` start over, and every
other setting it names wins. What its own `extend` names comes along, over the
default as well. A rule switched off for one line (`# noqa: <code>`) or one
file (`# ruff: noqa: <code>`) stays the repository's choice.
`nebensound-repo check` shows every rule and prefix the repository's
configuration and the files its `extend` names switch on or off, as each file
writes it, against the same entry of the default, and for an
`extend-select` entry the entries of the default's `ignore` it switches back
on (see [the tool](docs/tool.md#the-rules)).

Pyright runs in strict mode, with the repository's configuration laid over it:
the one Pyright finds for the unit, `pyrightconfig.json` or the
`[tool.pyright]` table of a `pyproject.toml`. Its rule severities, Python
version and platform, execution environments and stub paths stay the
repository's choice, and so does `# pyright: reportX=false` in a file. A lower
mode is a finding, as `typeCheckingMode` in the configuration and as
`# pyright: basic` or `# pyright: standard` in a file; strict mode holds all
the same. A `# type: ignore` without rules before a file's first statement,
which takes the whole file out of Pyright, is a finding as well; on a line of
its own it stays the repository's choice.

Hadolint runs without a default of the pipeline: its own rules contradict no
other tool of the pipeline, so there is nothing to lay beneath them, and
`nebensound-repo check` reports nothing for it. `nebensound-repo run dockerfile`
hands it the repository's `.hadolint.yaml` or `.hadolint.yml` at the root, and
without one an empty configuration, so that no configuration in a user's home
folder counts.
`ignored`, `override`, the trusted registries, the label schema, a failure
threshold and the inline and global ignore pragmas stay the repository's rule
configuration. What would pass every finding is set aside: `no-fail`, a
`failure-threshold` of `none` or `ignore` (Hadolint's default, `info`, holds
then), and every `HADOLINT_*` variable. A configuration Hadolint cannot read
fails the check, where Hadolint itself would run without it.

Settings that take whole files out stay what they are: set aside by the block
and held against the declaration by phase 0 (see [Exclusions](#exclusions)).

### Exclusions

`.nebensound/repo.toml` is the only place where a repository takes a file out
of a check, with an `[[exclude]]` entry and its reason. `nebensound-repo tree`
shows such a file as excluded, with the reason. Rust code is excluded only as a
whole package (see [`check-rust`](#check-rust)).

No block lets a tool's own way of leaving files out take a file out: an ignore
file needs no reason and can turn a check green that checked nothing.
`.gitignore` takes nothing out either. The blocks read none of these settings,
except lychee's `exclude_path`, which `check-markdown` and `check-html` apply
from the `lychee.toml` they read, and rustfmt's `ignore`, which comes
along with the repository's rustfmt configuration and takes effect on a nightly
toolchain, and Ruff's `# ruff: noqa`, which a Python file holds itself.
Settings in a tool's configuration that pass a whole file unchecked are set
aside on the command line: Prettier's `requirePragma`, `checkIgnorePragma`,
`rangeStart` and `rangeEnd`, and rustfmt's `disable_all_formatting`;
`check-python` runs Pyright without its `ignore` and `exclude` and Ruff with
`--no-force-exclude`, and judges coverage itself, so that coverage.py's `omit`
takes nothing out. Run by
hand, the tools read all of them, so phase 0
holds them against the declaration and fails on every file one of them takes
out that no `[[exclude]]` entry takes out (`unexcused-ignore`). For lychee,
that also keeps `exclude_path` from taking a file out in the block: phase 0
matches it against the path from the root, such as `docs/a.md`, as the block
does. A setting that takes out less is fine. Phase 0 reads:

| Tool              | Setting                                                                                                  |
| ----------------- | -------------------------------------------------------------------------------------------------------- |
| Prettier          | `.prettierignore` at the root                                                                            |
| Taplo             | `include` and `exclude` in `.taplo.toml` or `taplo.toml` at the root                                     |
| markdownlint-cli2 | `ignores` in `.markdownlint-cli2.jsonc` or `.markdownlint-cli2.yaml`, in every folder                    |
| lychee            | `exclude_path` in `lychee.toml` at the root                                                              |
| Pyright           | `include`, `ignore`, `exclude` in `pyrightconfig.json` or `[tool.pyright]`, in every folder              |
| coverage.py       | `omit` in `.coveragerc`, `.coveragerc.toml`, `setup.cfg`, `tox.ini` or `pyproject.toml`, in every folder |
| Ruff              | `# ruff: noqa` or `# flake8: noqa` without rule codes, in the Python file itself                         |
| Ruff              | `exclude`, `extend-exclude` in `.ruff.toml`, `ruff.toml` or `[tool.ruff]`, in every folder               |
| Ruff              | `include`, `lint.exclude`, `format.exclude` in the configuration that governs a file                     |
| rustfmt           | `ignore` in `.rustfmt.toml` or `rustfmt.toml`, in every folder                                           |
| Stylelint         | `.stylelintignore` at the root; `ignoreFiles` in the configuration of every folder                       |
| html-validate     | `.htmlvalidateignore` in every folder                                                                    |

Only the files the tool reads in a block count: a `.prettierignore` that takes
out a Rust file takes out nothing Prettier would have looked at. Taplo's
`include` counts only against `.toml` files, the ones Taplo reads without it; a
`Pipfile` or a lockfile of the `toml` check (`Cargo.lock`, `uv.lock`,
`poetry.lock`, `pdm.lock`) is taken out of Taplo only by `exclude`. A setting
that takes out nothing but lockfiles the register names, such as a
`.prettierignore` naming `composer.lock` or an `exclude` naming `Cargo.lock`,
takes nothing out of a check: no block hands a lockfile to Prettier, and Taplo
lints without its configuration. One that takes out another file as well is
reported at that file only. rustfmt's `ignore` is read in
every folder, relative to that folder, since which configuration rustfmt applies
to a file depends on where it runs; a `.rs` file it takes out cannot be excused
by an `[[exclude]]` entry of its own, only by one that takes out its whole
package. Stylelint's `ignoreFiles` is read in the configuration Stylelint
finds in each folder, relative to that folder, in order with its negated
patterns, and counts for the files below it that no nearer configuration
governs; a configuration in JavaScript is not judged, the block sets its
`ignoreFiles` aside all the same, and a file that something it extends would
still take out fails in the block. html-validate's `.htmlvalidateignore` is
read in every folder, relative to that folder, as html-validate's command line
reads it. coverage.py takes its settings from the first of `.coveragerc` (or the
file `--rcfile` or `COVERAGE_RCFILE` names), `.coveragerc.toml`, `setup.cfg`,
`tox.ini` and `pyproject.toml` in the folder it runs in that exists, where
`setup.cfg`, `tox.ini` and `pyproject.toml` count only when they hold settings
of its own; since that depends on where it runs, phase 0 reads `omit` of the
run and report sections in every one of them in every folder, relative to that
folder. In `.coveragerc` the sections are `[coverage:run]` and
`[coverage:report]`, or `[run]` and `[report]` where the prefixed section is
missing; in `setup.cfg` and `tox.ini` `[coverage:run]` and
`[coverage:report]`; in `pyproject.toml` `[tool.coverage.run]` and
`[tool.coverage.report]`; and in `.coveragerc.toml` the latter, or `[run]` and
`[report]` where the prefixed table is missing, as coverage.py looks them up.
coverage.py picks the section before it reads `omit`, so a prefixed section
without `omit` hides the plain one. In the INI files, an `omit` under
`[DEFAULT]` counts as set in the section picked, since Python's configparser
hands it on to every section, and a section name keeps the spaces inside its
brackets, so `[ run ]` is not `[run]`.
Ruff's `exclude` and `extend-exclude` are read in the configuration
Ruff finds in each folder (the first of `.ruff.toml`, `ruff.toml` and a
`pyproject.toml` with a `[tool.ruff]` table), relative to that folder, `*`
crossing a `/` and a pattern without a `/` naming a file or folder of that name
anywhere below, as Ruff matches them; each counts for what lies below its
folder that no nearer configuration governs, and for the folder of a nearer one
itself, which Ruff checks with the configuration above it. Ruff's `include`,
`lint.exclude` and `format.exclude` are read in the configuration that governs
a file, the nearest one above it, and matched as Ruff matches them: against
the file's path and, for `lint.exclude` and `format.exclude`, its name, but not
against the folders above it, so a folder's name there takes nothing out.
`lint.exclude` takes a file out of `ruff check` alone and `format.exclude` out
of `ruff format` alone, and the finding names the step. Ruff's default
`include` takes every `.py` and `.pyi` file, all the check covers, so only an
`include` of the configuration's own can leave one out; `extend-include` only
adds to `include` and is never a finding. Where a configuration names another
in `extend`, the settings of that one count beneath its own, as Ruff lays
them, each pattern relative to the folder of the file that writes it; an
`extend` is followed inside the repository only, and one that comes back round
is a loop, which Ruff refuses (`unreadable-ignore-file`). Pyright's `exclude`
is read like its `ignore`; its `include` takes out every file below the folder
of its configuration that none of its entries names. Ruff's `exclude` takes the
place of its default list, Pyright's adds to its own; what those lists leave
out (Ruff's `.venv`, `node_modules`, `dist` and the like; Pyright's
`**/node_modules`, `**/__pycache__` and `**/.*`) is no finding, whether a
setting names it again or not. A setting phase 0 cannot read fails as well (`unreadable-ignore-file`). A JavaScript options file of markdownlint-cli2
(`.markdownlint-cli2.cjs` or `.mjs`) is not judged here, since phase 0 cannot read it
without running it; it fails as a file the tool does not read (`unread-configuration`). A setting
that switches a rule off for some files, such as markdownlint-cli2's
`overrides`, takes no file out of the tool and stays the
repository's rule configuration. lychee's `.lycheeignore` leaves out addresses,
not files, so phase 0 does not judge it. dockerfmt and Hadolint have no setting
that takes a file out: `# dockerfmt-ignore` leaves one instruction as written,
and Hadolint's `ignored` and ignore pragmas switch rules off.

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
  `cargo xtask setup` calls `nebensound-repo setup`, which finds or installs the
  programs the register's checks call
  ([the tool](docs/tool.md#setting-up-the-programs)),
  and then adds what only that repository needs, such as a program its own
  checks call.
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
line that is not empty and is no comment (it starts with `#`, but not with
`##[`) must match a whole line the block prints, with `*` standing for any
text, for example
`::error file=data.json::not formatted the way Prettier writes it`. That file is
not part of the case's repository.

A case of either kind may hold a file `forbidden-line`, which names commands
the block must not start, such as a workflow command that a name or a file of
the case's repository would start: every line that is not empty and is no
comment is a pattern, as in `expected-finding`, that no command the runner
would read in a line the block prints may match. Like `expected-finding`
it is not part of the case's repository.

The lines are read the way the runner of GitHub Actions reads them. The tool
prints everything that comes from a program or from the repository while the
runner's processing of workflow commands is stopped (see
[the principles](docs/principles.md#5-one-rule-per-kind-of-problem)), and a
line inside such a stretch starts nothing. So a pattern of `expected-finding`
that starts with `::` names an annotation, and only a line outside a stopped
stretch matches it; any other pattern matches a line wherever it stands. A
pattern of `forbidden-line` is matched against the command the runner would
read in a line outside a stopped stretch, from where it starts. The runner
first reads the newer form: past white space the line starts with `::`, and a
command it knows is named before the next `::`. Only when that names none does
it read the older form, from the first `##[` in the line. So `::warning::x*`
forbids the newer form and `##[warning]x*` the older one, and the tool's own
`::error file=a##[warning]x.dockerfile::…` is the command `error` and starts no
warning.
The failing cases `older-command-form-in-a-file-name` and
`colons-at-the-start-of-a-file-name` of check-dockerfile, and
`content-a-program-quotes` of check-json, hold such a file: a name that holds
`##[warning]x`, a name that starts with `::warning::x`, and a JSON file whose
broken line, which Prettier quotes, holds `##[warning]quoted`. A name with a
line break never reaches a check: phase 0 refuses it
([`control-character`](docs/declaration.md#control-character)), and a case has
to pass phase 0, so the tool's own tests show that refusal instead.

`cargo xtask check --all` first holds the blocks, the register and
`nebensound.yml` to each other:

- every directory under `.github/actions/` is a block that belongs to a register
  entry (`check-<name>`, `version-<writer>`, `build-<name>`,
  `publish-<target>`), or the workflow's own machinery (`phase0`, `tool`);
- every check block is called from a step of `nebensound.yml` that is named
  after its entry and switched on it with exactly the condition
  `${{ !cancelled() && contains(matrix.checks, '<name>') }}`, so that an
  inverted or widened switch is caught. The step stands in the check job, the
  one whose matrix is `${{ fromJSON(needs.declare.outputs.units) }}`, and has
  no other key than `name`,
  `if`, `uses` and `with`, and hands the block exactly
  `working-directory: ${{ matrix.path }}`, so that neither
  `continue-on-error` nor another path silences the check. Beside it the step
  hands `plan: ${{ needs.declare.outputs.plan }}`, the plan of phase 0, exactly
  when the block's `action.yml` declares the input `plan`, as `check-rust` does
  to write the versions before it tests a package, and never otherwise: the
  block's cases get the plan exactly then, so a step that dropped it would
  leave every caller's run without what the cases test;
- every version writer's block is called from a step named after its entry
  and switched on it with exactly `${{ contains(matrix.writers, '<writer>') }}`,
  and every build block from a step named after its entry and switched on it
  with exactly `${{ matrix.build == '<name>' }}`, both in the build job (the
  job whose matrix is `${{ fromJSON(needs.declare.outputs.builds) }}`), every
  version writer's step above every build step, so that the build packages the
  version of the run. Neither
  condition holds `!cancelled()`, so that nothing is built after a step that
  failed. The step has no other key than `name`, `if`, `uses` and `with`; it
  hands a version writer exactly `plan: ${{ needs.declare.outputs.plan }}` and
  a build exactly `working-directory: ${{ matrix.path }}`, beside which it
  hands the plan exactly when the build block's `action.yml` declares the
  input `plan`, as `build-docker` does to label what it builds with the unit's
  version;
- the step `Not written yet` of the check job counts exactly the checks that
  have a block as written (`WRITTEN_CHECKS`), the step `Not written yet` of
  the build job exactly the builds (`WRITTEN_BUILDS`), the step
  `Version writers not written yet` exactly the version writers, and the step
  `Publish targets not written yet` exactly the publish targets, each under
  the name of its block, the dots of a target turned into hyphens, and beside
  them `release-assets`, which the tool's release stage carries out itself in
  `Publish` and which has no block;
- every block has a passing and a failing case. A `.DS_Store` beside the cases
  is passed over: Git ignores it, so the gate never sees it;
- every step of a block carries no other key than `name`, `id`, `shell`,
  `env`, `run`, `uses` and `with`, what a thin block needs: the call of the
  tool with its inputs handed in through `env`, the actions the block needs,
  and the `id` by which a later step reads an output. The cases call the tool,
  not the block's steps, so they would not notice a `continue-on-error` that
  keeps every caller's job green whatever the tool finds, or an `if` or a
  `working-directory` that puts a decision or a place into the block.

For now the xtask knows only how a check, a version writer and a build are
called and tested, so a `publish-` block is reported until the xtask learns
that kind. The first one teaches it, together with its cases.

Then it runs every block against its cases, each copied into a fresh Git
repository. The work of every block runs from the tool (`check-css`,
`check-dockerfile`, `check-html`, `check-json`, `check-markdown`,
`check-python`, `check-rust`, `check-toml`, `check-yaml`, `version-cargo`,
`build-cargo-package`, `build-cargo-release`, `build-docker`), and its steps
hold nothing of it, so the cases run through the tool itself: the xtask builds
the tool of this commit
with Cargo, runs phase 0 on the case with `nebensound-repo check` and `plan`, as
`Declare` does, and calls the tool as the block's step does, in the environment of a
workflow run, so that the findings are annotations; its temporary folder
(`RUNNER_TEMP`) is the case's own, so that what a build leaves there goes with
the case. A check calls `nebensound-repo run` once for every unit the plan
starts the check for. The version writer calls `nebensound-repo write-versions`
once, provided a unit of the plan selects it. A build calls `nebensound-repo
build` once for every unit the plan starts the build for and gives a version of
the run, after the version writers the unit selects, each through the tool, and
only when they passed. A version writer gets the plan, and a check or a build
that takes it, as `check-rust` does, gets it as its step in `nebensound.yml`
hands it: the xtask hands it to the tool with `--plan`, in a file beside the
case. The cases of `check-rust`, `build-cargo-package` and
`build-cargo-release` install the toolchains a case needs through rustup, and
those of `check-python` the interpreter a case pins through uv, as the check and
the build do in a run. The cases of `build-cargo-release` build for the one
target of the register that is the machine's own, and add it to the toolchain:
in the gate `x86_64-unknown-linux-musl`; a machine the register lists no target
for refuses the build by name and the cases fail. The cases of
`build-docker` build, start and scan their images with the machine's Docker
and its Buildx, as the build does in a run: they need an `x86_64` Linux machine
with a running Docker daemon, as the gate's runner is; elsewhere the tool
refuses the build by name and the cases fail. Before the cases of each check and
build it sets up the programs the register names for it with `nebensound-repo
setup --pinned <name>`, the first call of a block that needs programs: it
installs the pinned version of every program the check or build calls where it
is missing (for `cargo-package`, none), so that the cases do not depend on which
blocks ran before the xtask. A runtime one of those programs needs, such as the
Node.js of Stylelint, html-validate, Prettier and markdownlint, is the one
`nebensound-repo` finds: a pinned one in the tool's folder first, else the
computer's, when it is new enough for the programs. On a developer's computer
the setup goes without `--with-runtimes` and installs no runtime, so a missing
or too old one is installed only by `cargo xtask setup --with-runtimes` or with
a yes at the prompt. In a run on GitHub Actions, which is how the gate runs the
xtask, the setup goes with `--with-runtimes`, as the block's own call does: a
Node.js the runner image lacks or carries too old is installed in the version
the register pins, into the tool's folder, where `nebensound-repo run` finds it
and puts it in front of the `PATH` of the programs that need it. So the cases
pass whatever Node.js the image carries; one that is new enough is used as it
is. A setup that fails is a problem of its own, and the block's cases still run
with the programs that are there.

A passing case needs every call to succeed. A failing case needs one to fail
and the tool to print every finding the case names, so a call that fails for
another reason does not count. The xtask runs no block's own steps: a block
whose work the tool does not carry out is a problem of its own, named with the
reason that its cases cannot run.

The action `tool` is no block, and its cases do not lie beside it: the one step
that decides something before the tool exists, the one that fetches and
verifies the release's binary, is run by a test of the xtask
(`xtask/tests/tool_action.rs`) as a runner runs it, with stand-ins for Git, gh
and curl, against a case for every way it ends: the verified binary on each of
the three systems, a binary that does not verify, and every reason for a build
from source.

A call of the tool for a case gets only the variables a step on a runner has
too: `PATH`, `HOME`, the user, the locale, where Cargo, rustup and Go keep their
files, the proxy and certificate settings, and the `XDG_` folders. A variable
that steers a program, such as `CARGO_TERM_COLOR`, `RUSTFLAGS` or
`NODE_OPTIONS`, would otherwise make a case print something else locally than
in the gate. Beside them it gets `CI`, `GITHUB_ACTIONS` and `RUNNER_TEMP`, as in
a workflow run. Phase 0 runs with the name `case`, the default branch `main` and
the visibility `public`, so that a case gets no finding for any of them.

What a call of the tool prints stands in the log behind a `|`, so that a
reader tells it from the xtask's own lines. In the gate the xtask is a program
`nebensound-repo xtask` starts, so everything it prints stands in a stretch in
which the runner processes no workflow command: neither the findings of a
failing case nor a name from a case start a command there, and what the xtask
itself finds is no annotation of the gate either, but a line of the log and the
failure of the step.

The cases run wherever the tool does, on Linux and macOS alike, except those of
`build-docker`, which need an `x86_64` Linux machine with Docker, and those of
`build-cargo-release`, which need a machine the register lists a target for
(see above).

`cargo xtask setup` calls `nebensound-repo setup` for this repository with the
tool of this commit, built from the package at the root of this repository
with the toolchain the tool pins, hands it every option unchanged (`--pinned`, `--with-runtimes`,
`--check`, the names of checks and builds), and exits with its exit code. The
cases take the pinned programs `cargo xtask check --all` sets up itself, and a
runtime from the computer or from `--with-runtimes`, which `cargo xtask check
--all` passes itself only in a run on GitHub Actions.

The cases are
taken out of this repository's own checks with `[[exclude]]` and a reason in
[`.nebensound/repo.toml`](.nebensound/repo.toml). A case of `check-rust` is a
whole Cargo package, the only way Rust code can be excluded, and so is a case
of `build-cargo-package` or `build-cargo-release`. None of them belongs to the
Cargo workspace at the root: its manifest excludes everything below
`.github/actions`, so Cargo run inside a case finds the case and not that
workspace.

## Licence

Licensed under the [European Union Public Licence v. 1.2](LICENSE) (EUPL-1.2).
