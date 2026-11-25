# The phases

A repository calls one workflow, `nebensound.yml`, from one caller file. The
workflow runs in phases, one after another; each phase is one or more jobs. This
page describes the phases that are built, the names their jobs report under,
and what runs on a pull request and on a push.

## What is built

| Phase   | Jobs                                                                                                                                                        |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Declare | `nebensound / Declare`                                                                                                                                      |
| Version | `nebensound / Version`                                                                                                                                      |
| Check   | `nebensound / Check repository`, `nebensound / Check component <name>` and `nebensound / Check`                                                             |
| Build   | `nebensound / Build repository`, `nebensound / Build component <name>`, each with `(<target>)` behind it for a build with targets, and `nebensound / Build` |
| Publish | `nebensound / Publish`, after a push to `main` only                                                                                                         |

Publish is the release stage on `main`: it writes the records of the versions
of a run, tags and releases, and no version is read from them. It publishes to
no target yet, and nothing is deployed. Those phases arrive one at a time, each
when it does something: a job that only existed to be green would report
success for work nobody did. Until a phase is built, no job stands in for it,
and this page does not describe it.

## Declare

Phase 0. The job `nebensound / Declare` checks the calling repository out with
its whole history and every branch and tag, runs the tool `nebensound-repo` on
it and hands the result on. Phase 0 is one call, `nebensound-repo declare`,
which does steps 2 to 6 below:

