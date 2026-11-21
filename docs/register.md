# Register

The vocabulary of the declaration: every name a `pipeline` may use, and what the
pipeline adds to every repository without being asked. The fields that use these
names are described in [the reference of the declaration](declaration.md).

This page is generated. `nebensound-repo register` prints it from the register
and the defaults compiled into the tool, and a test of the tool fails when the
committed page differs. To change it, change `register.toml` or `defaults.toml`
in `tools/nebensound-repo/`, then write the page again from there:

```sh
cargo run --quiet -- register > ../../docs/register.md
```

A name stands here once the format knows it. Whether a step behind it is built
yet is a separate question: [the phases](phases.md) say which phases run today,
and a check whose building block is not written yet fails by name in its unit.

What a name means does not change within an edition. Versions are resolved from
the declaration of every earlier commit, read in its edition with the register
of the tool that runs (see [Versions](declaration.md#versions) and
[Editions](declaration.md#editions)). A default that changed under the same name
(the release mode of a target, or what a preset brings) would change what the
declarations of old commits mean, and with it the versions of an existing
history. Such a change comes as a new name or as a new edition.

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
(see [Programs](#programs)).

| Check        | Extensions                                               | File names                                         | Programs                                                 |
| ------------ | -------------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------------- |
| `asciidoc`   | `.adoc`, `.asciidoc`                                     | none                                               | none                                                     |
| `cpp`        | `.c`, `.h`, `.cpp`, `.hpp`, `.cc`, `.hh`, `.cxx`, `.ino` | none                                               | none                                                     |
| `css`        | `.css`, `.scss`                                          | none                                               | none                                                     |
| `dockerfile` | `.dockerfile`                                            | `Dockerfile`, `Containerfile`                      | `dockerfmt`, `hadolint`                                  |
| `html`       | `.html`, `.htm`                                          | none                                               | none                                                     |
| `javascript` | `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.tsx`             | none                                               | none                                                     |
| `json`       | `.json`, `.jsonc`, `.geojson`                            | `composer.lock`, `flake.lock`, `Pipfile.lock`      | `biome`, `jq`, `prettier`                                |
| `markdown`   | `.md`, `.markdown`                                       | none                                               | `lychee`, `markdownlint-cli2`, `prettier`                |
| `php`        | `.php`                                                   | none                                               | none                                                     |
| `python`     | `.py`, `.pyi`                                            | none                                               | none                                                     |
| `qml`        | `.qml`                                                   | none                                               | none                                                     |
| `r`          | `.r`                                                     | none                                               | none                                                     |
| `rust`       | `.rs`                                                    | none                                               | `cargo-llvm-cov`                                         |
| `shell`      | `.sh`, `.bash`                                           | none                                               | none                                                     |
| `sql`        | `.sql`                                                   | none                                               | none                                                     |
| `toml`       | `.toml`                                                  | `Cargo.lock`, `uv.lock`, `poetry.lock`, `pdm.lock` | `taplo`                                                  |
| `typst`      | `.typ`                                                   | none                                               | none                                                     |
| `yaml`       | `.yml`, `.yaml`                                          | none                                               | `actionlint`, `check-jsonschema`, `prettier`, `yamllint` |

## Programs

Every program a check or a build calls, pinned to one version.
`nebensound-repo setup` finds or installs the ones a repository's checks call
(see [the tool](../tools/nebensound-repo/README.md#setting-up-the-programs)).
A program on a runtime is one package, which the runtime's installer puts in
place, and needs at least the version of the runtime its package states;
every other one is a build per platform. Each download is refused unless it
has the digest the register pins; the addresses and digests stand in
`register.toml`, each with the release data it was taken from. On a platform
without a build only the computer's copy can be used: the default mode of
`setup` takes it from the `PATH`, and `setup --pinned` fails for the program.

| Program             | Version  | Runtime                     | Builds                                                                                                |
| ------------------- | -------- | --------------------------- | ----------------------------------------------------------------------------------------------------- |
| `actionlint`        | `1.7.12` | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64` |
| `biome`             | `2.5.15` | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64` |
| `cargo-llvm-cov`    | `0.9.0`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64` |
| `check-jsonschema`  | `0.39.0` | `python`, `3.11.0` or later | one package for every platform                                                                        |
| `dockerfmt`         | `1.0.0`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`                                      |
| `hadolint`          | `2.15.1` | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`                    |
| `jq`                | `1.8.1`  | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`                    |
| `lychee`            | `0.24.2` | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`                    |
| `markdownlint-cli2` | `0.23.2` | `node`, `22.0.0` or later   | one package for every platform                                                                        |
| `prettier`          | `3.9.6`  | `node`, `14.0.0` or later   | one package for every platform                                                                        |
| `taplo`             | `0.10.0` | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64` |
| `trivy`             | `0.76.2` | none                        | `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`                    |
| `yamllint`          | `1.38.0` | `python`, `3.10.0` or later | one package for every platform                                                                        |

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

## Builds

A build produces one kind of artifact. The kind decides which targets can take
it. A build that needs version writers takes the version of what it produces
from what they write: a unit that declares it selects them, or phase 0 fails
(see [`build-without-version-writer`](declaration.md#build-without-version-writer)).
A build that needs files reads them below the unit's path: a unit it builds
holds them, or phase 0 fails (see
[`build-without-file`](declaration.md#build-without-file)). The programs are
what the build calls (see [Programs](#programs)).

| Build           | Produces     | Needs version writers | Needs files         | Programs |
| --------------- | ------------ | --------------------- | ------------------- | -------- |
| `asciidoctor`   | `site`       | none                  | none                | none     |
| `cargo-package` | `crate`      | `cargo`               | none                | none     |
| `cargo-release` | `executable` | none                  | none                | none     |
| `docker`        | `container`  | none                  | `deploy/Dockerfile` | `trivy`  |
| `esphome`       | `firmware`   | none                  | none                | none     |
| `platformio`    | `firmware`   | none                  | none                | none     |
| `typst`         | `document`   | none                  | none                | none     |

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

## Dependabot policy

How every ecosystem a declaration has watched is watched, in every repository.
`nebensound-repo dependabot` prints the `.github/dependabot.yml` the declaration
and this policy give: one entry per ecosystem, each with the policy applied.
Phase 0 warns where the file in the tree departs from it (see
[`stale-dependabot`](declaration.md#stale-dependabot)). Only the defaults hold the
policy; a declaration cannot change it.

| Setting                                 | Key                        | Value            |
| --------------------------------------- | -------------------------- | ---------------- |
| How often                               | `schedule.interval`        | `weekly`         |
| Open pull requests per entry, at most   | `open-pull-requests-limit` | `5`              |
| Together in the group `minor-and-patch` | `update-types`             | `minor`, `patch` |
| Labels                                  | `labels`                   | `dependencies`   |

An update the group does not take comes alone, and so does a security update.
The commit prefix (`commit-message.prefix`) depends on the ecosystem, and some
dependencies stay out of the group (`exclude-patterns`), each raise of them a
pull request of its own:

| Ecosystem        | Commit prefix  | Out of the group                                       |
| ---------------- | -------------- | ------------------------------------------------------ |
| `cargo`          | `deps`         | none                                                   |
| `docker`         | `docker(deps)` | none                                                   |
| `github-actions` | `ci(deps)`     | `Nebensound/pipeline/.github/workflows/nebensound.yml` |
| `npm`            | `deps`         | none                                                   |
| `pip`            | `deps`         | none                                                   |
| `uv`             | `deps`         | none                                                   |

## Excluded everywhere

What nobody checks, in any repository. A declaration can pull an entry back in
with `[[include]]`; it cannot add a pattern of its own. An entry that holds only
while the file is empty excuses nothing once the file holds content.

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
| `**/.gitkeep`       | an empty marker that keeps a folder; only while the file is empty             |
| `**/.keep`          | an empty marker that keeps a folder; only while the file is empty             |
