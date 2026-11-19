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

A lockfile is assigned by its name, never by the extension `.lock`, because
that extension is shared by formats that have nothing in common: `Cargo.lock`
and `uv.lock` belong to `toml`, `composer.lock` and `flake.lock` to `json`, and
`yarn.lock` and `Gemfile.lock` are in a format of their own that no check
covers, so they stand on the list of what nobody checks. A lockfile has to
parse; it is not formatted, since the tool that writes it decides its layout.

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

| Field             | Type                                 | Default                        | Effect                                                                                                                                                   |
| ----------------- | ------------------------------------ | ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pipeline`        | preset name or table                 | nothing is declared            | What runs for the repository beyond the base set. See [Pipeline](#pipeline).                                                                             |
| `versioning`      | string                               | `"self"`                       | Which units around the repository carry a version. Only the values that include `self` are open to the repository. See [Versioning](#versioning).        |
| `release`         | string                               | decided by the publish targets | `"single-stage"` or `"two-stage"`: how the unit's version reaches a full release. See [Releases](#releases).                                             |
| `minimum-version` | version `"X.Y.Z"` or `"X.Y.Z-pre.N"` | none                           | The version the unit gets is never lower than this. See [Releases](#releases).                                                                           |
| `allow-unused`    | table of check → reason              | none                           | Checks of this unit's own pipeline that may find no file without failing, each with the reason why it stays. Without an entry, such a check is an error. |
| `component`       | table                                | none                           | The pipeline a component can ask for with `pipeline = { repo = true }`. See `[repo.component]` below.                                                    |

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

| Field             | Type                                 | Default                        | Effect                                                                                                                                                   |
| ----------------- | ------------------------------------ | ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `path`            | path                                 | required                       | Where the component lives: a folder, or a single file. It has to hold something Git tracks, otherwise `missing-path`.                                    |
| `pipeline`        | preset name or table                 | nothing is declared            | What runs for the component beyond the base set. See [Pipeline](#pipeline).                                                                              |
| `versioning`      | string                               | `"self"`                       | Which units around the component carry a version. See [Versioning](#versioning).                                                                         |
| `release`         | string                               | decided by the publish targets | `"single-stage"` or `"two-stage"`: how the unit's version reaches a full release. See [Releases](#releases).                                             |
| `minimum-version` | version `"X.Y.Z"` or `"X.Y.Z-pre.N"` | none                           | The version the unit gets is never lower than this. See [Releases](#releases).                                                                           |
| `allow-unused`    | table of check → reason              | none                           | Checks of this unit's own pipeline that may find no file without failing, each with the reason why it stays. Without an entry, such a check is an error. |

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

An exclusion takes the file out of phase 0 and out of its check alike: every
building block checks exactly the files `nebensound files <check> <unit>` names,
and those are the unit's files less what `[[exclude]]` takes out.

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

Whether a unit carries a version decides whether phase 0 resolves one for it
(see [Versions](#versions)), whether the plan names its build, and whether it
may publish and select version writers. The report of phase 0 names the file
that is to hold the version: `.nebensound/version` for the repository,
`.nebensound/versions/<name>` for a component. Versions are not written or
tagged yet.

## Releases

`release` says how a unit's version reaches a full release:

| Value            | Meaning                                                                                                                                                                                                        |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `"single-stage"` | A merge to `main` is a full release.                                                                                                                                                                           |
| `"two-stage"`    | A merge to `main` is a pre-release `X.Y.Z-pre.N`. A pull request from `main` into the branch `release`, merged with a merge commit, makes the commit it promotes, the tip of `main`, the full release `X.Y.Z`. |

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
a person takes the step to `0.1.0` or `1.0.0`; no commit message leads to
`1.0.0`. It is written the way a tag carries a version: three numbers, as
`"1.0.0"`, without a leading zero or a prefix such as `v`, or a pre-release of
them, as `"1.0.0-pre.10"`, with `pre.` and a number from 1 without a leading
zero. Any other suffix, such as `-rc.1`, stops phase 0, as described under
[When the file cannot be read](#when-the-file-cannot-be-read), and `check-toml`
marks it at its line and column. A pre-release only means something for a unit
released in two stages; in one stage it is the error `pre-release-minimum`
until the unit's versions have reached its three numbers. How the value takes
effect is described under
[How a version is resolved](#how-a-version-is-resolved).

A repository that moves to the pipeline with releases made elsewhere sets
`minimum-version` above the highest version it ever gave out, pre-releases
included, so that no version is given out twice.

A generated unit takes both from the unit that generated it. The report of phase
0 and `nebensound explain` show for every unit how it is released and its
minimum version, each with where it came from: written on the unit, the default
of its publish targets, or inherited from the unit that generated it.

Both fields take effect where the version of a commit is resolved, as described
under [Versions](#versions). Nothing is tagged or released yet.

## Versions

Tags are the truth, and no version is stored in the tree. Phase 0 resolves for
every unit that carries a version which version the commit at hand has, from
Git alone, and `nebensound plan` lists them under `versions` (see
[the tool's README](../tools/nebensound/README.md#the-plan)).
`nebensound version` shows them as a table, with the number of commits waiting
for each unit's next release, and the job `nebensound / Version` writes the same
table into the summary of every run (see [the phases](phases.md#version)).

### Tag names

| Unit                            | Full release                 | Pre-release                        |
| ------------------------------- | ---------------------------- | ---------------------------------- |
| The repository                  | `vX.Y.Z`                     | `vX.Y.Z-pre.N`                     |
| A component                     | `<component>-vX.Y.Z`         | `<component>-vX.Y.Z-pre.N`         |
| A unit a component generates    | `<component>-<entry>-vX.Y.Z` | `<component>-<entry>-vX.Y.Z-pre.N` |
| A unit the repository generates | `<entry>-vX.Y.Z`             | `<entry>-vX.Y.Z-pre.N`             |

Everything before `X.Y.Z` is the unit's **prefix**: `v`, `<component>-v`, and so
on. The repository has no name, so a unit it generates is named like a
component. Tags are matched strictly: three numbers without a leading zero, and
`N` a number from 1 without a leading zero. A tag belongs to the unit whose
prefix it starts with and whose version reads after it; one that reads as no
unit's release belongs to the unit with the longest prefix it starts with. A tag
that reads `<name>-vX.Y.Z` or `<name>-vX.Y.Z-pre.N` is the release of a unit
named `<name>` even when the declaration names no such unit any more: the old
tags of a removed component `viewer` are passed over, not taken for the
repository's because they start with `v`.
Because a generated unit's prefix carries its parent's name, it can meet another
unit's prefix (a component `docs-guide` beside the entry `guide` of a component
`docs`); phase 0 refuses that as `ungeneratable-name`.

### How a version is resolved

- **The line of `main`** is the first-parent line of `HEAD`. A merge into
  `release` is the exception: the merge commit GitHub builds for the pull
  request from `main`, or the merge itself. Below its first parent lie the
  merges of `release`, so there the line is `HEAD` followed by the
  first-parent line of the commit of `main` it merges. Phase 0 tells such a
  merge by its parents: the first one lies on the first-parent line of
  `origin/release`, the second one on that of `origin/main`.
- **Each commit's own declaration** says where a unit lived and how it was
  released at that commit. A unit is recognised across commits by its name.
  Its path and its mode at a commit are those of the declaration that commit
  holds, resolved as phase 0 resolves the one at hand: a mode left out comes
  from the unit's publish targets, and a generated unit's path from the tree
  the commit held. Where a commit's declaration does not name the unit, or
  cannot be read, its path and its mode at the commit at hand are used. A unit
  that moves, or a generated unit whose folder or file is renamed, therefore
  keeps counting the commits from before, and a switch between one stage and
  two keeps the releases of each period.
- **Earlier declarations are read with the tool that runs**: its reader and
  its register. A declaration it cannot read, such as one written for an older
  reader with a field this one no longer knows, falls under the rule above. A
  default of the register that changes under the same name changes what the
  declarations of old commits mean, so it is a breaking change of the pipeline
  (see [the register](register.md)).
- **The base** is the unit's highest full release on a released commit that
  `HEAD` contains. Without such a tag the base is `0.0.0`. A person with write
  access can push any tag, so only tags on released commits count. Which
  commit is released depends on how the unit was released at the commit the
  tag is on:
  - in one stage there, a commit on the line of `main`;
  - in two stages there, a commit promoted into `release`: the second parent
    of a merge commit on the first-parent line of `origin/release`, when it is
    a commit of the line of `main`. Below the commit `release` was started on,
    the first-parent line of `origin/release` goes on through the history of
    `main`; a merge there brought a side branch into `main` and promoted
    nothing.

  A full release on a released commit counts whatever its number. Whether the
  pipeline set it is for the release check to tell, a step of its own. Any
  other full release with the unit's prefix on a commit `HEAD` contains is the
  warning `unexpected-tag` and does not count. A pre-release is given only to a
  commit of the line of `main` where the unit was released in two stages, so
  one on any other commit `HEAD` contains, or on one where the unit was
  released in one stage, is the warning `unexpected-tag` as well.

- **Only the tip of `main` is promoted.** A merge into `release` whose second
  parent is not the commit `origin/main` points at is the error
  `stale-promotion`, both on the pull request into `release` and on the merge
  itself, and resolves no version. The commits of `main` after an older commit
  were given pre-releases counted from the release before; promoting the older
  one would give their numbers out again. Promoting an older commit is not
  supported. The check runs only at the merge: Git keeps no record of where
  `origin/main` pointed when a merge was made, and the promoted commit, read on
  its own, is no merge, so phase 0 there reports nothing and resolves its full
  version. The release stage therefore runs phase 0 at the merge and resolves
  on the promoted commit only when that passed; a run on the promoted commit
  alone must not get past a refused merge. Read later, a refused merge that
  was pushed past the check is taken for a promotion like any other; it got no
  tag, so the base does not move with it.

- **A commit touches a unit** when it changes a file below the unit's path as
  that commit declares it, or `.nebensound/repo.toml`. The repository is
  touched by every commit. Only the line of `main` counts, and a merge commit
  touches nothing. A moved or removed file touches the unit it left as well.
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

- **One stage**: a commit that touches the unit is the base raised once per
  touching commit since the base, each by its own change.
- **Two stages**: the target is the base raised once by the highest change among
  the touching commits since the base. A touching commit is `<target>-pre.N`,
  `N` being its position among them, counted from 1. The commit a merge into
  `release` promotes is `<target>`; promoted with nothing since the base, it
  releases nothing.
- **`minimum-version`**: the value in force is the highest one that a touching
  commit since the base names, the commit at hand included. Lowering the value
  has no effect, and once a full release has reached it, it does nothing; there
  is no warning for either.
  - In one stage it lifts the step at which it comes into force, so the commits
    after it go on from it rather than all landing on it.
  - In two stages it lifts the target to its `X.Y.Z`. A pre-release value
    `X.Y.Z-pre.N` also sets the number when the target is `X.Y.Z`: the commit
    at which it comes into force is at least `-pre.N`, and every commit after
    it gets the number of the one before plus one. With `1.0.0-pre.1` to
    `1.0.0-pre.3` given out, a commit that sets `1.0.0-pre.10` is
    `1.0.0-pre.10`, and the next one `1.0.0-pre.11`.
  - In one stage, which gives out no pre-release, a pre-release value is
    ignored once the version the commits before the commit at hand come to has
    reached its `X.Y.Z`, whichever of their tags are set. Before that it is
    the error `pre-release-minimum`. A commit at which it was the error still
    counts for the commits after it, raised by its own change and not lifted,
    so writing `X.Y.Z` afterwards gives `X.Y.Z`.
- A commit that carries its unit's base is that release, also when it is read
  again.
- A commit that does not touch the unit has no version, except the promoted
  commit of a unit released in two stages.

The version is a function of the history, not of the moment: every commit on
`main` raises on its own, so two runs on `main` at the same time come to
different numbers without waiting for each other. In one stage a commit comes
to the same version whether the tags of the commits before it are set yet or
not.

In two stages the base moves only with the tag of a promoted commit. Between
the merge into `release` and that tag, a commit that lands on `main` would
still count from the release before. The release stages close this window:
they share one queue and resolve a version only when a run has its turn, after
the run before has set its tag. A release stage at a merge into `release`
resolves on the commit it promotes, once phase 0 has passed at the merge itself
(see the rule on promoting only the tip of `main` above).

### What the history must hold

Phase 0 reads the whole history and every tag. A shallow clone is the error
`shallow-clone`, and a unit released in two stages needs the branch
`origin/release` (`missing-release-branch`). Without the branch, a full release
from a period in which a unit was released in two stages is
`unexpected-tag`, since no commit of that period can be shown to be promoted.
A merge into `release` is told apart only with `origin/main` as well; without
it, the line of `main` is the first-parent line of `HEAD`. The job
`nebensound / Declare` checks the repository out with its whole history and
every branch and tag. A release tag on a commit `HEAD` does not contain is
passed over without a finding: it lies in history this commit does not have,
such as a newer release, and whether it counts is not this commit's to judge.

Versions are resolved for `HEAD`, and every commit of the line, `HEAD`
included, is read with the declaration it holds. The units come from
`.nebensound/repo.toml` as it stands in the working tree. A change to the
declaration that is not committed yet therefore reaches the units but not the
history, and the versions mix the two: a move, a change of the release mode or a
new `minimum-version` takes effect only in part, and a finding such as
`pre-release-minimum` stays where it is until the change is committed. Commit a change to the declaration before reading
its versions with `check`, `plan`, `explain` or `ui`. In CI the checkout is the
commit, so the two agree.

## Findings

Phase 0 reports what is wrong with a declaration as findings. Each carries a
stable code; the wording of its message may improve, the code does not change.

- An **error** fails `nebensound / Declare`, and with it `nebensound / Check`.
  No plan is made, so no check runs.
- A **warning** stops nothing. It says that a statement in the declaration is
  not true, not that something is broken.

A finding reads like `error[uncovered-file] at src/main.rs: …`: the severity, the
code, the unit or path it is about, and a sentence. A finding that points at a
place in a file adds its line and column, counted from 1:
`error[partial-package-exclusion] at .nebensound/repo.toml:12:8: …`. The report of phase 0 shows
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

### `unexpected-tag`

Warning. A tag carries a unit's prefix and is no release of that unit, so it
does not count: it does not read `<prefix>X.Y.Z` or `<prefix>X.Y.Z-pre.N`, it
points at something other than a commit, it is a full release on a commit
that was not released, or it is a pre-release off the line of `main`. Whether
a commit was released depends on how the unit was released at that commit, as
its declaration says. In one stage only a commit on the line of `main` is
released, so a full release on a merged side branch does not count. In two
stages only a promoted commit is released, so a full release on any other
commit of `main` does not count, even once a later promotion makes
`origin/release` contain it. A pre-release is given only to a commit of the
line of `main` where the unit was released in two stages, so one on a merged
side branch, or on a commit where the unit was released in one stage, does not
count either, and does not stop a switch to one stage
([`unreleased-before-switch`](#unreleased-before-switch)).
A person with write access can push any tag, so only tags on released commits
count. A full release on a released commit counts whatever its number; whether
the pipeline set it is for the release check to tell, a step of its own. Delete
the tag, or rename it so that it carries no unit's prefix.

### `unconventional-commit`

Warning. A commit the version of a unit is raised by has a message that is not a
Conventional Commit, `<type>[(<scope>)][!]: <description>`. It counts as a patch.
The finding names the commit once, however many units it touches. History is
not rewritten for it; the commits after it are what to write as Conventional
Commits.

### `shallow-clone`

Error. The repository is a shallow clone, so the history that counts the
commits since a release, and the tags of the releases, may not be there. Check
the repository out with its whole history and its tags (`fetch-depth: 0` for
`actions/checkout`), or run `git fetch --unshallow --tags`.

### `missing-release-branch`

Error. A unit is released in two stages, and there is no branch
`origin/release`. Its full releases are the commits promoted there, so neither
its base nor its next version can be told. Create the branch and fetch it, or
release the unit in one stage.

### `unreleased-before-switch`

Error. A unit is released in one stage, and its newest tag is a pre-release:
it was released in two stages, and its last pre-release never became a full
release. Only the tip of `main` is promoted
([`stale-promotion`](#stale-promotion)), and the tip is the switch, so revert
the switch, promote the tip of `main` into `release`, and switch again. The
versions then go on from a full release. A pre-release on a commit that the
commit carrying the base contains was released with the base, whatever its
number, and does not lead here: the promotion of the tip moves the base past
every pre-release before it. A pre-release on a commit where the unit was
released in one stage does not count ([`unexpected-tag`](#unexpected-tag)) and
does not lead here either.

### `exhausted-version`

Error. A unit's version cannot be raised: the field a change raises already
holds 18446744073709551615, the largest number a version can carry. There is
no version after it, and wrapping round to zero would move the version
backwards.

### `pointless-release-branch`

Warning. The branch `origin/release` exists, and no unit that carries a version
is released in two stages, at the commit at hand or at any commit of the line
of `main` before it, so nothing is or was ever promoted there. Delete the
branch, or release a unit in two stages. Once a unit was released in two
stages, the branch holds the commits that tell its full releases from then,
and it is kept.

### `pre-release-minimum`

Error. A unit is released in one stage, and the `minimum-version` in force for
it is a pre-release `X.Y.Z-pre.N` whose `X.Y.Z` the version of the commits
before the commit at hand has not reached. One stage gives out no pre-release,
so no commit could ever meet the floor. The finding names the line and column in
`.nebensound/repo.toml` where the value is written, or where the unit's table
starts when the value was taken away: taking it away is lowering it, which has
no effect. Write `X.Y.Z`, or release the unit in two stages.

### `stale-promotion`

Error. The commit at hand merges into `release` a commit other than the one
`origin/main` points at: an older commit of `main`, or a commit of another
branch. Only the tip of `main` is promoted. The commits of `main` after an
older one were already given pre-releases counted from the release before, and
promoting the older one would give their numbers out again. The merge does not
count as a promotion and resolves no version. The promoted commit, read on its
own, is no merge and reports nothing, so the release stage resolves on it only
once phase 0 has passed at the merge, and tags nothing here. Promote the tip of `main` with a pull request from `main` into
`release`. Phase 0 tells a merge into `release` by its first parent, which lies
on the first-parent line of `origin/release`; without `origin/main` it cannot
tell the tip, and reports nothing.

### `unexcused-ignore`

Error. A tool's own setting takes a file out of what that tool reads in a
building block (a line of `.prettierignore`, for example), and no `[[exclude]]`
entry takes it out. The building block checks the file all the same, since it
reads no such setting; run by hand, the tool would skip it. Give the file an
`[[exclude]]` entry with a reason, or let the setting keep it in. A `.rs` file
that rustfmt's `ignore` takes out is excused only by an entry that takes out its
whole package (see
[`partial-package-exclusion`](#partial-package-exclusion)); inside a package,
`#[rustfmt::skip]` is the means.
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

