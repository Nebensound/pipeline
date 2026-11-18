# The phases

A repository calls one workflow, `nebensound.yml`, from one caller file. The
workflow runs in phases, one after another; each phase is one or more jobs. This
page describes the phases that are built, the names their jobs report under,
and what runs on a pull request and on a push.

## What is built

| Phase   | Jobs                                                                                            |
| ------- | ----------------------------------------------------------------------------------------------- |
| Declare | `nebensound / Declare`                                                                          |
| Check   | `nebensound / Check repository`, `nebensound / Check component <name>` and `nebensound / Check` |

The workflow does not version, build, publish or deploy anything yet. Those
phases arrive one at a time, each when it does something: a job that only
existed to be green would report success for work nobody did. Until a phase is
built, no job stands in for it, and this page does not describe it.

## Declare

Phase 0. The job `nebensound / Declare` checks the calling repository out, runs
the tool `nebensound` on it and hands the result on:

1. The tool is built from the same commit of this repository as the workflow,
   and kept in the cache under a fingerprint of its sources, so two runs of the
   same tool share one build, and every unit job of the run takes it from there
   ([`.github/actions/tool`](../.github/actions/tool)).
2. `nebensound check` reads `.nebensound/repo.toml`, resolves it against the
   register and checks it against what Git tracks. It prints the resolved
   configuration, the annotated tree, the findings and a summary line.
   [The reference of the declaration](declaration.md) describes every field and
   every finding.
3. When phase 0 found no error, every Rust package `[[exclude]]` takes out as a
   whole (`nebensound excluded-packages`) is named in the summary of the run
   with its reason, as a notice.
4. Then `nebensound plan` prints the plan: every unit,
   with the checks started for it and the build it names. The units that start
   at least one check go on to the Check phase.

An error ends the phase here: there is no plan, no check runs, and
`nebensound / Check` fails. A warning stops nothing.

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

### The summary

`nebensound / Check` runs after Declare and every unit job, whatever their
result. It fails when phase 0 did not pass or when any unit job did not pass,
and it passes when every check the plan started passed, or when the plan started
none.

## Check names

Every job reports under a name in two parts, `nebensound / Check`: GitHub names
a job of a called workflow after the job that calls it and then the job in the
called workflow.

- **The first part** is the job in the caller file. It is `nebensound` in every
  repository, which is why the caller file is the same everywhere and gives the
  job no `name` of its own.
- **The second part** is the job in the central workflow. The pipeline fixes it.

A ruleset requires a name in both parts. Of the names a run reports today, it
requires `nebensound / Check`, and only that one: the unit jobs change with the
repository, and Declare is part of what `Check` sums up. Further required names
arrive with the phases that are not built yet.

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

Today both run the same jobs, Declare and Check, with the same steps; nothing
depends on the event yet. A newer run for the same branch or pull request
cancels the one still running. The permissions are read-only: the workflow
reads the repository and writes nothing back.

This repository calls the workflow the same way, with one difference: its own
caller runs the workflow at the commit under test rather than at a pinned
version, so a change to the pipeline is checked by the pipeline it changes.
