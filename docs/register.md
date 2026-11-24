# Register

The vocabulary of the declaration: every name a `pipeline` may use, and what the
pipeline adds to every repository without being asked. The fields that use these
names are described in [the reference of the declaration](declaration.md).

This page is generated. `nebensound-repo register` prints it from the register
and the defaults compiled into the tool, and a test of the tool fails when the
committed page differs. To change it, change `register.toml` or `defaults.toml`
at the root of the repository, then write the page again from there:

```sh
cargo run --quiet -- register > docs/register.md
```

A name stands here once the format knows it. Whether a step behind it is built
yet is a separate question: [the phases](phases.md) say which phases run today,
and a check whose building block is not written yet fails by name in its unit.

What a name means does not change within an edition. Versions are resolved from
the declaration of every earlier commit, read in its edition with the register
of the tool that runs (see [Versions](declaration.md#versions) and
[Editions](declaration.md#editions)). A default that changed under the same name
(what a preset brings, for example) would change what the declarations of old
commits mean, and with it the versions of an existing history. Such a change
comes as a new name or as a new edition.

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
that no check covers has to be excluded. The programs are what the check calls
(see [Programs](#programs)). The coverage toolchain is the Rust toolchain a
check measures coverage on, on top of the one the workspace pins; the tool
installs it through rustup.

| Check        | Extensions                                               | File names                                                    | Programs                                                 | Coverage toolchain   |
| ------------ | -------------------------------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------- | -------------------- |
| `asciidoc`   | `.adoc`, `.asciidoc`                                     | none                                                          | none                                                     | none                 |
| `cpp`        | `.c`, `.h`, `.cpp`, `.hpp`, `.cc`, `.hh`, `.cxx`, `.ino` | none                                                          | none                                                     | none                 |
| `css`        | `.css`, `.scss`                                          | none                                                          | `prettier`, `stylelint`                                  | none                 |
| `dockerfile` | `.dockerfile`                                            | `Dockerfile`, `Containerfile`                                 | `dockerfmt`, `hadolint`                                  | none                 |
| `html`       | `.html`, `.htm`                                          | none                                                          | `html-validate`, `lychee`, `prettier`                    | none                 |
| `javascript` | `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.tsx`             | none                                                          | none                                                     | none                 |
| `json`       | `.json`, `.jsonc`, `.geojson`                            | `composer.lock`, `flake.lock`, `Pipfile.lock`                 | `biome`, `jq`, `prettier`                                | none                 |
| `markdown`   | `.md`, `.markdown`                                       | none                                                          | `lychee`, `markdownlint-cli2`, `prettier`                | none                 |
| `php`        | `.php`                                                   | none                                                          | none                                                     | none                 |
| `python`     | `.py`, `.pyi`                                            | none                                                          | `coverage`, `pyright`, `ruff`, `uv`                      | none                 |
| `qml`        | `.qml`                                                   | none                                                          | none                                                     | none                 |
| `r`          | `.r`                                                     | none                                                          | none                                                     | none                 |
| `rust`       | `.rs`                                                    | none                                                          | `cargo-llvm-cov`                                         | `nightly-2026-11-14` |
| `shell`      | `.sh`, `.bash`                                           | none                                                          | none                                                     | none                 |
| `sql`        | `.sql`                                                   | none                                                          | none                                                     | none                 |
| `toml`       | `.toml`                                                  | `Cargo.lock`, `uv.lock`, `poetry.lock`, `pdm.lock`, `Pipfile` | `taplo`                                                  | none                 |
| `typst`      | `.typ`                                                   | none                                                          | none                                                     | none                 |
| `yaml`       | `.yml`, `.yaml`                                          | none                                                          | `actionlint`, `check-jsonschema`, `prettier`, `yamllint` | none                 |

## Programs

Every program a check or a build calls, pinned to one version.
`nebensound-repo setup` finds or installs the ones a repository's checks call,
or those of the checks and builds it is named (see
[the tool](tool.md#setting-up-the-programs)).
A program on a runtime is one package, which the runtime's installer puts in
place together with its companions, the packages pinned beside it that are
no command, and needs at least the version of the runtime its package states;
every other one is a build per platform. Each download is refused unless it
has the digest the register pins; the addresses and digests stand in
`register.toml`, each with the release data it was taken from. On a platform
without a build only the computer's copy can be used: the default mode of
`setup` takes it from the `PATH`, and `setup --pinned` fails for the program.

| Program             | Version   | Runtime                     | Builds                                                                                                                                       |
| ------------------- | --------- | --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `actionlint`        | `1.7.12`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64`                                        |
| `biome`             | `2.5.15`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64`                                        |
| `cargo-llvm-cov`    | `0.9.0`   | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64`                                        |
| `check-jsonschema`  | `0.39.0`  | `python`, `3.11.0` or later | one package for every platform                                                                                                               |
| `coverage`          | `7.17.4`  | `python`, `3.11.0` or later | one package for every platform                                                                                                               |
| `dockerfmt`         | `1.0.0`   | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`                                                                             |
| `hadolint`          | `2.15.1`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`                                                           |
| `html-validate`     | `11.21.1` | `node`, `22.22.0` or later  | one package for every platform                                                                                                               |
| `jq`                | `1.8.1`   | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`                                                           |
| `lychee`            | `0.24.2`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`                                                           |
| `markdownlint-cli2` | `0.23.2`  | `node`, `22.0.0` or later   | one package for every platform                                                                                                               |
| `prettier`          | `3.9.6`   | `node`, `14.0.0` or later   | one package for every platform                                                                                                               |
| `pyright`           | `1.1.414` | `node`, `14.0.0` or later   | one package for every platform                                                                                                               |
| `ruff`              | `0.17.6`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64`                                        |
| `stylelint`         | `18.0.0`  | `node`, `22.22.2` or later  | one package for every platform, with `stylelint-config-standard` `41.0.0`, `stylelint-config-standard-scss` `18.0.0`, `postcss-scss` `4.0.9` |
| `taplo`             | `0.10.0`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64`                                        |
| `trivy`             | `0.76.2`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`                                                           |
| `uv`                | `0.13.9`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64`                                        |
| `yamllint`          | `1.38.0`  | `python`, `3.10.0` or later | one package for every platform                                                                                                               |

## Runtimes

What a program on a runtime runs on. A runtime on the computer is used as it is,
whatever its version, as long as it prints one and can run the programs on it;
one that prints none is passed over, and so is one older than a program on it
needs. With none left, the runtime counts as missing. The pinned one is
installed only when the runtime is missing and someone agrees.

| Runtime  | Name    | Version   | Installs with | Builds                                                                                                |
| -------- | ------- | --------- | ------------- | ----------------------------------------------------------------------------------------------------- |
| `node`   | Node.js | `24.22.3` | `npm`         | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64` |
| `python` | Python  | `3.14.10` | `pip`         | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64` |

## Images

Every container image a build starts, pinned to the digest the registry states
for its tag. It is pulled by that digest, so the tag only says which version it
is.

| Image      | Repository      | Tag       | Digest                                                                    |
| ---------- | --------------- | --------- | ------------------------------------------------------------------------- |
| `buildkit` | `moby/buildkit` | `v0.34.2` | `sha256:c5f8862f52499773af85bc07695b2379ef08e8002352b0df2c4864c91d78dabe` |

## Builds

A build produces one kind of artifact. The kind decides which targets can take
it. A build that needs version writers takes the version of what it produces
from what they write: a unit that declares it selects them, or phase 0 fails
(see [`build-without-version-writer`](declaration.md#build-without-version-writer)).
A build that needs files reads them below the unit's path: a unit it builds
holds them, or phase 0 fails (see
[`build-without-file`](declaration.md#build-without-file)). The programs are
what the build calls (see [Programs](#programs)), the images what it starts
(see [Images](#images)). A build with targets builds once per target, each
in a job of its own on the runner named beside it (see
[the Build phase](phases.md#build)).

| Build           | Produces     | Needs version writers | Needs files         | Programs | Images     | Targets                                                                                                                            |
| --------------- | ------------ | --------------------- | ------------------- | -------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `asciidoctor`   | `site`       | none                  | none                | none     | none       | none                                                                                                                               |
| `cargo-package` | `crate`      | `cargo`               | none                | none     | none       | none                                                                                                                               |
| `cargo-release` | `executable` | `cargo`               | none                | none     | none       | `aarch64-apple-darwin` on `macos-latest`, `x86_64-apple-darwin` on `macos-26-intel`, `x86_64-unknown-linux-musl` on `ubuntu-22.04` |
| `docker`        | `container`  | none                  | `deploy/Dockerfile` | `trivy`  | `buildkit` | none                                                                                                                               |
| `esphome`       | `firmware`   | none                  | none                | none     | none       | none                                                                                                                               |
| `platformio`    | `firmware`   | none                  | none                | none     | none       | none                                                                                                                               |
| `typst`         | `document`   | none                  | none                | none     | none       | none                                                                                                                               |

## Publish targets

A target accepts some kinds of artifact, and it can demand that a check is in
force for the unit that publishes there. On a target with dependencies by
version, a registry, packages require each other by version: what a package
published there requires by path, and a pull request does not touch, must
have a release already, or the pull request fails (see
[`unreleased-requirement`](declaration.md#unreleased-requirement)).

| Target             | Accepts                              | Requires check | Dependencies by version |
| ------------------ | ------------------------------------ | -------------- | ----------------------- |
| `crates.io`        | `crate`                              | `rust`         | yes                     |
| `ghcr`             | `container`                          | none           | no                      |
| `pages`            | `site`                               | none           | no                      |
| `private-registry` | `crate`                              | none           | yes                     |
| `release-assets`   | `executable`, `firmware`, `document` | none           | no                      |

## Version writers

A version writer receives a unit's version as a derived copy.

| Writer     | Writes                                                              |
| ---------- | ------------------------------------------------------------------- |
| `asciidoc` | the :revnumber: attribute in the unit's AsciiDoc source document    |
| `cargo`    | the package version in the unit's Cargo.toml, written through Cargo |

## Ecosystems

The package ecosystems Dependabot can watch, and the file names phase 0
recognizes a manifest of each by. Where an ecosystem knows a lockfile, phase 0
warns when Git does not track the lockfile of a manifest (see
[`missing-lockfile`](declaration.md#missing-lockfile)).

| Ecosystem        | Dependabot name  | Manifests                     | Lockfile                                                           |
| ---------------- | ---------------- | ----------------------------- | ------------------------------------------------------------------ |
| `cargo`          | `cargo`          | `Cargo.lock`                  | `Cargo.lock` of a `Cargo.toml`, in the root of the Cargo workspace |
| `docker`         | `docker`         | `Dockerfile`, `Containerfile` | none                                                               |
| `github-actions` | `github-actions` | none                          | none                                                               |
| `npm`            | `npm`            | `package-lock.json`           | none                                                               |
| `pip`            | `pip`            | `requirements.txt`            | none                                                               |
| `uv`             | `uv`             | `uv.lock`                     | none                                                               |

## Excluded everywhere

What nobody checks, in any repository. A declaration can pull an entry back in
with `[[include]]`; it cannot add a pattern of its own. An entry that holds only
while the file is empty excuses nothing once the file holds content.

| Pattern              | Reason                                                                        |
| -------------------- | ----------------------------------------------------------------------------- |
| `*.png`              | image, binary; no tool has an opinion about it                                |
| `*.jpg`              | image, binary; no tool has an opinion about it                                |
| `*.jpeg`             | image, binary; no tool has an opinion about it                                |
| `*.gif`              | image, binary; no tool has an opinion about it                                |
| `*.webp`             | image, binary; no tool has an opinion about it                                |
| `*.svg`              | image as source text; checkable, but without a formatter in the organization  |
| `*.ico`              | image, binary; no tool has an opinion about it                                |
| `*.pdf`              | rendered result, not the source                                               |
| `*.xlsx`             | spreadsheet document; packed XML that no tool here opens                      |
| `*.docx`             | text document; packed XML that no tool here opens                             |
| `*.odt`              | text document; packed XML that no tool here opens                             |
| `*.woff`             | font, binary; no tool has an opinion about it                                 |
| `*.woff2`            | font, binary; no tool has an opinion about it                                 |
| `*.ttf`              | font, binary; no tool has an opinion about it                                 |
| `*.otf`              | font, binary; no tool has an opinion about it                                 |
| `*.zip`              | archive; its content is reachable by no check                                 |
| `*.gz`               | archive; its content is reachable by no check                                 |
| `*.tar`              | archive; its content is reachable by no check                                 |
| `*.bin`              | binary artifact; not a source, and no tool reads it                           |
| `*.hex`              | firmware image; a generated result, not the source                            |
| `*.elf`              | firmware image; a generated result, not the source                            |
| `*.stl`              | CAD geometry; partly readable as text, but without a tool in the organization |
| `*.step`             | CAD geometry; partly readable as text, but without a tool in the organization |
| `*.gcode`            | machine code; generated by the slicer, not written                            |
| `*.db`               | database file, binary; no tool reads it                                       |
| `**/LICENSE`         | legal text, taken over unchanged                                              |
| `**/NOTICE`          | legal text, taken over unchanged                                              |
| `**/CODEOWNERS`      | a syntax of its own, read by GitHub; no formatter for it                      |
| `**/.gitignore`      | tool configuration without a formatter                                        |
| `**/.gitattributes`  | tool configuration without a formatter                                        |
| `**/.dockerignore`   | tool configuration without a formatter                                        |
| `**/.editorconfig`   | tool configuration without a formatter                                        |
| `**/yarn.lock`       | lockfile in a format of its own; no tool in the organization checks it        |
| `**/Gemfile.lock`    | lockfile in a format of its own; no tool in the organization checks it        |
| `**/justfile`        | command list in a syntax of its own; no tool in the organization checks it    |
| `**/Makefile`        | command list in a syntax of its own; no tool in the organization checks it    |
| `**/.gitkeep`        | an empty marker that keeps a folder; only while the file is empty             |
| `**/.keep`           | an empty marker that keeps a folder; only while the file is empty             |
| `**/py.typed`        | an empty marker of a typed package (PEP 561); only while the file is empty    |
| `**/.python-version` | a version number read by uv; no tool has an opinion about its format          |