### When the file cannot be read

Some mistakes stop phase 0 before any finding is made. It then prints one
sentence that names the file and the mistake, and fails:

- the file is missing, or is not a regular file;
- the TOML does not parse;
- a key is unknown, a required one is missing, or a value has the wrong type;
- a name, path or pattern breaks the rules above, or a reason is blank;
- `release` is neither `"single-stage"` nor `"two-stage"`, or
  `minimum-version` is neither `X.Y.Z` nor `X.Y.Z-pre.N`;
- an `[[exclude]]` entry names a pattern, or the same path stands twice under
  `[[exclude]]`;
- a pipeline combines `repo = true` with other keys, or says `repo = false`, or
  a table says `publish = true`.

## Trying it locally

The same tool that runs phase 0 in the workflow runs on a checkout. From
`tools/nebensound/` in a checkout of this repository:

```sh
cargo run --quiet -- -C <repository> check                 # the whole report
cargo run --quiet -- -C <repository> explain <path>        # what holds for one path, and why
cargo run --quiet -- -C <repository> plan                  # what a run would start
cargo run --quiet -- -C <repository> files <check> <unit>  # what one building block checks
cargo run --quiet -- -C <repository> excluded-packages     # the Rust packages taken out as a whole
```

[The tool's README](../tools/nebensound/README.md) describes every call.
