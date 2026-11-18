# The declaration

Every repository that calls the pipeline describes itself in one file,
`.nebensound/repo.toml`. This page describes every field of that file, what it
defaults to and what it does, and every finding phase 0 can report about it.

The names a field refers to (presets, checks, builds, targets, version writers,
ecosystems) are listed in [the register](register.md). What runs when, and under
which name, is described in [the phases](phases.md).

## The file

```toml
#:schema https://raw.githubusercontent.com/Nebensound/pipeline/main/.github/actions/check-toml/schemas/nebensound-declaration.json

[repo]
pipeline = "none"

[component.tool]
path       = "tools/nebensound"
pipeline   = "rust-workspace"
versioning = "none"

[[exclude]]
path   = "docs/diagram.drawio"
reason = "drawn in an editor that writes it; no tool here formats it"
```

- The file lies at exactly `.nebensound/repo.toml`. A repository without it
  fails phase 0: without the file, "nothing is published here" cannot be told
  apart from "nobody ever set this up". A link or anything else that is not a
  regular file is refused.
- It is TOML. Every key that is not described here is an error, so a misspelt
  key fails instead of quietly meaning nothing.
- `[repo]` is required. Everything else is optional.
- The `#:schema` line gives an editor the
  [JSON Schema of the file](../.github/actions/check-toml/schemas/nebensound-declaration.json),
  for completion and for marking an unknown key where it is written. The
  `check-toml` building block checks the file against the same schema whether
  the line is there or not. Phase 0 is the judge: the schema lets through some
  files that phase 0 then rejects, never the other way round.
- Phase 0 reads the file in every run, in the job `nebensound / Declare`, and
  checks it against what Git tracks in the repository: `git ls-files`, so a
  file nobody added is not judged, and a committed one is not missed.

## Units

A unit is a part of the repository that the pipeline treats as a whole: it has a
pipeline, and it may carry a version.

- **The repository** is always a unit. Its path is `.`, and `[repo]` describes
  it.
- **A component** is a unit declared under `[component.<name>]`. Its `path`
  names a folder, or a single file when exactly that file is the unit.
