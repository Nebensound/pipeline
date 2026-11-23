# The declaration

Every repository that calls the pipeline describes itself in one file,
`.nebensound/repo.toml`. This page describes every field of that file, what it
defaults to and what it does, and every finding phase 0 can report about it.

The names a field refers to (presets, checks, builds, targets, version writers,
ecosystems) are listed in [the register](register.md). What runs when, and under
which name, is described in [the phases](phases.md).

## The file

```toml
#:schema https://raw.githubusercontent.com/Nebensound/pipeline/main/tools/nebensound-repo/schemas/nebensound-declaration.json

[repo]
audience = "public"
pipeline = "none"

[component.tool]
path       = "tools/nebensound-repo"
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
- `[repo]` is required, and in it `audience`. Everything else is optional.
- The `#:schema` line gives an editor the
  [JSON Schema of the file](../tools/nebensound-repo/schemas/nebensound-declaration.json),
  for completion and for marking an unknown key where it is written. The
  `check-toml` building block checks the file against the same schema whether
  the line is there or not, and always against the schema of the version the
  caller pins, while the line points at `main` and can run ahead of that
  version (see [Calling the workflow](calling.md#the-schema-line-runs-ahead-of-the-pin)).
  Phase 0 is the judge: the schema lets through some
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

A lockfile is assigned by its name, never by the extension `.lock`, because
that extension is shared by formats that have nothing in common: `Cargo.lock`,
`uv.lock`, `poetry.lock` and `pdm.lock` belong to `toml`, `composer.lock`,
`flake.lock` and `Pipfile.lock` to `json`, and `yarn.lock` and `Gemfile.lock`
are in a format of their own that no check covers, so they stand on the list of
what nobody checks. A lockfile has to parse; it is not formatted, since the tool
that writes it decides its layout.

A marker file that keeps an otherwise empty folder, `.gitkeep` or `.keep`,
stands on the list of what nobody checks as long as it is empty: it needs no
exclusion. Empty means what Git holds for it, not what lies in the working
tree. A marker added with `git add -N` holds nothing in Git yet, so it does not
count as empty until it is staged. A marker that holds content is a file like
any other, and phase 0 reports it as `language-without-check`. The same holds
for `py.typed`, the empty marker of a Python package that ships its type hints;
`.python-version`, which names the interpreter uv installs, stands on the list
whatever it holds.

`Pipfile` is TOML without an extension and belongs to `toml` by its name. It is
no lockfile, so unlike `Pipfile.lock` it is formatted like any `.toml` file; the
README of the pipeline says how to format it by hand. Other files a Python
project commonly holds stay a gap on purpose: phase 0 reports each of them as
`language-without-check` until the repository excludes it by its path, with a
reason of its own.

- `requirements.txt`, the other `requirements*.txt`, the files in a
  `requirements/` folder and `constraints.txt`: a uv project keeps its
  dependencies in `pyproject.toml` and `uv.lock`. A repository that keeps such a
  list, for example one that `uv export` writes for an image, says why in its
  exclusion. As the manifest of the ecosystem `pip`, `requirements.txt` is also
  held to `unwatched-manifest`.
- `.coveragerc`, `pytest.ini`, `tox.ini` and `setup.cfg`: coverage.py and
  pytest read the same settings from `pyproject.toml`, and a second file can
  silently take precedence over it, since both tools take the first file they
  find and never merge. Phase 0 still reads coverage.py's `omit` in
  `.coveragerc`, `setup.cfg` and `tox.ini`, excluded or not.

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

| Field       | Type                    | Default  | Effect                                                                                          |
| ----------- | ----------------------- | -------- | ----------------------------------------------------------------------------------------------- |
| `edition`   | whole number            | `1`      | How the commit that holds the file is read. `1` is the only edition. See [Editions](#editions). |
| `repo`      | table                   | required | The repository as a unit. See `[repo]` below.                                                   |
| `component` | table of tables by name | none     | The components, each a unit of its own. See `[component.<name>]` below.                         |
| `include`   | array of tables         | none     | What is pulled back into the check after an exclusion. See `[[include]]` below.                 |
| `exclude`   | array of tables         | none     | What is taken out of the check, each entry with its reason. See `[[exclude]]` below.            |

### `[repo]`

| Field             | Type                    | Default             | Effect                                                                                                                                                   |
| ----------------- | ----------------------- | ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `audience`        | string                  | required            | For whom the repository is meant: `"public"`, `"internal"` or `"customer"`. See [Audience](#audience).                                                   |
| `pipeline`        | preset name or table    | nothing is declared | What runs for the repository beyond the base set. See [Pipeline](#pipeline).                                                                             |
| `versioning`      | string                  | `"self"`            | Which units around the repository carry a version. Only the values that include `self` are open to the repository. See [Versioning](#versioning).        |
| `minimum-version` | version `"X.Y.Z"`       | none                | The version the unit gets is never lower than this. See [Releases](#releases).                                                                           |
| `feat-types`      | array of commit types   | none                | Commit types that count like `feat` for every unit, beside `feat` itself. See [How a version is resolved](#how-a-version-is-resolved).                   |
| `allow-unused`    | table of check → reason | none                | Checks of this unit's own pipeline that may find no file without failing, each with the reason why it stays. Without an entry, such a check is an error. |
| `component`       | table                   | none                | The pipeline a component can ask for with `pipeline = { repo = true }`. See `[repo.component]` below.                                                    |

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

| Field             | Type                    | Default             | Effect                                                                                                                                                   |
| ----------------- | ----------------------- | ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `path`            | path                    | required            | Where the component lives: a folder, or a single file. It has to hold something Git tracks, otherwise `missing-path`.                                    |
| `pipeline`        | preset name or table    | nothing is declared | What runs for the component beyond the base set. See [Pipeline](#pipeline).                                                                              |
| `versioning`      | string                  | `"self"`            | Which units around the component carry a version. See [Versioning](#versioning).                                                                         |
| `minimum-version` | version `"X.Y.Z"`       | none                | The version the unit gets is never lower than this. See [Releases](#releases).                                                                           |
| `allow-unused`    | table of check → reason | none                | Checks of this unit's own pipeline that may find no file without failing, each with the reason why it stays. Without an entry, such a check is an error. |

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

| Field        | Type                             | Default | Effect                                                                                                                                                                                                                                                                                                                                      |
| ------------ | -------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `repo`       | `true`                           | absent  | Takes the pipeline `[repo.component]` offers. Only in the form `{ repo = true }`, alone.                                                                                                                                                                                                                                                    |
| `version`    | array of version writer names    | `[]`    | The version writers that receive the unit's version as a derived copy. Phase 0 demands that the unit, or the children it generates, carries a version. Only `cargo` is written so far.                                                                                                                                                      |
| `check`      | array of check names             | `[]`    | Checks beyond the base set. Each is in force for every file of the unit and runs in the Check phase.                                                                                                                                                                                                                                        |
| `build`      | build name                       | none    | The build that produces the unit's artifact. The plan names it for a unit that carries a version of its own and generates no children, and that unit gets a job in the Build phase of every run that gives it a version, the runs of a commit that touches it. Phase 0 demands that `version` selects every version writer the build needs. |
| `publish`    | `false` or array of target names | `false` | Where the artifact goes: `false` for nowhere, said out loud, or a list of targets. `publish = true` is an error, since it names no target. A target without a building block fails `nebensound / Check` by name, and no target has one yet.                                                                                                 |
| `dependabot` | array of ecosystem names         | `[]`    | The ecosystems Dependabot is to watch for this unit, beyond the base set. Phase 0 compares them with the manifests the unit holds. Nothing writes a Dependabot configuration from them yet.                                                                                                                                                 |

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

An exclusion takes the file out of phase 0 and out of its check alike: every
building block checks exactly the files `nebensound-repo files <check> <unit>`
names, and those are the unit's files less what `[[exclude]]` takes out.

Rust code is excluded by the package, never by the file. An entry that takes a
`.rs` file out of a package without taking out the whole package, the folder
that holds its `Cargo.toml`, is an error (see
[`partial-package-exclusion`](#partial-package-exclusion)). Inside a package,
Rust has its own means: `#[rustfmt::skip]`, `#[coverage(off)]` with a
justification, `#[allow(...)]`. A package excluded as a whole is meant for code
of others inside the repository, such as a vendored copy: `check-rust` leaves it
out of every step and compiles it only as a dependency of others (see
[the README](../README.md#check-rust)), and every run names it with its reason.
A package in a Git submodule cannot be excluded this way: phase 0 sees the files
Git tracks, where a submodule is a single entry, and the gate checks out no
submodules.

`[[exclude]]` is the only way out of a check: a tool's own ignore setting takes nothing out of a building block, and
phase 0 fails on every file such a setting takes out that no `[[exclude]]` entry
does (see [`unexcused-ignore`](#unexcused-ignore) and
[the README](../README.md#exclusions)).

## Audience

`audience` in `[repo]` says for whom the repository is meant. Rules come in
layers: some hold for every repository of the organization, some in addition
for its open-source repositories, some in addition for its internal ones, and
some in addition for internal projects a customer can see. The value says
which layers hold for the repository. GitHub knows whether a repository is
public, but not whether a customer sees it, so the declaration says it.

| Value        | Meaning                                                                                         |
| ------------ | ----------------------------------------------------------------------------------------------- |
| `"public"`   | Open source of the organization. The repository is public.                                      |
| `"internal"` | Private, for the organization alone.                                                            |
| `"customer"` | Private, and a customer can see it. For rules it includes everything that holds for `internal`. |

- The declaration of the commit at hand carries it; without it, phase 0 fails
  with [`missing-audience`](#missing-audience). There is no default, since none
  of the three is true of every repository.
- Declarations of earlier commits are read with it or without it, and it takes
  no part in resolving versions: a history resolves to the same versions
  whether its earlier commits carry the field or not.
- In a run, phase 0 holds the value against how GitHub shows the repository:
  `"public"` exactly when the repository is public. A difference is
  [`mismatched-audience`](#mismatched-audience). On a developer's machine
  nothing is compared: `nebensound-repo check` compares only when it is given
  `--visibility`, which the job `nebensound / Declare` passes.
- A pull request into `main` that changes the value names the old and the new
  value in the summary of the run, as a notice and in the step summary.
- The plan carries the value as `audience` (see
  [the tool's README](../tools/nebensound-repo/README.md#the-plan)), and
  `nebensound-repo explain` shows it for every path.
- No rule or check uses the value yet.

### Where a fact lives

So that no fact is kept twice, each lives in one of three places:

- **In the declaration**: what the repository consists of, what the pipeline
  does with it, and for whom it is meant. It is changed by pull request and
  read from the tree alone, so it is known on a developer's machine without
  the network as well.
- **As a repository property on GitHub**: only what GitHub has to decide
  without reading the tree, which is what an organization ruleset targets (see
  [the repository settings](repository-settings.md#repository-properties)). A
  property that repeats a fact of the declaration is a copy for
  administration; no step of the pipeline reads a property to decide anything.
- **Nowhere**: what the files or GitHub already say, such as the languages of
  a repository and whether it is a fork.

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
  pipeline, `allow-unused` and `minimum-version` of the unit that generated
  it, and carries a version of its own.
- A unit that generates children leaves the building to them, also when it
  carries a version beside theirs.

Whether a unit carries a version decides whether phase 0 resolves one for it
(see [Versions](#versions)), whether the plan names its build, and whether it
may publish and select version writers. No version is stored in the tree: the
history is the truth.

## Releases

A unit has one way to a full release: a merge into `main`. Every commit of the
line of `main` that touches the unit and that the history gives a version is a
full release of it (see [Versions](#versions)).

`minimum-version` is a floor: the version a unit gets is never lower. It is how
a person takes the step to `0.1.0` or `1.0.0`; no commit message leads to
`1.0.0`. It is three numbers, as `"1.0.0"`, without a leading zero, a prefix
such as `v` or a suffix. Anything else, a pre-release part such as `-pre.1`
included, stops phase 0, as described under
[When the file cannot be read](#when-the-file-cannot-be-read), and `check-toml`
marks it at its line and column. How the value takes effect is described under
[How a version is resolved](#how-a-version-is-resolved).

A repository that moves to the pipeline with releases made elsewhere sets
`minimum-version` above the highest version it ever gave out, pre-releases
included, so that no version is given out twice.

A generated unit takes the field from the unit that generated it. On a unit
that carries no version and generates no children, it has no version to act
on; phase 0 warns about it with
[`release-field-without-version`](#release-field-without-version).

The report of phase 0 and `nebensound-repo explain` show for every unit its
minimum version, with where it came from: written on the unit, or inherited
from the unit that generated it.

The field takes effect where the version of a commit is resolved, as described
under [Versions](#versions). The release stage on `main` writes the records of
those versions (see [Records](#records)).

## Versions

The history is the truth, and no version is stored in the tree. Phase 0
resolves for every unit that carries a version which version the commit at
hand has, from the commits of the repository and the declarations they hold,
and from nothing else. `nebensound-repo plan` lists them under `versions` (see
[the tool's README](../tools/nebensound-repo/README.md#the-plan)).
`nebensound-repo version` shows them as a table, with each unit's current
version, and the job `nebensound / Version` writes the same table into the
summary of every run (see [the phases](phases.md#version)).

Tags and releases are records: what the release stage writes down. No tag is
read to decide a version, and a tag is never a base. Phase 0 only holds the
records against the history (see [Records](#records)).

### Tag names

| Unit                            | Full release                 | Pre-release                        |
| ------------------------------- | ---------------------------- | ---------------------------------- |
| The repository                  | `vX.Y.Z`                     | `vX.Y.Z-pre.N`                     |
| A component                     | `<component>-vX.Y.Z`         | `<component>-vX.Y.Z-pre.N`         |
| A unit a component generates    | `<component>-<entry>-vX.Y.Z` | `<component>-<entry>-vX.Y.Z-pre.N` |
| A unit the repository generates | `<entry>-vX.Y.Z`             | `<entry>-vX.Y.Z-pre.N`             |

These are the names the release stage gives a unit's records. No version the
history gives is a pre-release yet; a tag of that form is still read as a
record and held against the history. Everything before `X.Y.Z` is the unit's
**prefix**: `v`, `<component>-v`, and so on. The repository has no name, so a
unit it generates is named like a component. A tag is read as a release
strictly: three numbers without a leading zero, and `N` a number from 1 without
a leading zero. A tag reads as the release of the
unit whose prefix it starts with when its version reads after it; a tag that
reads as no unit's release is passed over. A tag that reads
`<name>-vX.Y.Z` or `<name>-vX.Y.Z-pre.N` is the release of a unit named
`<name>` even when the declaration names no such unit any more: the old tags
of a removed component `viewer` are passed over, not taken for the
repository's because they start with `v`.
Because a generated unit's prefix carries its parent's name, it can meet another
unit's prefix (a component `docs-guide` beside the entry `guide` of a component
`docs`); phase 0 refuses that as `ungeneratable-name`, since the release stage
would write both units' records under the same names.

### How a version is resolved

- **The line of `main`** is the first-parent line of `HEAD`. A merge commit on
  it is a commit of the line like any other. The commits that only its second
  or a later parent reaches are not on the line and count for nothing,
  whatever their messages say.
- **Each commit's own declaration** says where a unit lived at that commit and
  which minimum version it had there. A unit is recognised across commits by
  its name. Its path at a commit is that of the declaration that commit holds,
  resolved as phase 0 resolves the one at hand: a generated unit's path comes
  from the tree the commit held. A unit that moves, or a generated unit whose
  folder or file is renamed, therefore keeps counting the commits from before.
- **Which commits count for a unit**: only those whose own declaration names
  the unit and gives it a version there. A unit's count therefore starts at
  the first commit of the line whose declaration names it, from `0.0.0`. A unit
  declared with `versioning = "none"` for a time starts its count when it gets
  a version; a unit that is removed and comes back goes on from where it was,
  and the commits in between do not count. The count never restarts, so no
  version is given twice. A commit without a declaration does not count. A
  commit whose declaration cannot be read takes a unit's path from the nearest
  commit after it on the line whose declaration can be read, not from the
  commit at hand, and counts for the units that declaration gives a version.
  What such a commit counts for is thereby fixed once that later commit
  exists: a later move of the unit does not count it again in a different
  way. Until a readable declaration follows it, the declaration at hand stands
  in for it.
- **Earlier declarations are read in their edition** (see
  [Editions](#editions)), with the tool that runs: its reader and its
  register. A declaration it cannot read falls under the rule above. A
  default of the register cannot change under the same name, since that
  would change what the declarations of old commits mean: such a change comes
  as a new name or a new edition (see [the register](register.md)).
- **The released commits** carry the releases, not tags: every commit of the
  line of `main` that touches the unit and that the history gives a version is
  a full release.

- **The base** of a unit at a commit is the version of its newest released
  commit before it that `HEAD` contains, as the history shows it; `0.0.0`
  before the first.

- **The current version** of a unit is the highest version the history gives
  any commit of the line up to `HEAD`. Every unit has one from the first commit
  that touches it on. It is what the version writer `cargo` requires of a path
  dependency whose unit the commit at hand does not touch, where a package
  that gets a version needs it, and it stands in the plan as `current`. On a
  pull request it carries `-pr.<number>` only when the pull request touches
  the unit: it is then the version of the pull request.

- **A commit touches a unit** when it changes a file below the unit's path as
  that commit declares it, or `.nebensound/repo.toml`. The repository is
  touched by every commit. Only the line of `main` counts. What a commit
  changes is read against its first parent, so a merge commit touches what
  the branch it merges brings into the line, exactly as a squash commit of the
  same changes would, and its message decides its change like any other
  commit's. A merge that changes nothing against its first parent touches
  only the repository. A moved or removed file touches the unit it left as
  well.
- **The change of a commit** follows from its message as a
  [Conventional Commit](https://www.conventionalcommits.org/en/v1.0.0/). A
  message that is not a Conventional Commit counts as a patch. What a change
  raises depends on the version it raises:

  | Version                  | Breaking change (`!` after the type, or a `BREAKING CHANGE:` footer) | `feat` | Everything else |
  | ------------------------ | -------------------------------------------------------------------- | ------ | --------------- |
  | below `0.1.0`            | patch                                                                | patch  | patch           |
  | `0.1.0` to below `1.0.0` | minor (`0.3.2` to `0.4.0`)                                           | patch  | patch           |
  | `1.0.0` and above        | major                                                                | minor  | patch           |

  No change leads to `1.0.0`; that step is taken with `minimum-version`.

  A repository whose changes are not features names its own types for the
  column `feat` with `feat-types` in `[repo]`. With `feat-types = ["rule"]`, a
  commit `rule: …` raises like `feat`, and `rule!: …` like a breaking change.
  A type is written as a Conventional Commit writes it, letters only, and is
  compared without regard to case; each type stands once, and `feat` and `fix`
  are not listed. Nothing else can be declared: the breaking marker is the same
  for every type, and a type that is not listed counts as everything else. Each
  commit is read with the list its own declaration holds, like every field
  above that decides a version: the list counts from the commit that writes it
  on, a commit before it counts without it, and a commit whose declaration
  cannot be read takes the list from the nearest commit after it whose
  declaration can, and from the declaration at hand until one follows. A
  declaration without the field reads every commit as before.

- **The version of a commit** that touches the unit is the base raised once
  per touching commit since the base, each by its own change.
- **`minimum-version`**: the value in force is the highest one that a touching
  commit since the base names, the commit at hand included. Lowering the value
  has no effect, and once a full release has reached it, it does nothing; there
  is no warning for either. It lifts the step at which it comes into force, so
  the commits after it go on from it rather than all landing on it.
- A commit that does not touch the unit has no version.

The version is a function of the history, not of the moment: every commit on
`main` raises on its own, so two runs on `main` at the same time come to
different numbers without waiting for each other. No tag is read, so a commit
comes to the same version whether any tag is set or not, and a repository
resolves the same versions with all of its tags deleted.

### Records

A tag reads as a release of a unit as described under [Tag names](#tag-names).
Phase 0 holds every such tag on a commit `HEAD` contains against the history:

- A tag on exactly the commit to which the history gives its version is a
  faithful record and passes.
- Any other such tag whose version is not lower than the unit's version at
  `HEAD`, the version of the commit at hand when it touches the unit and
  otherwise its current version, is the error
  [`conflicting-tag`](#conflicting-tag): a release of the unit could meet it.
- A tag below that stays unnoticed, so the tags of a repository from before it
  called the pipeline do no harm below its `minimum-version`.

A tag on a commit `HEAD` does not contain is passed over without a finding: it
lies in history this commit does not have, such as a newer release, and what
it records is not this commit's to judge.

The release stage on `main` writes the records: after a push to `main`, every
unit the pushed commit touches gets a lightweight tag on that commit, the
commit the history gives its version, and a GitHub Release of it, a full
release, since a version on `main` is never a pre-release. A unit the commit
does not touch gets none, so every record it writes is a faithful one. See
[the phases](phases.md#publish).

### What the history must hold

Phase 0 reads the whole history, and every tag to hold it against that
history. A shallow clone is the error `shallow-clone`. The job
`nebensound / Declare` checks the repository out with its whole history and
every tag.

Versions are resolved for `HEAD`, and every commit of the line, `HEAD`
included, is read with the declaration it holds. The units come from
`.nebensound/repo.toml` as it stands in the working tree. A change to the
declaration that is not committed yet therefore reaches the units but not the
history, and the versions mix the two: a move or a new `minimum-version` takes
effect only in part, and a unit that no commit declares yet has no version.
Commit a change to the declaration before reading its versions with `check`,
`plan`, `explain` or `ui`. In CI the checkout is the commit, so the two agree.

## Editions

`edition` at the top level of the declaration says how the commit that holds
it is read: with which reader, with which meaning of the register's names, and
with which rules of counting. It is a whole number and optional; left out, it
is `1`, and `1` is the only edition there is. This repository's declaration and
the examples leave it out while that holds.

The promise of an edition: within it, the versions of an existing history never
change with the version of the tool, and a declaration that was valid stays
readable with the same meaning. A change that would break this comes as a new
edition, which a repository switches on with a commit and which holds from that
commit on, while the tool goes on reading the older ones. The tool's tests hold
the promise: they resolve recorded histories to the versions recorded with them,
and a change that moves one of them fails.

The promise of edition 1 binds from `v0.1.0` of this repository on, the first
version meant for callers. Until then the rules of counting may still be
corrected within edition 1.

The promise covers the versions of the first-parent line of `main`. The
versions of pre-branches join it when they are built. A push to a pre-branch
resolves none before that (see
[the phases](phases.md#pull-request-and-push)), so none of them can change.

A tool that meets an edition it does not know stops phase 0 by name and says
that a newer pipeline is needed, wherever the declaration that names it stands
on the line of `main`, not only at `HEAD`. The edition is read before anything
else of the file, so a declaration written for a newer pipeline says so instead
of failing on a field this reader does not know. It is a failure to read the
file, not a finding (see [When the file cannot be read](#when-the-file-cannot-be-read)).

## Findings

Phase 0 reports what is wrong with a declaration as findings. Each carries a
stable code; the wording of its message may improve, the code does not change.

- An **error** fails `nebensound / Declare`, and with it `nebensound / Check`.
  No plan is made, so no check runs.
- A **warning** stops nothing. It says that a statement in the declaration is
  not true, not that something is broken, or that the repository departs from
  the organization's conventions (`nonconforming-name`,
  `nonconforming-default-branch`, `missing-minimum-file`). `check` reports
  these three; the other calls do not look for them.

A finding reads like `error[uncovered-file] at src/main.rs: …`: the severity, the
code, the unit or path it is about, and a sentence. A finding that points at a
place in a file adds its line and column, counted from 1:
`error[partial-package-exclusion] at .nebensound/repo.toml:12:8: …`. The report of phase 0 shows
each finding under the path it is about in the tree, and lists them all under
`Findings`. In a workflow run (`GITHUB_ACTIONS=true`, with no terminal
attached) the list holds each finding as an annotation of the run instead,
`::error` or `::warning` titled with its code, so that it shows on the run and
on the pull request. It names the file where the finding stands at a file the
repository holds, with line and column where it has them. Its message starts
with the code in brackets, since the log shows the message without the title:
`[missing-minimum-file] at LICENSE: …`. GitHub keeps only a limited number of
annotations per step, so a long list may show only in part on the run and the
pull request; the log of the step always holds every finding.

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

### `missing-audience`

Error. The declaration at the commit at hand does not say for whom the
repository is meant. Write `audience` in `[repo]`: `"public"` for open source
of the organization, `"internal"` for the organization alone, `"customer"`
where a customer can see it (see [Audience](#audience)). Declarations of
earlier commits are not held to it.

### `mismatched-audience`

Error. In a run, the declared audience does not fit how GitHub shows the
repository: `"public"` is declared exactly when the repository is public. The
finding names the value and the visibility. Correct the value, or the
repository's visibility. On a developer's machine nothing is compared, so the
finding comes only from a run.

### `missing-path`

Error. A component's `path` holds nothing Git tracks: the path is misspelt, the
folder is empty, or its files are not added. Correct the path, add the files, or
remove the component.

### `language-without-check`

Error. A file is not excluded, and no check in the register covers its kind.
Exclude it with a reason if nobody should look at it. If it is a kind of file
that belongs checked, the register needs a check for it, if need be one whose
building block fails until it is written. A `.gitkeep` or `.keep` that holds
content lands here too: empty it, or give it a name that says what it is.

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

### `release-field-without-version`

Warning. A unit writes `minimum-version`, carries no version of its own and
generates no children that would take the field over, so nothing ever reads the
value. The finding stands at the field in `.nebensound/repo.toml`, with the
line and column of its value. It is about the declaration at hand and changes no
version. Give the unit a `versioning` value that includes `self`, or remove the
field.

### `build-without-version-writer`

Error. A unit declares a build, and does not select a version writer that build
needs. What the build produces carries the version that writer puts in place
(`cargo-package` packages the version in the unit's `Cargo.toml`, which the
writer `cargo` sets), so without it the artifact would carry whatever version
the sources hold instead of the one the run gives the unit. The register names
the writers each build needs (see [Builds](register.md#builds)). Add the writer
to the unit's `version` list, or choose a preset that selects it. A unit that
generates children is reported once, for its own declaration, which the
children inherit. A unit that carries no version of its own and generates no
children is not reported: it gets no build job, so nothing it builds can carry
a wrong version.

### `build-without-file`

Error. A unit gets a build job for its build, and does not hold a file that
build reads below the unit's path. The build `docker` builds the image from
`deploy/Dockerfile` below the unit's path and from nothing else, so without it
the build job fails before it builds. The register names the files each build
needs (see [Builds](register.md#builds)). Add the file at the path the finding
names, or choose a build that fits the unit. Only the units the plan builds are
held to it: a unit that carries a version of its own and generates no children.
A generated unit needs the file below its own path; the unit that generated it
leaves the building to its children and needs none.

### `unwatched-manifest`

Warning. A file is the manifest of an ecosystem (`Cargo.lock` for `cargo`, for
example), and neither the base set nor a unit containing it has that ecosystem
watched. Add the ecosystem to the `dependabot` list of a unit that contains the
file.

### `pointless-watch`

Warning. A unit has an ecosystem watched and holds no manifest of it. Remove the
ecosystem from its `dependabot` list. An ecosystem of the base set is exempt.

### `missing-lockfile`

Warning. A manifest's ecosystem knows a lockfile, and Git does not track it.
What is checked and built is then resolved anew in each run instead of being
what was resolved and committed. The register names, per ecosystem, the
lockfile of a manifest and where it stands (see
[Ecosystems](register.md#ecosystems)): for a `Cargo.toml` the `Cargo.lock` in
the root of the Cargo workspace its package belongs to, found the way Cargo
finds it. The members of a workspace lead to the same lockfile, so it is
reported once, at the manifest nearest the root of the repository, and the
message names the lockfile. Commit it. A manifest that an `[[exclude]]` entry
takes out is passed over. Taking out only the reported manifest of a workspace
moves the finding to the next member; to silence it, take out every manifest
that leads to the lockfile, such as the folder of the workspace. Whether a
committed lockfile is up to date is for the check of each language.

### `unconventional-commit`

Warning. A commit the version of a unit is raised by has a message that is not a
Conventional Commit, `<type>[(<scope>)][!]: <description>`. It counts as a patch.
The finding names the commit once, however many units it touches. History is
not rewritten for it; the commits after it are what to write as Conventional
Commits.

### `shallow-clone`

Error. The repository is a shallow clone, so the history the versions are
counted from, and the tags held against it, may not be there. Check
the repository out with its whole history and its tags (`fetch-depth: 0` for
`actions/checkout`), or run `git fetch --unshallow --tags`.

### `exhausted-version`

Error. A unit's version cannot be raised: the field a change raises already
holds 18446744073709551615, the largest number a version can carry. There is
no version after it, and wrapping round to zero would move the version
backwards.

### `conflicting-tag`

Error. A tag reads as a release of a unit, `<prefix>X.Y.Z` or
`<prefix>X.Y.Z-pre.N`, sits on a commit `HEAD` contains, and the history gives
that version to another commit, or to none, while the version is not lower than
the unit's version at `HEAD`: the version of the commit at hand when it touches
the unit, otherwise its current version. A tag is a record and never moves a
version, and a release of the unit could meet this one. The finding names the
tag, its commit and the commit the history gives that version, if any. There
are two ways out: delete the tag if nobody published it, or raise
`minimum-version` so that the unit's versions pass it: past a tag
`X.Y.Z-pre.N` write `X.Y.Z` or higher, and past a tag `X.Y.Z` three numbers
above it. The finding names the value that applies. A tag on exactly the
commit to which the history gives its version is a faithful record, and one
below the unit's version stays unnoticed (see [Records](#records)).

### `unexcused-ignore`

Error. A tool's own setting takes a file out of what that tool reads in a
building block (a line of `.prettierignore`, for example), and no `[[exclude]]`
entry takes it out. The building block checks the file all the same, since it
reads no such setting; run by hand, the tool would skip it. Give the file an
`[[exclude]]` entry with a reason, or let the setting keep it in. A lockfile
the register names is never reported: no building block hands a lockfile to
Prettier, and Taplo lints without its configuration, so taking one out takes
nothing out of a check. A `.rs` file
that rustfmt's `ignore` takes out is excused only by an entry that takes out its
whole package (see
[`partial-package-exclusion`](#partial-package-exclusion)); inside a package,
`#[rustfmt::skip]` is the means. A Python file that holds `# ruff: noqa`
without rule codes takes itself out of Ruff's linter, which honours the line in
`check-python` as well: give the file an `[[exclude]]` entry, or switch single
rules off instead (`# ruff: noqa: <code>`). A file that the default list of
Ruff's or Pyright's `exclude` leaves out, such as one below `node_modules`, is
never reported. An `include` of Ruff or Pyright is reported where it leaves
out a file the tool reads without it, never for its default. Ruff's
`lint.exclude` and `format.exclude` take a file out of one step alone, which
the message names; what a Ruff configuration takes over through `extend`
counts as its own.
[The README](../README.md#exclusions) lists the settings phase 0 reads.

### `unreadable-ignore-file`

Error. A tool's setting that takes files out cannot be read: it does not parse,
a pattern or an entry is not what the tool takes, or a file it names through
`ignore-from-file` or `extends` cannot be read. Phase 0 cannot hold it against
the declaration, so it cannot tell whether the setting takes out more than
`[[exclude]]` does. Correct the setting. The message says what is wrong.

### `partial-package-exclusion`

Error. An `[[exclude]]` entry takes code out of a Rust package and not the whole
package: it names a `.rs` file or a folder inside the package, or it takes out
the package's folder while an `[[include]]` pulls a file of it back in. Cargo
compiles a package with all of its files, so a file with code cannot be left
out on its own. The finding stands at the entry's `path` in
`.nebensound/repo.toml`, with line and column. Remove the entry and use what
Rust has for the case: `#[rustfmt::skip]`, `#[coverage(off)]` with a
justification, `#[allow(...)]`. For code of others, exclude the package's whole
folder and pull nothing of it back in.

A package is a folder whose `Cargo.toml` Git tracks and has a `[package]` table;
a `Cargo.toml` that does not read counts as one. A `.rs` file belongs to the
innermost package around it, and a file that lies in no package is not judged
here.

### `nonconforming-name`

Warning. The repository's name is not lowercase letters and digits joined by
single hyphens (`^[a-z0-9]+(-[a-z0-9]+)*$`). `.github` is exempt, since GitHub
requires that name. The message names the name that would conform: lowercase,
`_` and `.` become `-`, hyphens that follow each other become one, and a
hyphen at either end goes. Words are not split at a change of case, so
`MyTool` becomes `mytool`. Nothing is renamed: renaming moves the repository's
address, and a person decides it.

`check` takes the name from `--name`; the workflow hands it in from the event
of the run. Without it, the name is the one the address of `origin` ends in.
Where Git knows no address, `check` says so in one line and checks no name.

### `nonconforming-default-branch`

Warning. The repository's default branch is not `main`, where the pipeline
gives every version out. `check` takes the branch from `--default-branch`; the
workflow hands it in from the event of the run. Without it, the branch is the
one `refs/remotes/origin/HEAD` names, which a clone sets
(`git remote set-head origin --auto` sets it later). Where Git does not know
it, `check` says so in one line and checks no branch.

### `missing-minimum-file`

Warning. The repository does not hold a file every repository holds, once, at
the root of the repository. The finding stands at the path where the file
belongs, and its message names that path. Every place and name GitHub itself
honours counts:

| File                          | Counts as                                                                                          | Named at                 |
| ----------------------------- | -------------------------------------------------------------------------------------------------- | ------------------------ |
| README                        | `README`, alone or with any extension, in any case, at the root, under `.github/` or under `docs/` | `README.md`              |
| licence or proprietary notice | a name that starts with `LICENSE`, `LICENCE`, `COPYING` or `UNLICENSE`, in any case, at the root   | `LICENSE`                |
| `AGENTS.md`                   | `AGENTS.md` at the root                                                                            | `AGENTS.md`              |
| code owners                   | `CODEOWNERS` at the root, under `docs/` or under `.github/`                                        | `.github/CODEOWNERS`     |
| Dependabot configuration      | `.github/dependabot.yml` or `.github/dependabot.yaml`                                              | `.github/dependabot.yml` |
| `.gitignore`                  | `.gitignore` at the root                                                                           | `.gitignore`             |
| `.editorconfig`               | `.editorconfig` at the root                                                                        | `.editorconfig`          |

A file counts when Git tracks it, whatever it holds. Add the file.

### When the file cannot be read

Some mistakes stop phase 0 before any finding is made. It then prints one
sentence that names the file and the mistake, and fails:

- the file is missing, or is not a regular file;
- the TOML does not parse;
- a key is unknown (`release` among them: a unit has one way to a full
  release), a required one is missing, or a value has the wrong type;
- a name, path or pattern breaks the rules above, or a reason is blank;
- `edition` is a whole number below 1, or names an edition this tool does not
  know, which says that a newer pipeline is needed. The edition is read before
  anything else of the file. An edition this tool does not know stops phase 0
  also where the declaration of an earlier commit of the line of `main` names
  it (see [Editions](#editions)); any other mistake there makes that
  declaration one that cannot be read (see
  [How a version is resolved](#how-a-version-is-resolved));
- `audience` is none of `"public"`, `"internal"` and `"customer"`;
- `minimum-version` is not `X.Y.Z`, a pre-release part such as `-pre.1`
  included;
- an entry of `feat-types` is not letters only, is `feat` or `fix`, or stands
  twice;
- an `[[exclude]]` entry names a pattern, or the same path stands twice under
  `[[exclude]]`;
- a pipeline combines `repo = true` with other keys, or says `repo = false`, or
  a table says `publish = true`.

## Trying it locally

The same tool that runs phase 0 in the workflow runs on a checkout. From
`tools/nebensound-repo/` in a checkout of this repository:

```sh
cargo run --quiet -- -C <repository> check                 # the whole report
cargo run --quiet -- -C <repository> explain <path>        # what holds for one path, and why
cargo run --quiet -- -C <repository> plan                  # what a run would start
cargo run --quiet -- -C <repository> files <check> <unit>  # what one building block checks
cargo run --quiet -- -C <repository> excluded-packages     # the Rust packages taken out as a whole
cargo run --quiet -- -C <repository> setup                 # find or install the programs its checks call
```

[The tool's README](../tools/nebensound-repo/README.md) describes every call.
