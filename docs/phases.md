# The phases

A repository calls one workflow, `nebensound.yml`, from one caller file. The
workflow runs in phases, one after another; each phase is one or more jobs. This
page describes the phases that are built, the names their jobs report under,
and what runs on a pull request and on a push.

## What is built

| Phase   | Jobs                                                                                            |
| ------- | ----------------------------------------------------------------------------------------------- |
| Declare | `nebensound / Declare`                                                                          |
| Version | `nebensound / Version`                                                                          |
| Check   | `nebensound / Check repository`, `nebensound / Check component <name>` and `nebensound / Check` |
| Build   | `nebensound / Build repository`, `nebensound / Build component <name>` and `nebensound / Build` |
| Publish | `nebensound / Publish`, after a push to `main` only                                             |

Publish is the release stage on `main`: it writes the records of the versions
of a run, tags and releases, and no version is read from them. It publishes to
no target yet, the stage on `release` that makes the full release of a unit
released in two stages is not built, and nothing is deployed. Those phases
arrive one at a time, each when it does something: a job that only existed to
be green would report success for work nobody did. Until a phase is built, no
job stands in for it, and this page does not describe it.

## Declare

Phase 0. The job `nebensound / Declare` checks the calling repository out with
its whole history and every branch and tag, runs the tool `nebensound-repo` on
it and hands the result on:

1. The tool is built from the same commit of this repository as the workflow,
   and kept in the cache under a fingerprint of its sources, so two runs of the
   same tool share one build, and every unit job of the run takes it from there
   ([`.github/actions/tool`](../.github/actions/tool)).
