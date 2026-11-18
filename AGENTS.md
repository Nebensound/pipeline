# Contribution Rules

This repository is Nebensound's central pipeline: one reusable workflow, its
building blocks, and the tool `nebensound` that reads a repository's
declaration. Every repository that calls it runs its code, so a change here
changes what every caller runs. These rules apply to every change, whoever or
whatever writes it.

## Building blocks

- A building block performs **one step** and is named after its register entry
  with the category in front: `check-<name>`, `build-<name>`,
  `publish-<target>`. It is never named after a preset: nobody executes a
  preset.
- A check that does not exist yet fails by name instead of being left out. A
  missing check reports nothing and leaves a pull request without a signal.

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
once

- the gate is green,
- a reviewer other than the author has reviewed it,
- every review thread is resolved, and
- it makes no decision its issue does not make.

Merging into `release` is reserved for the maintainers.