- **A generated unit** is brought into being by a `versioning` value that names
  children: one unit per folder or per file directly below the declaring unit
  (see [Versioning](#versioning)).

### How a file belongs to units

A file belongs to every unit whose path contains it. Paths are compared by
segment, not by text: `crates/foo` contains `crates/foo/src/lib.rs`, and it does
not contain `crates/foobar`. Units may nest, and two units may name the same
path.

For each file, phase 0 decides one of four things:

| Mark | Meaning                                                                                                                 |
| ---- | ----------------------------------------------------------------------------------------------------------------------- |
| `✓`  | A check covers the file, and it is in force here: in the base set, or in the pipeline of a unit that contains the file. |
| `×`  | A check would cover the file, and it is not in force for any unit that contains it. An error: `uncovered-file`.         |
| `?`  | No check in the register covers this kind of file. An error: `language-without-check`.                                  |
| `–`  | An exclusion takes the file out, with its reason.                                                                       |

Which check covers a file is decided by its extension, compared in lowercase
(`README.MD` is Markdown), or for some checks by its whole file name
(`Dockerfile`). A leading dot does not start an extension: `.gitignore` has
none. The register lists every check with its extensions and file names.

### Where a check runs

The checks in force for a file are the base set and the checks of every unit
that contains it. A check runs **once, at the outermost unit it is in force
for**: the base set runs at the repository, and a check a component declares
covers everything below that component, so a unit inside it that names the same
check again starts no second run over the same files. When two units name the
same path, the one listed first keeps the check: the repository, then the
declared components by name, then the generated units.

## Fields

The tables below give every field with its type, its default when it is left
out, and its effect.

### The top level

| Field       | Type                    | Default  | Effect                                                                               |
| ----------- | ----------------------- | -------- | ------------------------------------------------------------------------------------ |
| `repo`      | table                   | required | The repository as a unit. See `[repo]` below.                                        |
| `component` | table of tables by name | none     | The components, each a unit of its own. See `[component.<name>]` below.              |
| `include`   | array of tables         | none     | What is pulled back into the check after an exclusion. See `[[include]]` below.      |
| `exclude`   | array of tables         | none     | What is taken out of the check, each entry with its reason. See `[[exclude]]` below. |

### `[repo]`

| Field             | Type                    | Default                        | Effect                                                                                                                                                   |
| ----------------- | ----------------------- | ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pipeline`        | preset name or table    | nothing is declared            | What runs for the repository beyond the base set. See [Pipeline](#pipeline).                                                                             |
| `versioning`      | string                  | `"self"`                       | Which units around the repository carry a version. Only the values that include `self` are open to the repository. See [Versioning](#versioning).        |
| `release`         | string                  | decided by the publish targets | `"single-stage"` or `"two-stage"`: how the unit's version reaches a full release. See [Releases](#releases).                                             |
| `minimum-version` | version `"X.Y.Z"`       | none                           | The version the unit gets is never lower than this. See [Releases](#releases).                                                                           |
| `allow-unused`    | table of check → reason | none                           | Checks of this unit's own pipeline that may find no file without failing, each with the reason why it stays. Without an entry, such a check is an error. |
| `component`       | table                   | none                           | The pipeline a component can ask for with `pipeline = { repo = true }`. See `[repo.component]` below.                                                    |

### `[repo.component]`

A default for components, offered only on request: a component that leaves its
`pipeline` out does not receive it.

| Field      | Type                 | Default | Effect                                                                                                      |
| ---------- | -------------------- | ------- | ----------------------------------------------------------------------------------------------------------- |
| `pipeline` | preset name or table | none    | The pipeline a component with `pipeline = { repo = true }` receives. It cannot itself be `{ repo = true }`. |

### `[component.<name>]`

The name is the key of the table. It identifies the unit in the report of phase
0, in the plan and in the name of its job (`nebensound / Check component <name>`).
A name holds lowercase letters, digits, `-` and `.`, starts and ends on a letter
or digit, and holds no `..`; it is meant to survive a Git tag, a file name, a
shell argument and a URL without quoting. No two units may share a name,
generated ones included.

| Field             | Type                    | Default                        | Effect                                                                                                                                                   |
| ----------------- | ----------------------- | ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `path`            | path                    | required                       | Where the component lives: a folder, or a single file. It has to hold something Git tracks, otherwise `missing-path`.                                    |
| `pipeline`        | preset name or table    | nothing is declared            | What runs for the component beyond the base set. See [Pipeline](#pipeline).                                                                              |
| `versioning`      | string                  | `"self"`                       | Which units around the component carry a version. See [Versioning](#versioning).                                                                         |
| `release`         | string                  | decided by the publish targets | `"single-stage"` or `"two-stage"`: how the unit's version reaches a full release. See [Releases](#releases).                                             |
| `minimum-version` | version `"X.Y.Z"`       | none                           | The version the unit gets is never lower than this. See [Releases](#releases).                                                                           |
| `allow-unused`    | table of check → reason | none                           | Checks of this unit's own pipeline that may find no file without failing, each with the reason why it stays. Without an entry, such a check is an error. |

### `pipeline`

What runs for a unit. It takes one of three forms, and nothing in between:

- **A preset name**, such as `pipeline = "rust-library"`: everything the preset
  says, looked up in the register. A preset cannot be partly overridden; to
  change one part, write the table out.
- **`pipeline = { repo = true }`**: whatever `[repo.component]` offers. It
  cannot be combined with any other key, and `repo = false` is an error: leave
  the pipeline out instead.
- **A table written out**, as `pipeline = { check = ["rust"] }` or as a table
  of its own (`[component.<name>.pipeline]`), with the fields below.

Left out, a unit runs nothing beyond the base set: no version writer, no check
of its own, no build, nothing published, no ecosystem watched beyond the base
set.

| Field        | Type                             | Default | Effect                                                                                                                                                                                      |
| ------------ | -------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `repo`       | `true`                           | absent  | Takes the pipeline `[repo.component]` offers. Only in the form `{ repo = true }`, alone.                                                                                                    |
| `version`    | array of version writer names    | `[]`    | The version writers that receive the unit's version as a derived copy. Phase 0 demands that the unit, or the children it generates, carries a version. No version is written yet.           |
| `check`      | array of check names             | `[]`    | Checks beyond the base set. Each is in force for every file of the unit and runs in the Check phase.                                                                                        |
| `build`      | build name                       | none    | The build that produces the unit's artifact. The plan names it for a unit that carries a version of its own and generates no children. The Build phase is not built yet.                    |
| `publish`    | `false` or array of target names | `false` | Where the artifact goes: `false` for nowhere, said out loud, or a list of targets. `publish = true` is an error, since it names no target. Nothing is published yet.                        |
| `dependabot` | array of ecosystem names         | `[]`    | The ecosystems Dependabot is to watch for this unit, beyond the base set. Phase 0 compares them with the manifests the unit holds. Nothing writes a Dependabot configuration from them yet. |

### `[[include]]`

Pulls a file back into the check that an exclusion takes out. Adding work needs
no justification, so the reason is optional, and an inclusion may be a pattern.

| Field    | Type            | Default  | Effect                                                             |
| -------- | --------------- | -------- | ------------------------------------------------------------------ |
| `path`   | path or pattern | required | What it applies to. See [Paths and patterns](#paths-and-patterns). |
| `reason` | string          | none     | Why, if anyone writes it down. When given, it must not be blank.   |

### `[[exclude]]`

Takes a file out of the check, with the reason why nobody looks at it. In a
repository's declaration only a concrete path may be excluded, never a pattern:
a pattern would excuse a whole kind of file in a single line, and which kinds
nobody checks is decided centrally (see
[Excluded everywhere](register.md#excluded-everywhere)). The same path may stand
only once.

| Field    | Type   | Default  | Effect                                                                     |
| -------- | ------ | -------- | -------------------------------------------------------------------------- |
| `path`   | path   | required | The file, or the folder whose files are taken out.                         |
| `reason` | string | required | Why nobody looks there. It must say something: a blank reason is an error. |

Today an exclusion settles a file for phase 0. The building blocks do not read
`[[exclude]]` yet; until they do, they honour the exclusions a repository gives
its tools, as [the README](../README.md#exclusions-for-now) describes.

## Paths and patterns

A **path** is a path inside the repository, separated by `/`: `.` for the
repository itself, never absolute, no `.` or `..` segment, no empty segment, no
backslash. A trailing `/` is allowed and means nothing more. A path matches the
file it names and everything below the folder it names.

`[[include]]` takes a **pattern** as well, in one of two forms:

| Form        | Example      | Matches                                                                  |
| ----------- | ------------ | ------------------------------------------------------------------------ |
| `*.<ext>`   | `*.svg`      | Every file with that extension, anywhere.                                |
| `**/<name>` | `**/LICENSE` | Every file of that name, and everything below every folder of that name. |

When several entries match a file, the central ones and the declaration's
together, the **most specific** entry decides: a path beats every pattern, a
longer path beats a shorter one, and `**/<name>` beats `*.<ext>`. At equal
specificity the inclusion wins: adding work is always allowed, excusing it is
not.

## Versioning

`versioning` says which units around the declaring unit carry a version. Every
value names the whole effect, so reading it tells whether the declaring unit
itself is included.

| Value                      | The declaring unit | Generated units                        |
| -------------------------- | ------------------ | -------------------------------------- |
| `"self"`                   | carries a version  | none                                   |
| `"self-and-child-folders"` | carries a version  | one per folder directly below its path |
| `"self-and-child-files"`   | carries a version  | one per file directly below its path   |
| `"child-folders"`          | carries none       | one per folder directly below its path |
| `"child-files"`            | carries none       | one per file directly below its path   |
| `"none"`                   | carries none       | none                                   |

- The repository always carries a version, so only `self`,
  `self-and-child-folders` and `self-and-child-files` are open to it. Any other
  value is the error `repository-without-version`, and the repository is
  treated as `self`.
- A generated unit is named after its folder or file: the last segment without
  its extension, in lowercase, every character other than an ASCII letter or
  digit turned into `-`, and `-` trimmed from both ends. `Handbook v2.typ`
  becomes `handbook-v2`, and `Übersicht.typ` becomes `bersicht`. A name with no
  ASCII letter or digit left, such as `日本.typ`, is the error
  `ungeneratable-name`.
- A generated unit has nowhere to say anything of its own. It takes the
  pipeline, `allow-unused`, `release` and `minimum-version` of the unit that
  generated it, and carries a version of its own.
- A unit that generates children leaves the building to them, also when it
  carries a version beside theirs.

Whether a unit carries a version decides today whether the plan names its
build, and whether it may publish and select version writers. The report of
phase 0 names the file that is to hold the version: `.nebensound/version` for
the repository, `.nebensound/versions/<name>` for a component. Versions are not
computed, written or tagged yet.

## Releases

`release` says how a unit's version reaches a full release:

| Value            | Meaning                                                                                                                                                                                    |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `"single-stage"` | A merge to `main` is a full release.                                                                                                                                                       |
| `"two-stage"`    | A merge to `main` is a pre-release `X.Y.Z-pre.N`. A pull request from `main` into the branch `release`, merged with a merge commit, makes the commit it promotes the full release `X.Y.Z`. |

- Without the field, the unit's publish targets decide.
  [The register](register.md#publish-targets) names for every target how a
  unit that publishes there is released: two stages for `crates.io` and
  `private-registry`, one stage for `ghcr`, `pages` and `release-assets`. A
  unit with several targets takes two stages if any of them does, and a unit
  that publishes nothing takes one stage. A unit with the preset
  `rust-library-public` is therefore released in two stages, and one with
  `rust-service` in one.
- A value written on the unit wins over that default.
- Which mode holds is written down, never derived from whether the branch
  `release` exists: a deleted branch must not turn the next merge into a full
  release.

`minimum-version` is a floor: the version a unit gets is never lower. It is how
a person takes the step to `0.1.0` or `1.0.0`. It is three numbers, as
`"0.1.0"`, without a leading zero, a prefix such as `v` or a suffix. Anything
else stops phase 0, as described under
[When the file cannot be read](#when-the-file-cannot-be-read), and `check-toml`
marks it at its line and column.

A generated unit takes both from the unit that generated it. The report of phase
0 and `nebensound explain` show for every unit how it is released and its
minimum version, each with where it came from: written on the unit, the default
of its publish targets, or inherited from the unit that generated it.

Neither field changes a run yet: versions are not computed, tagged or released
so far.

## Findings

Phase 0 reports what is wrong with a declaration as findings. Each carries a
stable code; the wording of its message may improve, the code does not change.

- An **error** fails `nebensound / Declare`, and with it `nebensound / Check`.
  No plan is made, so no check runs.
- A **warning** stops nothing. It says that a statement in the declaration is
  not true, not that something is broken.

A finding reads like `error[uncovered-file] at src/main.rs: …`: the severity, the
code, the unit or path it is about, and a sentence. The report of phase 0 shows
each finding under the path it is about in the tree, and lists them all under
`Findings`.

### `unknown-preset`

Error. A `pipeline` names a preset the register does not have. The message lists
the presets it has. Name one of them, or write the pipeline out as a table.

### `unknown-register-entry`

Error. A pipeline written out names a version writer, check, build, target or
ecosystem the register does not have. Use a name from [the register](register.md).
A name that is missing there is added to the register in this repository, not in
the declaration.

### `no-requestable-default`

Error. A unit asks for `pipeline = { repo = true }`, and `[repo.component]`
offers no pipeline, or offers `{ repo = true }` itself. Give
`[repo.component]` a `pipeline`, or give the unit a pipeline of its own.

### `repository-without-version`

Error. `[repo]` sets `versioning` to a value without `self` (`child-folders`,
`child-files` or `none`). The repository always describes a defined state, so it
always carries a version. Use `self`, `self-and-child-folders` or
`self-and-child-files`, or move the children into a component.

### `missing-path`

Error. A component's `path` holds nothing Git tracks: the path is misspelt, the
folder is empty, or its files are not added. Correct the path, add the files, or
remove the component.

### `language-without-check`

Error. A file is not excluded, and no check in the register covers its kind.
Exclude it with a reason if nobody should look at it. If it is a kind of file
that belongs checked, the register needs a check for it, if need be one whose
building block fails until it is written.

### `childless-generator`

Warning. A unit's `versioning` promises children (`*-child-folders` or
`*-child-files`), and there is none directly below its path. Use a value
without children, or add them.

### `ungeneratable-name`

Error. A folder or file that a `versioning` value turns into a unit cannot be
named: nothing remains of its name once it is cleaned, or the name is already
taken by another unit. Rename the folder or file, or declare it as a component
with a name of its own.

### `uncovered-file`

Error. A check in the register covers the file, and that check is not in force
for any unit that contains it. Add the check to the pipeline of a unit that
contains the file, for example a component around it with a fitting preset, or
exclude the file with a reason.

### `unused-check`

Error. A unit's own pipeline names a check, and the unit holds no file that check
covers: its promise would be empty. Remove the check from the pipeline, or keep
it with an `allow-unused` entry that says why. The base set is exempt.

### `pointless-suppression`

Warning. `allow-unused` names a check that the unit's own pipeline does not run.
Remove the entry.

### `pointless-exclusion`

Warning. An `[[exclude]]` entry matches no file Git tracks, so its reason
excuses nothing. Remove the entry, or correct its path.

### `pointless-inclusion`

Warning. An `[[include]]` entry pulls nothing back: no file it matches would be
excluded without it. Remove the entry.

### `unreachable-target`

Error. A unit publishes to a target and builds nothing, or builds a kind of
artifact the target does not accept. The register lists what each build produces
and what each target accepts. Choose a build and targets that fit, or publish
nothing.

### `target-demands-check`

Error. A target demands a check (`crates.io` demands `rust`), and that check is
not in force for the unit: neither in its own pipeline nor in that of a unit
containing it. Add the check to the unit or to a unit around it.

### `publish-without-version`

Error. A unit publishes and carries no version of its own, so whatever it
publishes would have no identity to appear under. Give it a `versioning` value
that includes `self`, or let it generate children that carry one, or publish
nothing.

### `version-writer-without-version`

Error. A unit selects a version writer, carries no version of its own and
generates no children that carry one, so there is nothing to copy. Give it a
`versioning` value that includes `self`, or let it generate children, or remove
the version writer.

### `unwatched-manifest`

Warning. A file is the manifest of an ecosystem (`Cargo.lock` for `cargo`, for
example), and neither the base set nor a unit containing it has that ecosystem
watched. Add the ecosystem to the `dependabot` list of a unit that contains the
file.

### `pointless-watch`

Warning. A unit has an ecosystem watched and holds no manifest of it. Remove the
ecosystem from its `dependabot` list. An ecosystem of the base set is exempt.

### When the file cannot be read

Some mistakes stop phase 0 before any finding is made. It then prints one
sentence that names the file and the mistake, and fails:

- the file is missing, or is not a regular file;
- the TOML does not parse;
- a key is unknown, a required one is missing, or a value has the wrong type;
- a name, path or pattern breaks the rules above, or a reason is blank;
- `release` is neither `"single-stage"` nor `"two-stage"`, or
  `minimum-version` is not a version `X.Y.Z`;
- an `[[exclude]]` entry names a pattern, or the same path stands twice under
  `[[exclude]]`;
- a pipeline combines `repo = true` with other keys, or says `repo = false`, or
  a table says `publish = true`.

## Trying it locally

The same tool that runs phase 0 in the workflow runs on a checkout. From
`tools/nebensound/` in a checkout of this repository:

```sh
cargo run --quiet -- -C <repository> check           # the whole report
cargo run --quiet -- -C <repository> explain <path>  # what holds for one path, and why
cargo run --quiet -- -C <repository> plan            # what a run would start
```

[The tool's README](../tools/nebensound/README.md) describes every call.