2. `nebensound-repo check` reads `.nebensound/repo.toml`, resolves it against
   the register and checks it against what Git tracks, and resolves the version
   of every unit from the history, holding the tags against it. It also holds
   the repository's name and default branch, which the action hands in from the
   event of the run, and the files every repository holds against the
   organization's conventions, as warnings. It is given the visibility GitHub
   shows for the repository, taken from the event or, where the event does not
   carry it, from the API, and holds the declared audience against it (see
   [Audience](declaration.md#audience)). It prints the resolved configuration,
   the annotated tree, the findings, each as an annotation of the run, and a
   summary line.
   [The reference of the declaration](declaration.md) describes every field and
   every finding.
3. When phase 0 found no error, `nebensound-repo plan` prints the plan: every unit, with the checks
   started for it and the build it names, and the version of every unit that
   carries one, its current version and the version writers it selects. On a
   pull request the plan is built the way the job Version reads it (see below):
   each version is the one the merge would get, with `-pr.<number>` behind it.
   The units that start at least one check go on to the Check phase, the units
   whose build is not `null` and that the plan gives a version of this run go
   on to the Build phase, each with the version writers it selects, and the
   whole plan is handed on with them: a later job checks out no history, so a
   version writer takes the versions from the plan. The plan also carries for
   whom the repository is meant.
4. On a pull request into `main` that changes the audience, the old and the new
   value are named in the summary of the run, as a notice and in the step
   summary. A promotion into `release` names no change: it was named on the
   pull request into `main`.
5. Then every Rust package `[[exclude]]` takes out as a
   whole (`nebensound-repo excluded-packages`) is named in the summary of the
   run with its reason, as a notice.

An error ends the phase here: there is no plan, no check runs, nothing is
built, and `nebensound / Check` and `nebensound / Build` fail. A warning stops
nothing.

## Version

The job `nebensound / Version` reports which version every unit carries. It
writes nothing and tags nothing. It checks the repository out the way Declare
does, with its whole history and every branch and tag, takes the tool from the
cache Declare filled and runs `nebensound-repo version`: the text table goes
into the log, and the same table as Markdown into the summary of the run, one
row per unit that carries a version:

| Column      | What it says                                                                                                                                     |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Unit, Path  | Which unit, and where it lives.                                                                                                                  |
| Release     | `single-stage` or `two-stage`.                                                                                                                   |
| Current     | The highest version the history gives a commit up to the commit at hand, a pre-release included; `none` before the first commit that touches it. |
| This commit | The version of the commit at hand, `none` when the commit does not touch the unit.                                                               |
| Waiting     | In two stages only: the commits since the last full release that touch the unit, the commit at hand included. Empty in one stage.                |

[The reference of the declaration](declaration.md#versions) describes how each
value is resolved. The repository always carries a version, so the table always
has a row.

On a pull request the checkout is the merge commit GitHub builds for it, and a
merge commit touches no unit. The tool therefore reads it as the commit the
squash merge would put on the base branch: one parent, the changes of the pull
request, and its title and description as the message, as they stand when the
run starts. The column is called `This pull request` there, and each version in
it is the one that merge would get with `-pr.<number>` behind it, such as
`0.4.2-pr.17` or `1.2.0-pre.3-pr.17`; so is the current version of a unit the
pull request touches. It names artifacts and is never published. A pull request
into `release` is merged with a merge commit and is read as it is.

The job runs whatever Declare's result. When phase 0 finds an error, such as a
shallow clone, there is no table: the log and the summary list every finding
and the job fails. It waits for Declare only to take the tool from the cache.
A cancelled run skips it.

## Check

### One job per unit

Every unit that starts a check gets a job of its own, named
`nebensound / Check repository` for the repository and
`nebensound / Check component <name>` for a component. A check runs once, at the
outermost unit it is in force for, so a unit whose checks all run further out
gets no job. The job first takes the tool from the cache Declare filled, and
builds it only when the cache misses. Then its steps:

1. **Not written yet.** It fails, by name, for every check of the unit that has
   no building block yet, so that a pull request says what is missing instead of
   passing without anyone having looked. The checks that are written still run.
   The building blocks written today are listed in
   [the README](../README.md#building-blocks).
2. **One step per check** of the unit, each the building block
   `check-<name>` over exactly the files `nebensound-repo files <check> <unit>`
   names: the unit's files that the check covers, less what `[[exclude]]` takes
   out. Every step runs even when one before it failed, so one run shows every
   finding of the unit.
3. **`xtask check --all`**, in the job of the repository only, and only when
   `xtask/Cargo.toml` exists. It runs `cargo xtask check --all` from the
   repository root with the toolchain the xtask pins (the `rust-toolchain.toml`
   in `xtask/`, else the one in the repository root); an xtask that pins none
   fails the step. This is where a repository's own checks run, as mandatory as
   the register's, without a workflow or a ruleset entry of their own. The
   xtask's code is Rust like any other, so phase 0 demands a unit that runs the
   `rust` check over `xtask/`.

### Where a failed check is found

The job `nebensound / Check` only gathers: it reports that something failed and
holds no finding. The finding is in the job of the unit that failed, in the step
of the check, where it is written to the log and, where the check can point at
a file, as an annotation on it, shown on the pull request's Files tab and in
the summary of the run.
A failure of phase 0 is in the job `nebensound / Declare`, which lists every
finding.

### The summary

`nebensound / Check` runs after Declare and every unit job, whatever their
result. It fails when phase 0 did not pass or when any unit job did not pass,
and it passes when every check the plan started passed, or when the plan started
none.

Its second step, **Version writers not written yet**, fails by name for every
version writer a unit of the plan selects that has no building block
`version-<writer>` yet, the way a check without one fails in its unit job. A
writer belongs to a unit that carries a version, which need not run a check,
so it is named here. The writers written today are listed in
[the README](../README.md#version-cargo).

Its third step, **Publish targets not written yet**, does the same for every
publish target a unit of the plan names that has no building block
`publish-<target>` yet, the dots of the target turned into hyphens
(`crates.io` is `publish-crates-io`), whether the commit touches the unit or
not. It fails on the pull request, before anything is merged, rather than in
Publish afterwards. No publish target has a building block yet, so a unit that
publishes anywhere fails `nebensound / Check` today, and Publish only ever
releases units with `publish = false`.

## Build

### One job per unit that builds in this run

Every unit whose build the plan names and that the plan gives a version of
this run gets a job of its own, named `nebensound / Build repository` for the
repository and `nebensound / Build component <name>` for a component. The plan
names the build of a unit that carries a version of its own and generates no
children (see [the declaration](declaration.md#versions)), and gives a unit a
version of the run when the commit touches it. A unit the commit does not touch
has no new artifact to hand on and gets no build job, also where a version
writer writes its current version for a package that depends on it. A
build job starts only when Declare and `nebensound / Check` passed: nothing is
built from a tree whose checks failed. It checks the repository out, takes the
tool from the cache Declare filled, and runs its steps:

1. **Not written yet.** It fails, by name, when the unit's build has no
   building block yet, so that a pull request says what is missing. The builds
   written today are listed in [the README](../README.md#build-cargo-package).
2. **One step per version writer** the unit selects, each the building block
   `version-<writer>` with the plan, so that the manifest carries the version
   of the run. A version writer without a building block has already failed
   `nebensound / Check`, so no build starts without its version.
3. **The build**, the building block `build-<name>` over the unit's path, with
   the plan where the build labels what it builds with the unit's version, as
   `build-docker` does. It builds the artifact once, tests the artifact itself
   and uploads it as an artifact of the run.

A step does not run after one that failed: an artifact built from versions that
were not written would be the wrong one. Tests of the source belong to Check;
the build tests what it hands on.

### The summary of the builds

`nebensound / Build` runs after Declare, `nebensound / Check` and every build
job, whatever their result, so it reports on every run. It fails when phase 0
did not pass, since then nothing is known about the builds. When nothing builds
in this run it passes with a sentence that says so, whatever Check found.
Otherwise it fails when Check did not pass, since then nothing was built, and
when any build job did not pass, and it passes when every build the plan
started passed.

## Publish

The job `nebensound / Publish` is the release stage on `main`. It runs after a
push to `main`, and on no other event or branch, once Declare,
`nebensound / Check` and `nebensound / Build` passed. Check is named on its
own, since Build is green when nothing builds, whatever Check found: nothing
is released from a tree whose checks failed. It is the only job that asks for
`contents: write`.

Every unit the plan gives a version of this run, that is, every unit the
pushed commit touches, gets its records: a tag on the pushed commit, named as
[the declaration](declaration.md#tag-names) names a unit's records, and a
GitHub Release of it.

| Unit                       | Version on `main` | The release                                    |
| -------------------------- | ----------------- | ---------------------------------------------- |
| The repository, one stage  | `X.Y.Z`           | A full release, and the latest.                |
| A component, one stage     | `X.Y.Z`           | A full release, not the latest.                |
| Any unit, two stages       | `X.Y.Z-pre.N`     | A pre-release, never the latest.               |
| Any unit, two stages, full | `X.Y.Z`           | None here: its full release is the next stage. |

A unit the commit does not touch gets nothing, also where a version writer
writes its current version for a package that depends on it: a tag on the
pushed commit with that version would not stand on the commit the history
gives it, and phase 0 would report it as `conflicting-tag` in the next run.

The job checks the repository out with its whole history and every tag, and
its steps are:

1. **Decide what to release.** Nothing is written yet. When the tag of a unit
   already stands on another commit, the job fails here and names it, before
   anything is written: a version belongs to exactly one commit. When a newer
   record of the unit already holds the pushed commit, as after a re-run of an
   older run, the unit is left out with a sentence that says so, and the job
   stays green. For each unit to release it finds the previous record: the
   tag of the unit with the highest version below the one released, on a
   commit the pushed commit contains.
2. **Create the tags and the drafts.** For each unit, the tag on the pushed
   commit, a lightweight one, and the release as a draft, titled
   `<unit> <version>` (the repository by its name, such as `pipeline 0.4.2`),
   with notes GitHub generates against the previous record. A unit's first
   release carries one fixed sentence instead.
3. **The publish targets**, while the releases are drafts: an immutable release
   takes no file once it is published. No target has a building block yet, so
   this step does not exist yet.
4. **Publish the releases.** Each draft is published, as a pre-release or not,
   and marked as the latest release or not, in the same call: GitHub honours
   that mark only there. When the published release shows that it is not
   immutable, because the repository has immutable releases switched off, the
   job writes a warning and a line in the summary of the run, and goes on.

A re-run completes what an earlier attempt left undone and changes nothing
else: a tag that stands on the pushed commit is kept, a draft the attempt left
is taken up, and a release published already is left as it is. Every step
writes what it did into the log, and the summary of the run lists every unit
with its release, or the sentence why there is none.

Tags and releases are records. Publish reads them for what records answer, the
notes since the previous record and whether a newer one holds the commit, and
never for a version: the versions are the plan's, resolved from the history.

A release published with the workflow's token starts no workflow, so a caller
that listens to tags or releases is not started by Publish.

## Check names

Every job reports under a name in two parts, `nebensound / Check`: GitHub names
a job of a called workflow after the job that calls it and then the job in the
called workflow.

- **The first part** is the job in the caller file. It is `nebensound` in every
  repository, which is why the caller file is the same everywhere and gives the
  job no `name` of its own.
- **The second part** is the job in the central workflow. The pipeline fixes it.

A ruleset requires a name in both parts. Of the names a run reports today, it
requires `nebensound / Version`, `nebensound / Check` and `nebensound / Build`,
and only those: the unit jobs change with the repository, so a ruleset that
required one would block a pull request when the unit goes away or is renamed,
and Declare is part of what `Check` and `Build` sum up. Of the reported jobs,
only `Check` is meant for a ruleset to require for the checks, and only `Build`
for the builds. `nebensound / Publish` runs after the merge, on `main`; on a
pull request it is skipped, and a ruleset counts a skipped check as passed, so
requiring it would add no signal. Further required names arrive with the phases
that are not built yet.

## Pull request and push

The caller file starts the workflow on every pull request and on every push to
`main` and to `release`:

```yaml
on:
  pull_request:
  push:
    branches:
      - main
      - release
```

Both run the same jobs, Declare, Version, Check and Build, with the same steps,
and a push to `main` runs Publish after them. The versions depend on the event:
on a pull request `Version` shows the version the merge would get with
`-pr.<number>` behind it, and the plan carries the same versions, which the
package test of `check-rust` and the build jobs write and build with.

A newer push to a pull request cancels its run that is still going: only the
newest commit of a pull request counts. A run after a push, on `main` or on
`release`, is never cancelled. It waits in the one queue of the repository
until the run before it has finished, so the runs after two merges publish in
the order of their pushes and with two different versions, and a run that
waits is never replaced by a newer one. The workflow sets this itself, in its
`concurrency`: a pull request's group cancels and keeps one run waiting, the
repository's group keeps up to 100 runs waiting and cancels none, and a run
beyond that is cancelled. GitHub gives the waiting runs their turn in the order
in which they began to wait, and since a run may begin to wait a moment after
it was started, it does not guarantee that order. Should two runs swap places,
the versions stay what the history gives each commit; the older run then finds
its commit held by the newer release and releases nothing (see
[Publish](#publish)).

The caller grants `contents: write`, and only Publish asks for it: every other
job reads the repository and writes nothing back, and so does every job of a
pull request.

This repository calls the workflow the same way, with one difference: its own
caller runs the workflow at the commit under test rather than at a pinned
version, so a change to the pipeline is checked by the pipeline it changes.
