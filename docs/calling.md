# Calling the workflow

What a repository needs to know when it starts calling `nebensound.yml`, and
that the workflow's reference does not state elsewhere. The caller file itself
is in [the README](../README.md#calling-the-pipeline); what runs and under which name is
in [the phases](phases.md).

## Pinning while no release exists

The reference in the caller file is pinned to an exact version. Until the first
release there is none, so pin the full 40-character SHA of a commit on `main`
and write the commit's date behind it as a comment, so that a reader sees how old
the pin is without looking the commit up:

```yaml
jobs:
  nebensound:
    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@<full commit SHA> # 2026-11-19
    permissions:
      contents: read
```

The date is the commit's, as `git log -1 --format=%cs <SHA>` prints it, not the
day of the pin. A shortened SHA or a branch name is not a pin. Without a
release, nothing raises the pin: raise it by hand, to the then current commit
of `main`. From the first release on, pin the release tag, such as `v0.0.1`,
which Dependabot raises as a pull request.

## The schema line runs ahead of the pin

The `#:schema` line of `.nebensound/repo.toml` points at `main`, so an editor
completes and marks keys by the newest schema, which can be ahead of the
workflow the repository pins. A key the newest schema knows can therefore be
fine in the editor and an error in the run. The check does not follow the line:
`check-toml` and phase 0 always use the schema of the pinned version, whether
the line is there or not. When the editor and the run disagree, the run is
right for this repository; raise the pin to use the key. See
[the declaration](declaration.md#the-file).

## Changing one linter rule

A repository's own Markdownlint or yamllint configuration is laid over the
pipeline's default rule by rule, so it names only the rule it wants different;
nothing of the default has to be repeated. [Linter rules](../README.md#linter-rules)
in the README states how, with an example of a one-rule configuration file.

For a single place in a single file, the tool's own inline directive is still
the smaller way and works in the check:

```markdown
<!-- markdownlint-disable-next-line MD033 -->
<details>
```

`<!-- markdownlint-disable MD033 -->` and `<!-- markdownlint-enable MD033 -->`
switch a rule off and on again around a stretch of a file, and yamllint reads a
comment such as `# yamllint disable-line rule:truthy`. What takes a file out of
a check is a different matter: only an `[[exclude]]` entry does that (see
[Exclusions](../README.md#exclusions)).
