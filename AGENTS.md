# Contribution Rules

This repository is Nebensound's central pipeline: one reusable workflow, its
building blocks, and the tool `nebensound-repo` that reads a repository's
declaration. Every repository that calls it runs its code, so a change here
changes what every caller runs. These rules apply to every change, whoever or
whatever writes it.

## Principles

Every change is held against these six rules.
[The principles](docs/principles.md) give the reason for each, the trust model,
and what does not hold yet. Where a rule below or a page of the reference
describes something else, it describes the state of today, and that page names
the issue that changes it.

1. **A simple flow.** A preparing job reads the declaration and hands finished
   data to the jobs after it. The checks run as a matrix of the same call: one
   job for all checks that only read the repository's files, and one job for
   each check that runs code of the repository. What can be put together is
   put together. Adding a check does not add a special path to the workflow.
2. **Reading and executing are kept apart.** A reading check runs only programs
   of the pipeline on the repository's files, with configuration that is data.
   An executing check runs code of the repository: build scripts, tests, an
   image build. The two never share a job. Only "passed" or "failed" crosses
   from one job to another.
3. **All logic is Rust, in the tool, and tested.** A step of a workflow or of an
   action is one call of the tool. No logic in shell, in YAML expressions or in
   embedded scripts, beyond what a program that is a library needs to be
   called. Python only where nothing else is possible.
4. **External programs are the exception and are started from the tool.** What
   can sensibly be done in Rust inside the tool is done there. An external
   program is a standard tool: very widely used and maintained. Every external
   program, action and package stands in one list, with its exact version and
   a digest, in a form an automatic updater reads.
5. **One rule per kind of problem.** A case that was not thought of is answered
   by a rule that removes its kind, not by a special case on top. A pull
   request that adds a second exception to the same rule stops and asks for
   the rule.
6. **Secrets and write permissions reach only what has been merged.** No job
   that runs code of the repository gets a secret or a token that can write,
   and no step that holds a secret or such a token compiles or starts code of
   the repository. A check cannot protect its result against the code it
   checks; the review of the change does that. Annotations are a reading aid,
   not proof.

## Building blocks

- A building block performs **one step** and is named after its register entry
  with the category in front: `version-<writer>`, `check-<name>`,
  `build-<name>`, `publish-<target>`. It is never named after a preset: nobody
  executes a preset.
- A check that does not exist yet fails by name instead of being left out. A
  missing check reports nothing and leaves a pull request without a signal.
- A building block arrives with its test cases: at least one passing and one
  failing case under `tests/pass/` and `tests/fail/` beside its `action.yml`,
  each failing case naming the finding it must produce (see
  [the README](README.md#test-cases-of-the-building-blocks)). The gate runs them
  on every pull request through `cargo xtask check --all`, so the cases are the
  proof that a block works; a run linked once in the pull request that adds the
  block is not needed. A change to what a block finds changes its cases with it.

## The tool

The commands of `nebensound-repo` are limited to the tasks of a repository. The
tool has no external subcommands and never starts a program because a
subcommand's name matched it: the programs it starts are those of the register,
Git, curl and Cargo.

## Workflows and actions

- Workflows and jobs set minimal, explicit permissions, never wider than the
  caller granted.
- Third-party actions are pinned to a major version or to a full commit SHA.
- No workflow uses `pull_request_target`.
- The repository keeps no process of its own. It is checked by the pipeline it
  provides.

## Language and content

- Everything is English: identifiers, documentation, comments, logs, error
  messages, commit messages, issues and pull requests.
- Nothing private is named: no private repository, no internal host, no
  internal document. The repository is public, and so is its history.

## Merging

While the repository is on `0.0.x`, a pull request may be merged into `main`
once all four hold:

1. The gate is green. Until every check of the base set has a building block,
   the gate cannot be green, and the rule reads instead: the only failures are
   the named `Not written yet` steps and the closing `Check` that sums them up.
   For the checks without a building block, the local check results stated in
   the pull request stand in: nothing fails, and every check that could not be
   run is named with its reason.
2. It was reviewed in a separate session that did not write it, with the
   findings as inline comments. Author and reviewer may act through the same
   GitHub account; what counts is that the reviewer starts without the
   author's context.
3. Every review thread is resolved.
4. It makes no decision its issue does not make. A decision is a choice that
   changes behaviour, an interface, a rule or the scope. Wording and details
   inside the issue's scope are not decisions: the pull request lists them and
   the review covers them. A pull request that needs a decision waits until
   its issue is amended.
