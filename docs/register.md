# Register

The vocabulary of the declaration: every name a `pipeline` may use, and what the
pipeline adds to every repository without being asked. The fields that use these
names are described in [the reference of the declaration](declaration.md).

This page is generated. `nebensound register` prints it from the register and
the defaults compiled into the tool, and a test of the tool fails when the
committed page differs. To change it, change `register.toml` or `defaults.toml`
beside the library in `tools/nebensound/crates/nebensound-declaration/`, then
write the page again from `tools/nebensound/`:

```sh
cargo run --quiet -- register > ../../docs/register.md
```

A name stands here once the format knows it. Whether a step behind it is built
yet is a separate question: [the phases](phases.md) say which phases run today,
and a check whose building block is not written yet fails by name in its unit.

## Base set

What holds in every repository without being declared: these checks are in
force for every file, and these ecosystems are watched. A pipeline adds to
them and cannot take anything away.

| What       | Names                              |
| ---------- | ---------------------------------- |
| Checks     | `markdown`, `yaml`, `json`, `toml` |
| Dependabot | `github-actions`                   |

## Presets

A preset is a pipeline with a name: `pipeline = "<preset>"` takes all of it.
Its checks come on top of the base set.

| Preset                        | Version    | Check                  | Build           | Publish            | Dependabot        |
| ----------------------------- | ---------- | ---------------------- | --------------- | ------------------ | ----------------- |
| `asciidoc-site`               | `asciidoc` | `asciidoc`             | `asciidoctor`   | `pages`            | none              |
| `esphome-firmware`            | none       | `yaml`                 | `esphome`       | `release-assets`   | none              |
| `none`                        | none       | none                   | none            | nothing            | none              |
| `platformio-firmware`         | none       | `cpp`                  | `platformio`    | `release-assets`   | none              |
| `python-service`              | none       | `python`, `dockerfile` | `docker`        | `ghcr`             | `uv`, `docker`    |
| `rust-binary`                 | `cargo`    | `rust`                 | `cargo-release` | `release-assets`   | `cargo`           |
| `rust-library`                | `cargo`    | `rust`                 | `cargo-package` | `private-registry` | `cargo`           |
| `rust-library-package`        | `cargo`    | none                   | `cargo-package` | `private-registry` | none              |
| `rust-library-public`         | `cargo`    | `rust`                 | `cargo-package` | `crates.io`        | `cargo`           |
| `rust-library-public-package` | `cargo`    | none                   | `cargo-package` | `crates.io`        | none              |
| `rust-service`                | `cargo`    | `rust`, `dockerfile`   | `docker`        | `ghcr`             | `cargo`, `docker` |
| `rust-workspace`              | none       | `rust`                 | none            | nothing            | `cargo`           |
| `typst-document`              | none       | `typst`                | `typst`         | `release-assets`   | none              |

## Checks

A check covers every file with one of its extensions or file names. A file
that no check covers has to be excluded.

| Check        | Extensions                                               | File names                    |
| ------------ | -------------------------------------------------------- | ----------------------------- |
| `asciidoc`   | `.adoc`, `.asciidoc`                                     | none                          |
| `cpp`        | `.c`, `.h`, `.cpp`, `.hpp`, `.cc`, `.hh`, `.cxx`, `.ino` | none                          |
| `css`        | `.css`, `.scss`                                          | none                          |
| `dockerfile` | `.dockerfile`                                            | `Dockerfile`, `Containerfile` |
| `html`       | `.html`, `.htm`                                          | none                          |
| `javascript` | `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.tsx`             | none                          |
| `json`       | `.json`, `.jsonc`, `.geojson`                            | `composer.lock`, `flake.lock` |
| `markdown`   | `.md`, `.markdown`                                       | none                          |
| `php`        | `.php`                                                   | none                          |
| `python`     | `.py`, `.pyi`                                            | none                          |
| `qml`        | `.qml`                                                   | none                          |
| `r`          | `.r`                                                     | none                          |
| `rust`       | `.rs`                                                    | none                          |
| `shell`      | `.sh`, `.bash`                                           | none                          |
| `sql`        | `.sql`                                                   | none                          |
| `toml`       | `.toml`                                                  | `Cargo.lock`, `uv.lock`       |
| `typst`      | `.typ`                                                   | none                          |
| `yaml`       | `.yml`, `.yaml`                                          | none                          |

## Builds

A build produces one kind of artifact. The kind decides which targets can take
it.

| Build           | Produces     |
| --------------- | ------------ |
| `asciidoctor`   | `site`       |
| `cargo-package` | `crate`      |
| `cargo-release` | `executable` |
| `docker`        | `container`  |
| `esphome`       | `firmware`   |
| `platformio`    | `firmware`   |
| `typst`         | `document`   |

