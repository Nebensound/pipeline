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

The workflow does not publish or deploy anything yet, and it reports versions
without tagging or releasing them. Those phases arrive one at a time,
each when it does something: a job that only existed to be green would report
success for work nobody did. Until a phase is built, no job stands in for it,
and this page does not describe it.

## Declare

Phase 0. The job `nebensound / Declare` checks the calling repository out with
its whole history and every branch and tag, runs the tool `nebensound` on it and
hands the result on:

1. The tool is built from the same commit of this repository as the workflow,
   and kept in the cache under a fingerprint of its sources, so two runs of the
   same tool share one build, and every unit job of the run takes it from there
   ([`.github/actions/tool`](../.github/actions/tool)).
2. `nebensound check` reads `.nebensound/repo.toml`, resolves it against the
   register and checks it against what Git tracks, and resolves the version of
   every unit from the tags and the history. It prints the resolved
   configuration, the annotated tree, the findings and a summary line.
   [The reference of the declaration](declaration.md) describes every field and
   every finding.
3. When phase 0 found no error, every Rust package `[[exclude]]` takes out as a
   whole (`nebensound excluded-packages`) is named in the summary of the run
   with its reason, as a notice.
4. Then `nebensound plan` prints the plan: every unit, with the checks started
   for it and the build it names, and the version of every unit that carries
   one, its last release, its pending version and the version writers it
   selects. On a pull request the plan is built the way the job Version reads it
   (see below): each version is the one the merge would get, with `-pr.<number>`
   behind it. The units that start at least one check go on to the Check phase,
   the units whose build is not `null` go on to the Build phase, each with the
   version writers it selects, and the whole plan is handed on with them: a
   later job checks out no history, so a version writer takes the versions from
   the plan.

An error ends the phase here: there is no plan, no check runs, nothing is
built, and `nebensound / Check` and `nebensound / Build` fail. A warning stops
nothing.

## Version

The job `nebensound / Version` reports which version every unit carries. It
writes nothing and tags nothing. It checks the repository out the way Declare
does, with its whole history and every branch and tag, takes the tool from the
cache Declare filled and runs `nebensound version`: the text table goes into the
log, and the same table as Markdown into the summary of the run, one row per
unit that carries a version:

| Column       | What it says                                                                         |
| ------------ | ------------------------------------------------------------------------------------ |
| Unit, Path   | Which unit, and where it lives.                                                      |
| Release      | `single-stage` or `two-stage`.                                                       |
| Last release | Its highest full release, `none` without one.                                        |
| This commit  | The version of the commit at hand, `none` when the commit does not touch the unit.   |
| Waiting      | The commits since the last release that touch the unit, the commit at hand included. |

[The reference of the declaration](declaration.md#versions) describes how each
value is resolved. The repository always carries a version, so the table always
has a row.

On a pull request the checkout is the merge commit GitHub builds for it, and a
merge commit touches no unit. The tool therefore reads it as the commit the
squash merge would put on the base branch: one parent, the changes of the pull
request, and its title and description as the message, as they stand when the
run starts. The column is called `This pull request` there, and each version in
it is the one that merge would get with `-pr.<number>` behind it, such as
`0.4.2-pr.17` or `1.2.0-pre.3-pr.17`. It names artifacts and is never
published. A pull request into `release` is merged with a merge commit and is
read as it is.

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
   `check-<name>` over exactly the files `nebensound files <check> <unit>`
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

## Build

### One job per unit that builds

Every unit whose build the plan names gets a job of its own, named
`nebensound / Build repository` for the repository and
`nebensound / Build component <name>` for a component. The plan names the build
of a unit that carries a version of its own and generates no children (see
[the declaration](declaration.md#versions)). A build job starts only when
Declare and `nebensound / Check` passed: nothing is built from a tree whose
checks failed. It checks the repository out, takes the tool from the cache
Declare filled, and runs its steps:

1. **Not written yet.** It fails, by name, when the unit's build has no
   building block yet, so that a pull request says what is missing. The builds
   written today are listed in [the README](../README.md#build-cargo-package).
2. **One step per version writer** the unit selects, each the building block
   `version-<writer>` with the plan, so that the manifest carries the version
   of the run. A version writer without a building block has already failed
   `nebensound / Check`, so no build starts without its version.
3. **The build**, the building block `build-<name>` over the unit's path. It
   builds the artifact once, tests the artifact itself and uploads it as an
   artifact of the run.

A step does not run after one that failed: an artifact built from versions that
were not written would be the wrong one. Tests of the source belong to Check;
the build tests what it hands on.

### The summary

`nebensound / Build` runs after Declare, `nebensound / Check` and every build
job, whatever their result, so it reports on every run. It fails when phase 0
or Check did not pass, since then nothing was built, and when any build job did
not pass. It passes when every build the plan started passed, or, with a
sentence that says so, when the plan starts none.

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
for the builds. Further required names arrive with the phases that are not
built yet.

## Pull request and push

The caller file starts the workflow on every pull request and on every push to
`main`:

```yaml
on:
  pull_request:
  push:
    branches:
      - main
```

Today both run the same jobs, Declare, Version, Check and Build, with the same
steps. Only the versions depend on the event: on a pull request `Version` shows
the version the merge would get with `-pr.<number>` behind it, and the plan
carries the same versions, which the package test of `check-rust` and the build
jobs write and build with. A newer run for the same branch or pull request cancels the one still
running. The permissions are read-only: the workflow reads the repository and
writes nothing back.

This repository calls the workflow the same way, with one difference: its own
caller runs the workflow at the commit under test rather than at a pinned
version, so a change to the pipeline is checked by the pipeline it changes.