1. The tool comes from the same commit of this repository as the workflow
   ([`.github/actions/tool`](../.github/actions/tool)), and every job of the
   run gets it the same way. Where that commit carries a release of this
   repository, the job fetches the release's binary for its system
   (`nebensound-repo-<target>.tar.gz`), verifies it with
   `gh release verify-asset` against the release's attestation and runs it; a
   binary that does not verify fails the job, and nothing of it is cached.
   Where there is none, such as on a pull request or a push of this
   repository, on a pin between two releases or on a system the release has no
   binary for, or where `gh` is missing or older than 2.93.0 or the API has too
   few requests left to verify it, a warning names why and the tool is built
   from source, as before: kept in the cache under a fingerprint of its sources
   and the runner's image, so two jobs of the same tool and image share one
   build. Publish alone never takes it from the cache (see
   [Publish](#publish)).
2. Phase 0, as `nebensound-repo check` runs it, reads `.nebensound/repo.toml`,
   resolves it against the register and checks it against what Git tracks, and
   resolves the version of every unit from the history, holding the tags against
   it. It also holds the repository's name and default branch, which the action
   hands in from the event of the run, and the files every repository holds
   against the organization's conventions, as warnings. It is given the
   visibility GitHub shows for the repository, taken from the event or, where
   the event does not carry it, from the API, and holds the declared audience
   against it (see [Audience](declaration.md#audience)). It prints the resolved
   configuration, the annotated tree, the findings, each as an annotation of the
   run, and a summary line. [The reference of the declaration](declaration.md)
   describes every field and every finding.
3. When phase 0 found no error, the plan is printed, as `nebensound-repo plan`
   prints it: every unit, with the checks started for it and the build it names,
   and the version of every unit that carries one, its current version and the
   version writers it selects. On a pull request the plan is built the way the
   job Version reads it (see below): each version is the one the merge would
   get, with `-pr.<number>` behind it. The units that start at least one check
   go on to the Check phase, the units whose build is not `null` and that the
   plan gives a version of this run go on to the Build phase, each with the
   version writers it selects, and the whole plan is handed on with them: a
   later job checks out no history, so a version writer takes the versions from
   the plan. The plan also carries for whom the repository is meant.
4. On a pull request into `main` that changes the audience, the old and the new
   value are named in the summary of the run, as a notice and in the step
   summary.
5. Then every Rust package `[[exclude]]` takes out as a whole
   (`nebensound-repo excluded-packages`) is named in the summary of the run with
   its reason, as a notice.
6. Last the two matrices are handed on beside the plan: the units that start
   at least one check, and the builds of this run, each with the version
   writers its unit selects, the name of its job and its runner. A build whose
   register entry lists targets stands once per target, on the runner the
   register names for it. After a push to a pre-branch no unit builds.

An error ends the phase here: there is no plan, no check runs, nothing is
built, and `nebensound / Check` and `nebensound / Build` fail. A warning stops
nothing.

## Version

The job `nebensound / Version` reports which version every unit carries. It
writes nothing and tags nothing. It checks the repository out the way Declare
does, with its whole history and every branch and tag, gets the tool as
Declare does and runs `nebensound-repo report-versions`: the text
table of `nebensound-repo version` goes into the log, and the same table as
Markdown into the summary of the run, one row per unit that carries a version:

| Column      | What it says                                                                                                             |
| ----------- | ------------------------------------------------------------------------------------------------------------------------ |
| Unit, Path  | Which unit, and where it lives.                                                                                          |
| Current     | The highest version the history gives a commit up to the commit at hand; `none` before the first commit that touches it. |
| This commit | The version of the commit at hand, `none` when the commit does not touch the unit.                                       |

[The reference of the declaration](declaration.md#versions) describes how each
value is resolved. The repository always carries a version, so the table always
has a row.

On a pull request the checkout is the merge commit GitHub builds for it, which
carries a message of GitHub's and contains the commits of the branch. The tool
therefore reads it as the commit the squash merge would put on the base
branch: one parent, the changes of the pull request, and its title and
description as the message, as they stand when the run starts. A merge commit
that lands the pull request with the same title and description comes to the
same versions. The column is called `This pull request` there, and each
version in it is the one that merge would get with `-pr.<number>` behind it,
such as `0.4.2-pr.17`; so is the current version of a unit the pull request
touches. It names artifacts and is never published.

On a pull request the job also holds what a release would need against the
records. A unit the pull request gives a version that publishes to a registry,
a target with `Dependencies by version` in
[the register](register.md#publish-targets), is followed through what its
package requires by path, as the version writer `cargo` follows it. Every unit
reached that the pull request does not touch must have a tag of its current
version; otherwise the job fails with
[`unreleased-requirement`](declaration.md#unreleased-requirement), which names
the way out. It runs here because this job reads the history and the tags of
the pull request as its merge would put them on `main`, and it fails the pull
request without holding back its checks and builds. After a push to `main`
nothing is held to it: the merge has happened, and failing then would only
leave `main` red. The publish blocks of the registries are not written yet, so
such a target also fails by name in `Check` until they are.

The job runs whatever Declare's result. When phase 0 finds an error, such as a
shallow clone, there is no table: the log and the summary list every finding
and the job fails. It waits for Declare only so that a tool built from source
comes from the cache Declare filled.
A cancelled run skips it.

After a push to a pre-branch, `pre` or `pre-*`, the job resolves no version and
shows no table. The tool knows no branch and reads the first-parent line of
`HEAD` as the line of `main`, so it would give each own commit of the branch a
full version that `main` never gives. Versions and pre-releases of pre-branches
are not built yet, so the job says that instead, as a notice and as a line in
the summary of the run: no version is resolved, nothing is built and nothing is
published.

## Check

### One job per unit

Every unit that starts a check gets a job of its own, named
`nebensound / Check repository` for the repository and
`nebensound / Check component <name>` for a component. A check runs once, at the
outermost unit it is in force for, so a unit whose checks all run further out
gets no job. The job first gets the tool as Declare does; built from source,
it comes from the cache Declare filled, and is built only when the cache
misses. Then its steps:

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
   `xtask/Cargo.toml` exists. It runs `cargo xtask check --all`
   (`nebensound-repo xtask`) from the repository root with the toolchain the
   xtask pins (the `rust-toolchain.toml` in `xtask/`, else the one in the
   repository root); an xtask that pins none fails the step. This is where a
   repository's own checks run, as mandatory as the register's, without a
   workflow or a ruleset entry of their own. The xtask's code is Rust like any
   other, so phase 0 demands a unit that runs the `rust` check over `xtask/`.

### Where a failed check is found

The job `nebensound / Check` only gathers: it reports that something failed and
holds no finding. It checks nothing out; it gets the tool as Declare does and
calls it with what it is handed (`nebensound-repo gather` and `nebensound-repo
unwritten`). The finding is in the job of the unit that failed, in the step
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
Publish afterwards. One target is written without a block: `release-assets`,
which the release stage carries out itself (see [Publish](#publish)). Every
other target has no building block yet, so a unit that publishes there fails
`nebensound / Check` today.

## Build

### One job per unit that builds in this run

Every unit whose build the plan names and that the plan gives a version of
this run, in a run that is not one after a push to a pre-branch, gets a job of
its own, named `nebensound / Build repository` for the
repository and `nebensound / Build component <name>` for a component, on
`ubuntu-latest`. A build whose register entry lists targets, such as
`cargo-release`, gets one job per target instead, on the runner the register
names for that target, with the target behind the name:
`nebensound / Build repository (x86_64-unknown-linux-musl)`. Each of them
builds for the target of the machine it runs on, and `nebensound / Build`
gathers them all. The plan names the build of a unit that carries a version of
its own and generates no children (see
[the declaration](declaration.md#versions)), and gives a unit a
version of the run when the commit touches it. A unit the commit does not touch
has no new artifact to hand on and gets no build job, also where a version
writer writes its current version for a package that depends on it. A
build job starts only when Declare and `nebensound / Check` passed: nothing is
built from a tree whose checks failed. It checks the repository out, gets the
tool as Declare does, and runs its steps:

1. **Not written yet.** It fails, by name, when the unit's build has no
   building block yet, so that a pull request says what is missing. The builds
   written today are listed in [the README](../README.md#building-blocks).
2. **One step per version writer** the unit selects, each the building block
   `version-<writer>` with the plan, so that the manifest carries the version
   of the run. A version writer without a building block has already failed
   `nebensound / Check`, so no build starts without its version.
3. **The build**, the building block `build-<name>` over the unit's path, with
   the plan where the build takes the unit's version of the run, as
   `build-docker` and `build-cargo-release` do. It builds the artifact once,
   tests the artifact itself and uploads it as an artifact of the run.

A step does not run after one that failed: an artifact built from versions that
were not written would be the wrong one. Tests of the source belong to Check;
the build tests what it hands on.

### The summary of the builds

`nebensound / Build` runs after Declare, `nebensound / Check` and every build
job, whatever their result, so it reports on every run. It fails when phase 0
did not pass, since then nothing is known about the builds. When nothing builds
in this run, as after a push to a pre-branch, it passes with a sentence that
says so, whatever Check found.
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

Every unit the history gives a version of the pushed commit, that is, every
unit the pushed commit touches, gets its records: a tag on the pushed commit,
named as [the declaration](declaration.md#tag-names) names a unit's records,
and a GitHub Release of it.

| Unit           | Version on `main` | The release                     |
| -------------- | ----------------- | ------------------------------- |
| The repository | `X.Y.Z`           | A full release, and the latest. |
| A component    | `X.Y.Z`           | A full release, not the latest. |

Every merge into `main` is a full release: a version on `main` is never a
pre-release.

A unit the commit does not touch gets nothing, also where a version writer
writes its current version for a package that depends on it: a tag on the
pushed commit with that version would not stand on the commit the history
gives it, and phase 0 would report it as `conflicting-tag` in the next run.

The job checks the repository out with its whole history and every tag, builds
the tool, fetches the artifacts of the run's builds with
`actions/download-artifact`, each into a folder of its own named after it, and
calls `nebensound-repo release --assets <that folder>`, which does the rest
([the tool](tool.md#the-release-stage)). It holds no other logic. The tool
works in this order:

1. **Decide what to release.** Nothing is written yet. The tool reads the
   version of every unit from the history, as phase 0 does. When the tag of a
   unit already stands on another commit, the job fails here and names it,
   before anything is written: a version belongs to exactly one commit. When a
   newer record of the unit already holds the pushed commit, as after a re-run
   of an older run, the unit is left out with a sentence that says so, and the
   job stays green. For each unit to release it finds the previous record: the
   tag of the unit with the highest version below the one released, on a
   commit the pushed commit contains. For each unit to release that publishes
   to `release-assets`, it holds the artifacts it was handed against what the
   release needs: every target of the unit's build has its artifact, every
   artifact of the build belongs to the version released and to the pushed
   commit and is for a target of the build, and each holds exactly one plain
   file, the archive, named for its target. Anything else fails the job here,
   each by name, before anything is written, unless the unit's release is
   published already: it takes no archive then. Artifacts of other units or of
   other kinds are passed over.
2. **Create the tags and the drafts.** For each unit, the tag on the pushed
   commit, a lightweight one, and the release as a draft, titled
   `<unit> <version>` (the repository by its name, such as `pipeline 0.4.2`),
   with notes GitHub generates against the previous record. A unit's first
   release carries one fixed sentence instead.
3. **The publish targets**, while the releases are drafts: an immutable release
   takes no file once it is published. For `release-assets`, every archive is
   attached to the unit's draft under its own name. An asset of that name the
   draft holds already with the same digest is kept; one with another digest,
   or whose upload broke off, is deleted and the archive uploaded again. A
   call that fails here fails the job, and no release is published. Every
   other target fails `nebensound / Check` before anything is built, so no
   unit that reaches Publish names one.
4. **Publish the releases.** Each draft is published as a full release, and
   marked as the latest release or not in the same call: GitHub honours that
   mark only there. When the published release shows that it is not
   immutable, because the repository has immutable releases switched off, the
   job writes a warning and a line in the summary of the run, and goes on.

A re-run completes what an earlier attempt left undone and changes nothing
else: a tag that stands on the pushed commit is kept, a draft the attempt left
is taken up with the archives it holds already, and a release published
already is left as it is. A re-run of Publish is handed the artifacts of its
run only while they are kept: the builds keep them for 7 days, and a run can be
re-run for longer. A published release takes no archive, so a re-run after
that is still green and changes nothing. A draft not published yet still
takes them, so a re-run that has to finish a draft after the artifacts expired
fails, each archive named as missing. The tool writes what it did into the log, and the summary of
the run lists every unit with its release, or the sentence why there is none.

Tags and releases are records. Publish reads them for what records answer, the
notes since the previous record and whether a newer one holds the commit, and
never for a version: the versions are resolved from the history, as for the
plan.

A release published with the workflow's token starts no workflow, so a caller
that listens to tags or releases is not started by Publish.

Of what another job wrote, only whether that job passed and the artifacts of
the builds reach Publish: it takes no job output, reads the versions from the
history itself, and gets the tool itself: the verified binary of the release
on the commit the caller pins, as every job does, or else a build from the
source of the pipeline at that commit, without the cache
([`.github/actions/tool`](../.github/actions/tool) with `cache: "false"`). Every
job of a branch may write the cache, a job that runs code of the repository
among them, so nothing in it is to run with a token that can write. The
artifacts are the products the builds tested, and Publish takes them as data:
the tool holds each against the version and the commit of the run and uploads
it, and never starts one. What does run with that token is the tool, the Rust
crates it is built from, as its `Cargo.lock` pins them, and the toolchain its
`rust-toolchain.toml` names. It is built with its default features alone,
which leave out the terminal interface (`ui`). Before the release, the job
runs `actions/download-artifact`, which is not handed the token. The token goes
to the step that fetches the released binary, whose gh only reads with it, and
to the one step that calls the tool, which hands it to curl on curl's standard
input, never on a command line.

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

The caller file starts the workflow on every pull request, when it is opened,
synchronized, reopened or edited, and on every push to `main`, `pre` and
`pre-*`:

```yaml
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
```

A pull request and a push to `main` run the same jobs, Declare, Version, Check
and Build, with the same steps, and a push to `main` runs Publish after them.
The versions depend on the event: on a pull request `Version` shows the version
the merge would get with `-pr.<number>` behind it, and the plan carries the
same versions, which the package test of `check-rust` and the build jobs write
and build with. A pull request into a pre-branch is a pull request like any
other: it gets the checks, the version with `-pr.<number>` behind it and the
builds, so what lands on a pre-branch was built before.

A run of `edited` is a run like any other: every job runs, as after a push to
the pull request, since a ruleset counts a skipped required check as passed. It
starts on every change of the title or the description, from which the version
shown is read, and when GitHub moves an open pull request onto `main` because
the pre-branch it went into was deleted at its landing. That costs a full run
per edit of the title or description, and the newer run cancels the one still
going.

A push to a pre-branch runs Declare and Check as a push to `main` does.
Versions and pre-releases of pre-branches are not built yet, so `Version`
resolves no version and says so, as a notice and as a line in the summary of
the run, no unit builds, `Build` passes with a sentence that says so, and
Publish does not run: nothing is published. The plan Declare hands on is the
one a push to `main` would get, so the package test of `check-rust` writes the
versions phase 0 reads into its own checkout, packages and puts the files back,
as on `main`; no job reports those versions in the summary or builds with
them.

A newer push or edit of a pull request cancels its run that is still going:
only the newest state of a pull request counts. A run after a push, to `main`
or to a pre-branch, is never cancelled. It waits in the one queue of the
repository until the run before it has finished, so the runs after two merges
publish in the order of their pushes and with two different versions, and a run
that waits is never replaced by a newer one. The workflow sets this itself, in
its `concurrency`: a pull request's group cancels and keeps one run waiting, the
repository's group keeps up to 100 runs waiting and cancels none, and a run
beyond that is cancelled. GitHub gives the waiting runs their turn in the order
in which they began to wait, and since a run may begin to wait a moment after
it was started, it does not guarantee that order. Should two runs swap places,
the versions stay what the history gives each commit; the older run then finds
its commit held by the newer release and releases nothing (see
[Publish](#publish)).

The caller's job grants `contents: write`, `pull-requests: read`,
`id-token: write`, `packages: write`, `attestations: write` and
`deployments: write`, and only Publish asks for more than a read token,
`contents: write`: every other job reads the repository and writes nothing
back, and so does every job of a pull request. The caller grants what the
planned parts need, since a permission added to it later would mean a change
in every repository (see [Calling the workflow](calling.md#permissions)).

This repository calls the workflow the same way, with one difference: its own
caller runs the workflow at the commit under test rather than at a pinned
version, so a change to the pipeline is checked by the pipeline it changes.