## Publish targets

A target accepts some kinds of artifact, and it can demand that a check is in
force for the unit that publishes there. It also says how a unit that publishes
there is released when the unit does not say: a unit with several targets is
released in two stages if any of them is, and a unit without a target in one
(see [Releases](declaration.md#releases)).

| Target             | Accepts                              | Requires check | Release        |
| ------------------ | ------------------------------------ | -------------- | -------------- |
| `crates.io`        | `crate`                              | `rust`         | `two-stage`    |
| `ghcr`             | `container`                          | none           | `single-stage` |
| `pages`            | `site`                               | none           | `single-stage` |
| `private-registry` | `crate`                              | none           | `two-stage`    |
| `release-assets`   | `executable`, `firmware`, `document` | none           | `single-stage` |

## Version writers

A version writer receives a unit's version as a derived copy.

| Writer     | Writes                                                              |
| ---------- | ------------------------------------------------------------------- |
| `asciidoc` | the :revnumber: attribute in the unit's AsciiDoc source document    |
| `cargo`    | the package version in the unit's Cargo.toml, written through Cargo |

## Ecosystems

The package ecosystems Dependabot can watch, and the file names phase 0
recognizes a manifest of each by.

| Ecosystem        | Dependabot name  | Manifests                     |
| ---------------- | ---------------- | ----------------------------- |
| `cargo`          | `cargo`          | `Cargo.lock`                  |
| `docker`         | `docker`         | `Dockerfile`, `Containerfile` |
| `github-actions` | `github-actions` | none                          |
| `npm`            | `npm`            | `package-lock.json`           |
| `pip`            | `pip`            | `requirements.txt`            |
| `uv`             | `uv`             | `uv.lock`                     |

## Excluded everywhere

What nobody checks, in any repository. A declaration can pull an entry back in
with `[[include]]`; it cannot add a pattern of its own.

| Pattern             | Reason                                                                        |
| ------------------- | ----------------------------------------------------------------------------- |
| `*.png`             | image, binary; no tool has an opinion about it                                |
| `*.jpg`             | image, binary; no tool has an opinion about it                                |
| `*.jpeg`            | image, binary; no tool has an opinion about it                                |
| `*.gif`             | image, binary; no tool has an opinion about it                                |
| `*.webp`            | image, binary; no tool has an opinion about it                                |
| `*.svg`             | image as source text; checkable, but without a formatter in the organization  |
| `*.ico`             | image, binary; no tool has an opinion about it                                |
| `*.pdf`             | rendered result, not the source                                               |
| `*.xlsx`            | spreadsheet document; packed XML that no tool here opens                      |
| `*.docx`            | text document; packed XML that no tool here opens                             |
| `*.odt`             | text document; packed XML that no tool here opens                             |
| `*.woff`            | font, binary; no tool has an opinion about it                                 |
| `*.woff2`           | font, binary; no tool has an opinion about it                                 |
| `*.ttf`             | font, binary; no tool has an opinion about it                                 |
| `*.otf`             | font, binary; no tool has an opinion about it                                 |
| `*.zip`             | archive; its content is reachable by no check                                 |
| `*.gz`              | archive; its content is reachable by no check                                 |
| `*.tar`             | archive; its content is reachable by no check                                 |
| `*.bin`             | binary artifact; not a source, and no tool reads it                           |
| `*.hex`             | firmware image; a generated result, not the source                            |
| `*.elf`             | firmware image; a generated result, not the source                            |
| `*.stl`             | CAD geometry; partly readable as text, but without a tool in the organization |
| `*.step`            | CAD geometry; partly readable as text, but without a tool in the organization |
| `*.gcode`           | machine code; generated by the slicer, not written                            |
| `*.db`              | database file, binary; no tool reads it                                       |
| `**/LICENSE`        | legal text, taken over unchanged                                              |
| `**/NOTICE`         | legal text, taken over unchanged                                              |
| `**/CODEOWNERS`     | a syntax of its own, read by GitHub; no formatter for it                      |
| `**/.gitignore`     | tool configuration without a formatter                                        |
| `**/.gitattributes` | tool configuration without a formatter                                        |
| `**/.dockerignore`  | tool configuration without a formatter                                        |
| `**/.editorconfig`  | tool configuration without a formatter                                        |
| `**/yarn.lock`      | lockfile in a format of its own; no tool in the organization checks it        |
| `**/Gemfile.lock`   | lockfile in a format of its own; no tool in the organization checks it        |
| `**/justfile`       | command list in a syntax of its own; no tool in the organization checks it    |
| `**/Makefile`       | command list in a syntax of its own; no tool in the organization checks it    |
