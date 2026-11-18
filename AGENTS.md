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
once all four hold:

1. The gate is green. Until the gate exists (it arrives with the issue that
   adds the central workflow), the local check results stated in the pull
   request stand in for it: nothing fails, and every check that could not be
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

Merging into `release` is reserved for the maintainers.
