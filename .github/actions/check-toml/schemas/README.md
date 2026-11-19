# Schemas for check-toml

The JSON Schemas `check-toml` validates against, kept here for two reasons:

- Validating against them needs no request to SchemaStore: a pull request
  must not turn red because someone else's server does not answer. (The
  action still downloads Taplo itself from its GitHub release in the
  install step; this is about the schemas only.)
- The check is deterministic: the schemas are part of the commit a caller
  pins, so a change to a schema changes the version of the action, and
  never reaches a repository behind its back.

| File                          | For                                                    |
| ----------------------------- | ------------------------------------------------------ |
| `cargo.json`                  | `Cargo.toml`                                           |
| `cargo-lints-cargo.json`      | the `cargo` lints, referenced by `cargo.json`          |
| `cargo-lints-clippy.json`     | the `clippy` lints, referenced by `cargo.json`         |
| `cargo-lints-rust.json`       | the `rust` lints, referenced by `cargo.json`           |
| `cargo-lints-rustdoc.json`    | the `rustdoc` lints, referenced by `cargo.json`        |
| `quikrun.json`                | a `package.metadata` table, referenced by `cargo.json` |
| `nebensound-declaration.json` | `.nebensound/repo.toml`; not from SchemaStore, below   |

## Source

[SchemaStore](https://github.com/SchemaStore/schemastore), `src/schemas/json/`,
at commit `ce64da2a95a2bd40740c2d608206f8a36024d30b` (taken on 2026-10-09).
SchemaStore is licensed under the Apache License 2.0; its `LICENSE` and
`NOTICE` from that commit lie beside the schemas, unchanged. The two changes
below are modifications in the sense of section 4 of that licence, and this
file states them.

Three changes, the first two made with `jq` for every file, the third by hand
for `cargo.json`, then formatted by Prettier:

- `$id` is removed. `check-toml` sets it at run time to the file's location on
  the runner.
- Every `$ref` to `https://www.schemastore.org/<name>.json` becomes `<name>.json`,
  so that it resolves to the file beside it.
- `cargo.json` only, see [Deviation](#deviation-in-cargojson): `lints` no longer
  is an `anyOf`.

```sh
jq --indent 2 'del(."$id") | walk(if type == "object" and has("$ref")
  and (."$ref" | type == "string")
  and (."$ref" | startswith("https://www.schemastore.org/"))
  then ."$ref" |= sub("^https://www.schemastore.org/"; "") else . end)' \
  "$source" > "<name>.json"
```

## Deviation in `cargo.json`

Upstream defines `lints` as `anyOf` of the lint tables (`Lints`) and an object
`{ workspace = true }`. Taplo 0.10.0 judges the first alternative wrongly when
it sits inside an `anyOf`: any `[lints.rust]`, `[lints.clippy]`,
`[lints.rustdoc]` or `[lints.cargo]` table is rejected as "not valid under any
of the schemas listed in the 'anyOf' keyword", whatever it holds, and so is
the `unexpected_cfgs` entry with `check-cfg` that `check-rust` asks for. That
happens with the upstream files fetched from SchemaStore as well, and the
schemas already know the `check-cfg` key (`DetailedLint`), so it is not a
missing key. A single alternative in an `anyOf` fails the same way, the
alternative itself passes when referenced directly.

So the vendored package `lints` is a plain `$ref` to `PackageLints`, a copy
of `Lints` that gains the property `workspace` (a boolean) in place of the
second alternative. `Lints` itself stays as upstream has it, so
`[workspace.lints]`, which shares it, is not loosened. What this loosens:
`workspace = true` next to other tool tables of a package's `[lints]` is no
longer rejected here. What stays: a table of a tool holds lints, a lint is a
level or a detailed lint, `workspace` must be a boolean, and a value of another
type (for example a misspelled `workspac = true`, which is no table) still
fails. Two passing test cases of `check-toml` hold a manifest each, with a form
that Cargo and upstream both accept: `tests/pass/cargo-lints/Cargo.toml` carries
the `check-cfg` entry in `[lints.rust]`,
`tests/pass/cargo-lints-workspace/Cargo.toml` carries `[lints] workspace = true`.
`cargo xtask check` runs the block against them, so a new copy of the files that
brings the `anyOf` back turns the gate red here.

Drop this deviation when a Taplo release reads the `anyOf` correctly and the
vendored copy is taken again from SchemaStore.

## Updating

Take the files again from a newer SchemaStore commit, together with its
`LICENSE` and `NOTICE`, apply the same changes (and check the deviation
above), and update the commit and date above, in one pull request. A schema
that references a further one brings that one along.

## `nebensound-declaration.json`

Not taken from anywhere: `nebensound schema` derives it from the types phase 0
reads `.nebensound/repo.toml` with. A test in `tools/nebensound` fails when it
differs from what the types describe, so a change to the declaration format
carries its schema in the same commit. To write it again, run in
`tools/nebensound`:

```sh
cargo run --quiet -- schema > ../../.github/actions/check-toml/schemas/nebensound-declaration.json
```

and format it with Prettier, as `check-json` requires.

The schema is looser than phase 0, never stricter: where a rule cannot be said
in a schema, phase 0 rejects the file and the schema lets it through. Phase 0
stays the judge; the schema lets an editor show the fields, the allowed values
and a mistake while the file is written.

`check-toml` always validates `.nebensound/repo.toml` against the copy here,
the one of the pipeline version that reads the file. The public address of the
copy on `main`, and the `#:schema` line that points an editor at it, stand in
the [README of the repository](../../../../README.md#check-toml).
