# Contributing

Thank you for helping. Every change follows the rules in
[`AGENTS.md`](AGENTS.md); this file describes how a change gets in.

## Proposing a change

1. Start from an issue. Look for an existing one first; if there is none, open
   one that says what should change and why. Report a vulnerability privately
   instead, as described in [`SECURITY.md`](SECURITY.md).
2. Create a branch from `main`, in a fork if you cannot push to this
   repository.
3. Open one pull request per issue, against `main`. Its description lists the
   details it chose within the issue's scope and any decision the issue does
   not make, and it closes the issue with `Closes #<number>`.

## Commit messages

Commit messages and pull request titles follow
[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), for
example `feat: add check-toml` or `fix: report a missing declaration`.

## Merging

Pull requests are squash-merged. The pull request title becomes the first line
of the commit on `main`, so it has to be a valid Conventional Commit header.
When a pull request may be merged is set out in [`AGENTS.md`](AGENTS.md).
