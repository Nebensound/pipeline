# Schemas for check-toml

The JSON Schemas `check-toml` validates against, kept here for two reasons:

- Validating against them needs no request to SchemaStore: a pull request
  must not turn red because someone else's server does not answer. (The
  action still downloads Taplo itself from its GitHub release in the
  install step; this is about the schemas only.)
- The check is deterministic: the schemas are part of the commit a caller
  pins, so a change to a schema changes the version of the action, and
  never reaches a repository behind its back.

| File                       | For                                                    |
| -------------------------- | ------------------------------------------------------ |
| `cargo.json`               | `Cargo.toml`                                           |
| `cargo-lints-cargo.json`   | the `cargo` lints, referenced by `cargo.json`          |
| `cargo-lints-clippy.json`  | the `clippy` lints, referenced by `cargo.json`         |
| `cargo-lints-rust.json`    | the `rust` lints, referenced by `cargo.json`           |
| `cargo-lints-rustdoc.json` | the `rustdoc` lints, referenced by `cargo.json`        |
| `quikrun.json`             | a `package.metadata` table, referenced by `cargo.json` |

## Source

[SchemaStore](https://github.com/SchemaStore/schemastore), `src/schemas/json/`,
at commit `ce64da2a95a2bd40740c2d608206f8a36024d30b` (taken on 2026-10-09).
SchemaStore is licensed under the Apache License 2.0; its `LICENSE` and
`NOTICE` from that commit lie beside the schemas, unchanged. The two changes
below are modifications in the sense of section 4 of that licence, and this
file states them.

Two changes, made with `jq`, then formatted by Prettier:

- `$id` is removed. `check-toml` sets it at run time to the file's location on
  the runner.
- Every `$ref` to `https://www.schemastore.org/<name>.json` becomes `<name>.json`,
  so that it resolves to the file beside it.

```sh
jq --indent 2 'del(."$id") | walk(if type == "object" and has("$ref")
  and (."$ref" | type == "string")
  and (."$ref" | startswith("https://www.schemastore.org/"))
  then ."$ref" |= sub("^https://www.schemastore.org/"; "") else . end)' \
  "$source" > "<name>.json"
```

## Updating

Take the files again from a newer SchemaStore commit, together with its
`LICENSE` and `NOTICE`, apply the same two changes, and update the commit and
date above, in one pull request. A schema
that references a further one brings that one along.
