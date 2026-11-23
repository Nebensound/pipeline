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
subcommand's name matched it: the programs it starts are those of the register,
Git, curl and Cargo.

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
- [Calling the workflow](docs/calling.md): how to pin before `v0.1.0`, when
  the caller starts the workflow, which permissions it grants and why, why the
  schema line can run ahead of the pin, and how to change one linter rule in
  one place.

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
    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@<full commit SHA>
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
change to the pipeline never reaches a repository as a silent switch. Until
`v0.1.0`, the first version meant for callers, it is pinned to the full
40-character SHA of a commit on `main` and raised by hand; from then on to a
release tag, which Dependabot raises as a pull request.

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

A run reports as `nebensound / Declare` (phase 0: the declaration read and
checked against the tree), `nebensound / Version` (the version of every unit,
as a table in the summary of the run), one job per unit that runs a check
(`nebensound / Check repository`, `nebensound / Check component <name>`),
`nebensound / Check`, which sums the units up, one job per unit that builds
and has a version of the run, that is, one the commit touches
(`nebensound / Build repository`, `nebensound / Build component <name>`), and
`nebensound / Build`, which sums the builds up. After a push to `main`,
`nebensound / Publish` tags and releases every unit with a version of the run
(see [the phases](docs/phases.md#publish)). A ruleset requires
`nebensound / Version`, `nebensound / Check` and `nebensound / Build`. A check
or a build the register knows but that has no building block yet fails by name
in the step `Not written yet` of its unit's job, and a version writer or a
publish target without one in the step `Version writers not written yet` or
`Publish targets not written yet` of `nebensound / Check`.

This repository calls the workflow the same way, with one difference: its own
[`gate.yml`](.github/workflows/gate.yml) calls the workflow at the commit that
is running, so a change to the pipeline is checked by the pipeline it changes.
Its declaration gives the tool `nebensound-repo` neither a version nor a build,
so the repository's own tags `vX.Y.Z` are the only ones, and its Build phase has
nothing to build. Every merge to `main` touches the repository and is released
in full, as `vX.Y.Z`, the latest release, with nothing attached. The
version writer `cargo` and the build `cargo-package` are held to their test
cases instead.

## Building blocks

Every check block checks one unit, and exactly the files `nebensound-repo files
<check> <unit path>` names: the files Git tracks below the unit's path whose
extension the register gives that check, less what an `[[exclude]]` entry of
the declaration takes out. A file nobody committed is not judged, and a
committed one is not missed. Each finding is an annotation on its file. A
version writer writes the versions of the plan, and a build block builds one
unit (see [`build-cargo-package`](#build-cargo-package) and
[`build-docker`](#build-docker)).

The unit job gets the tool the same way `Declare` does, from the cache phase 0
fills ([`.github/actions/tool`](.github/actions/tool)); it builds the tool
only when the cache misses. The list does not travel through a job output,
which is limited in size.

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
html-validate, markdownlint-cli2, Stylelint and yamllint are run through their
internals, so that the repository's rules are laid over the pipeline's default,
and of those four `run` takes only the pinned copy. What `run` hands the
programs, its configuration for Biome, the schemas for Taplo, the pipeline's
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
  ([`src/run/html.rs`](tools/nebensound-repo/src/run/html.rs)), switched on
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

- `.json` and `.geojson` are strict JSON, exactly one value per file, judged by
  `jq`.
- `.jsonc` is strict JSONC as [jsonc.org](https://jsonc.org) defines it: JSON
  plus comments, trailing commas allowed. Biome judges it as a parser only, with
  the tool's own configuration
  ([`run/biome.json`](tools/nebensound-repo/run/biome.json)) and no lint rule;
  a repository's Biome configuration is not read.
- `composer.lock`, `flake.lock` and `Pipfile.lock` are strict JSON like
  `.json`, judged by `jq`, but not format-checked: the tool that writes them
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
  pipeline's default (see [Linter rules](#linter-rules)). Its `ignores`, a
  negated `globs` entry and `gitignore` take nothing out.
- Every link into the repository leads to a file and a heading that exist
  (lychee, `--offline`). Links to the outside are not checked in a pull
  request: it should not turn red because someone else's server does not
  answer. lychee reads the repository's `lychee.toml` and `.lycheeignore` at
  the root, so excluded addresses, accepted status codes and timeouts stay the
  repository's configuration. An `exclude_path` there is held against the
  declaration by phase 0, like every setting that takes files out (see
  [Exclusions](#exclusions)). `run` names `lychee.toml` as the
  configuration, so a lychee table in `Cargo.toml`, `pyproject.toml` or
  `package.json`, which lychee would otherwise read where there is no
  `lychee.toml`, is not read. lychee's report stands in the output, and every
  broken link in it is a finding on the file that holds it, at the line and
  column lychee names.

### `check-python`

[`.github/actions/check-python`](.github/actions/check-python) checks one uv
project, the unit's path: its `pyproject.toml` lies there, and its `uv.lock`
there or at the root of the uv workspace above it. The interpreter is the one
`.python-version` names, at the unit or at that root, installed by uv from its
own downloads and never taken from the machine; like the toolchain of
`check-rust`, it is no input. uv 0.13.9, Ruff 0.17.6, Pyright 1.1.414 on
Node.js 24.22.3 and coverage.py 7.17.4 are pinned by their digests, as
[the register](docs/register.md#programs) pins them. pytest and its plugins are
the project's own, from its lockfile, since they import its code. coverage.py
runs on Python 3.11 or later, so the project's interpreter has to be one.

- The project installs as its `uv.lock` pins it (`uv sync --locked`), with its
  default dependency groups. A missing `pyproject.toml`, `uv.lock` or
  `.python-version` fails by name.
- Formatted the way Ruff writes it (`ruff format --check`).
- No Ruff finding, with the repository's configuration laid over the
  pipeline's default (see [Linter rules](#linter-rules)).
- No Pyright error in strict mode, with the repository's configuration laid
  over it (see [Linter rules](#linter-rules)).
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
  committed afterwards. The wheel is never handed on.

A file an `[[exclude]]` entry takes out is handed to no tool: Ruff and Pyright
get the list, pytest does not collect such a file, and coverage does not judge
it. An entry that takes out the unit's whole folder leaves nothing to check.
Ruff runs with `--no-force-exclude`, so its `exclude` takes no listed file out;
Pyright's `ignore` and `exclude` are set aside; coverage.py's `omit` counts for
nothing, since a file it leaves out counts as not covered.

Every step runs even when one before it failed, so one run shows every finding;
a step that needs the installed project is left out when installing it failed.
uv's cache, the interpreter and the project's environment lie in the runner's
temporary folder, and Ruff, pytest and Python write no cache and no bytecode
into the checkout.

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
  contents, outside the source tree. The versions are written first, as
  [`version-cargo`](#version-cargo) writes them, and put back when the step
  ends: a package without a version is not published, so only a unit with a
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
  `tools/nebensound-repo/schemas/`, where they lie with their licence notice
  (see [their README](tools/nebensound-repo/schemas/README.md)), so the check
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
https://raw.githubusercontent.com/Nebensound/pipeline/main/tools/nebensound-repo/schemas/nebensound-declaration.json
```

An editor that reads Taplo's directive finds it through this line before the
first key or table of `.nebensound/repo.toml`:

```toml
#:schema https://raw.githubusercontent.com/Nebensound/pipeline/main/tools/nebensound-repo/schemas/nebensound-declaration.json
```

### `check-yaml`

[`.github/actions/check-yaml`](.github/actions/check-yaml) checks every `.yml` and `.yaml` file of the unit:

- Formatted the way Prettier writes it, run as in `check-json`.
- No yamllint finding, with the repository's rules laid over the pipeline's
  default (see [Linter rules](#linter-rules)). Its `ignore` and
  `ignore-from-file` take nothing out.
- Every workflow directly under `.github/workflows` passes actionlint, with
  ShellCheck reading every `run:` script and pyflakes every Python one where
  they are on the search path, as ShellCheck is on GitHub's Linux runners; the
  output says whether each was found. actionlint 1.7.12 does not know `$/`
  yet and reports every such call as invalid; `run` drops exactly these two
  messages, and only for a `uses:` that begins with `$/`. Nor does it know
  the key `queue` of a `concurrency` section, which GitHub documents; `run`
  drops the message about that one key, and every other unknown key of the
  section is still a finding.
- `.github/dependabot.yml` matches the Dependabot schema (`check-jsonschema`,
  pinned, with the schema it brings along). An unknown key fails the check
  with the file and the key named, instead of surfacing only after the merge.

### `version-cargo`

[`.github/actions/version-cargo`](.github/actions/version-cargo) is the version
writer `cargo`. No manifest in the tree carries a version: the history is the
truth, and Cargo cannot take a version from outside. So the version is written
into the checkout of the run, immediately before something builds with it, and
never committed. The block hands the plan Declare printed to
`nebensound-repo write-versions --plan`, since its job checks out no history to
resolve a version from. Every unit of the plan with the writer `cargo` that
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
Otherwise the job `nebensound / Version` fails with
[`unreleased-requirement`](docs/declaration.md#unreleased-requirement), which
names the way out: touch the required unit in the same pull request. Whether
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
one its merge would get, with `-pr.<number>` behind it.

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

### `build-docker`

[`.github/actions/build-docker`](.github/actions/build-docker) is the build
`docker`. It builds the image of one unit and runs in the unit's job of the
Build phase, after the version writers the unit selects; it gets the plan to
read the unit's version of the run from it.

- The Dockerfile is `deploy/Dockerfile` below the unit's path, and the build
  context is the repository root. Each unit has its own, so several services of
  one workspace each build their own image. Phase 0 refuses a unit without
  `deploy/Dockerfile`
  ([`build-without-file`](docs/declaration.md#build-without-file)). The
  block's first step keeps a check of its own only as a safeguard: it fails by
  name where the checkout cannot read a file phase 0 saw, such as a tracked
  symlink whose target is missing.
- The image is built once, for `linux/amd64` only, on an `x86_64` runner, by a
  BuildKit of its own pinned by digest, without a cache, without provenance and
  without an SBOM, into an OCI layout. It carries the labels
  `org.opencontainers.image.version` (the unit's version of the run, on a pull
  request with `-pr.<number>` behind it), `.revision` (the commit), `.source`
  (the repository's address) and `.title` (the unit's name, the repository's
  for the repository itself).
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
  configuration file of Trivy is read, and there is no exception.
- The layout is uploaded as an artifact of the run named
  `container-<path>@<version>@<sha>`, the path percent-encoded as for a crate.
  It holds exactly the file `image.tar`. Nothing is pushed. A second attempt of
  the same job replaces it.

Nothing is cached, for the same reason as for a crate. What the block started
(the container, its volumes, the loaded image and the builder) is removed at
the end, whatever happened.

### Linter rules

Markdownlint, yamllint, Stylelint and html-validate run with a default of the
pipeline:
[`tools/nebensound-repo/rules/markdownlint.jsonc`](tools/nebensound-repo/rules/markdownlint.jsonc)
(`MD013` off, since Prettier does not wrap prose),
[`tools/nebensound-repo/rules/yamllint.yaml`](tools/nebensound-repo/rules/yamllint.yaml)
(yamllint's defaults, less what contradicts Prettier or GitHub Actions),
[`tools/nebensound-repo/rules/stylelint.yaml`](tools/nebensound-repo/rules/stylelint.yaml)
(`stylelint-config-standard`, for `.scss` `stylelint-config-standard-scss`) and
[`tools/nebensound-repo/rules/html-validate.json`](tools/nebensound-repo/rules/html-validate.json)
(the presets `recommended` and `a11y`, less what contradicts Prettier through
html-validate's own preset `prettier`). `run` hands each linter the default
compiled into the tool of the pipeline version that runs. A repository's own
configuration is laid over that default rule by rule: its file names only the
rules it wants different, every rule it does not name keeps the default's value,
and a later change of the default reaches it without a change of its own.
`nebensound-repo check` shows, per check, every rule the repository sets and how
it stands to the default (see
[the tool's README](tools/nebensound-repo/README.md#the-rules)).

A repository whose pull request template begins on a second-level heading
needs one Markdownlint rule changed, for that folder only. Instead of an inline
`<!-- markdownlint-disable-file MD041 -->` in the template,
`.github/.markdownlint-cli2.jsonc` holds the one rule:

```jsonc
{
  "config": {
    "MD041": false,
  },
}
```

`MD013` stays off there and everywhere else, as the default has it. Where the
rule is to hold in the whole repository, the same file stands at the root.
markdownlint-cli2 lays the `config` of a `.markdownlint-cli2.jsonc` or
`.markdownlint-cli2.yaml` over what lies above it rule by rule, in every
folder. A `.markdownlint.*` file, which holds rules only, it takes instead of
everything above it; the block lays the one at the root over the default all
the same, and where the root holds both, the options file lies over it. One in
a folder below the root still takes the place of everything above it in its
folder, the default included, and of the `config` of a `.markdownlint-cli2.*`
beside it, so the rules of a folder belong in its `.markdownlint-cli2.*`.

A repository that wants a comment to need no space after its `#` writes one
option of one yamllint rule into `.yamllint.yaml` at its root:

```yaml
rules:
  comments:
    require-starting-space: false
```

yamllint lays it over the default the way it lays a configuration over the one
its `extends` names: the default's other option of that rule, one space before
a comment, stays, and so does every other rule, such as `line-length` off. An
`extends` in the repository's file brings what it names along, over the
default as well: `extends: default` brings back yamllint's own line length.
yamllint reads one configuration, at the root (`.yamllint`, `.yamllint.yaml`
or `.yamllint.yml`).

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
[`tools/nebensound-repo/rules/ruff.toml`](tools/nebensound-repo/rules/ruff.toml),
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
on (see [the tool's README](tools/nebensound-repo/README.md#the-rules)).

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
except lychee's `exclude_path`, which comes along with the `lychee.toml`
`check-markdown` and `check-html` read, and rustfmt's `ignore`, which comes
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
matches it against the path from the root, such as `docs/a.md`, the string the
block hands lychee. A setting that takes out less is fine. Phase 0 reads:

| Tool              | Setting                                                                                                  |
| ----------------- | -------------------------------------------------------------------------------------------------------- |
| Prettier          | `.prettierignore` at the root                                                                            |
| Taplo             | `include` and `exclude` in `.taplo.toml` or `taplo.toml` at the root                                     |
| markdownlint-cli2 | `ignores` in `.markdownlint-cli2.jsonc` or `.markdownlint-cli2.yaml`, in every folder                    |
| yamllint          | `ignore` and `ignore-from-file` in `.yamllint`, `.yamllint.yaml` or `.yamllint.yml` at root              |
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
reported at that file only. Where
yamllint's file sets neither `ignore` nor `ignore-from-file`, phase 0 reads them
in the file its `extends` names, as yamllint does. rustfmt's `ignore` is read in
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
(`.markdownlint-cli2.cjs` or `.mjs`) is not judged, since phase 0 cannot read it
without running it; the block sets its `ignores` aside all the same. A setting
that switches a rule off for some files, such as yamllint's per-rule `ignore`
or markdownlint-cli2's `overrides`, takes no file out of the tool and stays the
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
  ([the tool's README](tools/nebensound-repo/README.md#setting-up-the-programs)),
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
line that is not empty and does not start with `#` must match a whole line the
block prints, with `*` standing for any text, for example
`::error file=data.json::not formatted the way Prettier writes it`. That file is
not part of the case's repository.

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
  the name of its block, the dots of a target turned into hyphens;
- every block has a passing and a failing case. A `.DS_Store` beside the cases
  is passed over: Git ignores it, so the gate never sees it.

For now the xtask knows only how a check, a version writer and a build are
called and tested, so a `publish-` block is reported until the xtask learns
that kind. The first one teaches it, together with its cases.

Then it runs every block against its cases, each copied into a fresh Git
repository. The blocks whose check runs from the tool (`check-css`,
`check-dockerfile`, `check-html`, `check-json`, `check-markdown`, `check-toml`,
`check-yaml`) hold nothing of their check in their steps, so their cases run
through the tool itself: the xtask builds the tool of this commit with Cargo,
runs phase 0 on the case with `nebensound-repo check` and `plan`, as `Declare`
does, and calls
`nebensound-repo run` once for every unit the plan starts the check for, in the
environment of a workflow run, so that the findings are annotations. Before
the cases of each such block it sets up the programs of its check with
`nebensound-repo setup --pinned <check>`, the first of the two calls the block
makes: it installs the pinned version of every program the check calls where it
is missing, so that the cases do not depend on which blocks ran before the
xtask. A runtime one of those programs needs, such as the Node.js of Stylelint,
html-validate, Prettier and markdownlint-cli2, is the one `nebensound-repo`
finds: a pinned one in the tool's folder first, else the computer's, when it is
new enough for the programs. On a developer's computer the setup goes without
`--with-runtimes` and installs no runtime, so a missing or too old one is
installed only by `cargo xtask setup --with-runtimes` or with a yes at the
prompt. In a run on GitHub Actions, which is how the gate runs the xtask, the
setup goes with `--with-runtimes`, as the block's own call does: a Node.js the
runner image lacks or carries too old is installed in the version the register
pins, into the tool's folder, where `nebensound-repo run` finds it and puts it
in front of the `PATH` of the programs that need it. So the cases pass whatever
Node.js the image carries; one that is new enough is used as it is. A setup
that fails is a problem of its own, and the block's cases still run with the
programs that are there.

Every other block runs its own steps. The xtask builds the tool through
[`.github/actions/tool`](.github/actions/tool), runs
[`.github/actions/phase0`](.github/actions/phase0) on the case, and runs a
check block once for every unit the plan starts the block's check for, just as
a unit job does, with the plan when the block takes one. A version
writer's block runs once with the plan, provided a unit of the plan selects the
writer. A build block runs once for every unit the plan starts the build for
and gives a version of the run, just as a build job does: first the block of
every version writer the unit selects, with the plan, and the build block only
when they passed, with the plan when it takes one. A passing case needs the
block to succeed everywhere. A failing case needs it to fail and to print every
finding the case names, so a block that fails for another reason does not
count.

The xtask keeps no copy of what such a block does. It reads every step from the
block's `action.yml` and runs it the way the runner of GitHub Actions does: it
decides each `if:`, fills in every `${{ … }}`, hands each `run:` to bash and
reads back what the step writes to `GITHUB_OUTPUT`, `GITHUB_ENV` and
`GITHUB_PATH`. A composite action a block calls is fetched at the ref the block
names, a branch, a tag or a full commit SHA, and run the same way. An action
that runs on Node.js cannot run like that; for the ones it knows, the xtask
stands in:

| Action                                                               | What stands in                                                                                                 |
| -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `actions/setup-node`, `actions/setup-go`                             | The tool already on the PATH, in the version the step asks for; anything else fails the step.                  |
| `Swatinem/rust-cache`, `actions/cache/restore`, `actions/cache/save` | Nothing: a cache changes how fast a step runs, not what it finds.                                              |
| `actions/upload-artifact`                                            | Nothing leaves the machine, but every path the step names has to exist, as `if-no-files-found: error` demands. |

Any other such action fails the xtask until it has a stand-in. No block calls
`actions/setup-node` or `actions/setup-go` today, so no case depends on the
Node.js already on the PATH: the cases of the checks that run from the tool get
Node.js the way their setup above finds it, in the gate the runner image's when
it is new enough and otherwise the one the register pins, installed with
`--with-runtimes`; `check-python` downloads the Node.js that Pyright runs on
itself. The two stand-ins stay, so that a block calling one of them
is run as well; for such a step the xtask takes the program already on the
PATH and fails the step unless it is the version the step asks for. One step
does not run:
`Make room on the runner` of `check-rust`, which deletes software of the machine with `sudo`; on a developer's machine,
that software would be theirs. A step the xtask leaves out has to exist, so
renaming it fails the xtask instead of letting the step run.

A step gets only the variables a step on a runner has too: `PATH`, `HOME`, the
user, the locale, where Cargo, rustup and Go keep their files, the proxy and
certificate settings, and the `XDG_` folders. A variable that steers a tool,
such as `CARGO_TERM_COLOR`, `RUSTFLAGS` or `NODE_OPTIONS`, would otherwise make
a block print something else locally than in the gate. Of the variables the
runner sets for every step, a step gets `CI`, `GITHUB_ACTIONS`, the `RUNNER_`
and file-command variables, and fixed values for the repository and the run
(`GITHUB_REPOSITORY`, `GITHUB_RUN_ID` and their kin), which an action a block
calls may read; a case belongs to no repository on GitHub and to no run. The
expressions `github.*`, `runner.*` and `job.*` know only the names the blocks
and the actions they call read, with the same values; any other name fails the
xtask, since on GitHub it would have a value the xtask cannot give. Phase 0
reads the name and the default branch of the event's repository, which are
`case` and `main`, so that a case gets no warning for either. What a step
installs with `cargo install` goes into a folder of the run that comes first on
its `PATH`, so the cases leave the developer's own programs as they are.

What a step or a call of the tool prints stands in the log behind a `|`, so
that the findings of a failing case do not become annotations of the gate. What
the xtask itself finds is an annotation.

The cases of the blocks that run from the tool run wherever the tool does, on
Linux and macOS alike. The steps of the other blocks are written for the Linux
runners of GitHub Actions, and so for now their cases run on Linux only, in the
gate on every pull request and locally on Linux. Elsewhere the xtask holds the
blocks, the register and `nebensound.yml` to each other, runs the cases of the
blocks that run from the tool, names the other blocks in one sentence and exits
with `1`, since their cases did not run.

`cargo xtask setup` calls `nebensound-repo setup` for this repository with the
tool of this commit, built from `tools/nebensound-repo/` with the toolchain the
tool pins, and hands it every option unchanged (`--pinned`, `--with-runtimes`,
`--check`, the names of checks). The cases of the blocks that run from the tool
take the pinned programs `cargo xtask check --all` sets up itself, and a
runtime from the computer or from `--with-runtimes`, which `cargo xtask check
--all` passes itself only in a run on GitHub Actions. Then, on Linux, it checks
what only the cases of the other blocks need: a program an action of their
steps would put on the `PATH`, in the version the step asks for (read from
their `action.yml`), Docker with Buildx, jq and rustup. It
installs none of these, since they stay the machine's, but names each one that
is missing or of another version and exits with `1` then, so that a machine it
passes can run the cases. `nebensound-repo setup` may have put a jq into its
cache, but the steps look for jq on the `PATH`. That the Docker
daemon runs is not checked; the cases of `build-docker` fail without it. On
another system those cases do not run, so `setup` leaves out what only they
need and says so, and its exit code is the tool's.

The cases are
taken out of this repository's own checks with `[[exclude]]` and a reason in
[`.nebensound/repo.toml`](.nebensound/repo.toml). A case of `check-rust` is a
whole Cargo package, the only way Rust code can be excluded.

## Licence

Licensed under the [European Union Public Licence v. 1.2](LICENSE) (EUPL-1.2).
