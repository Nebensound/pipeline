//! `nebensound-repo run`: the checks for CSS, Dockerfiles, HTML, JSON,
//! Markdown, TOML and YAML, run from the tool.
//!
//! No real program runs here. Every program is a shell script that acts like
//! the program on what it is given and prints what it was called with, so
//! that a test sees both the flags and how the findings are reported. That
//! the real programs find what the blocks found is shown by the cases of the
//! blocks, which `cargo xtask check --all` runs through `run`. Hence Unix
//! only: the gate runs on Linux.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};

use nebensound_repo::Register;
use nebensound_repo::path::RelativePath;
use nebensound_repo::programs::Platform;
use nebensound_repo::run::{
    self, Check, Context, Level, css, dockerfile, html, markdown, toml, yaml,
};
use nebensound_repo::setup::{Environment, find};
use support::{QUIET, call};
use tempfile::TempDir;

/// Held by every test of this file for as long as it runs: they write
/// programs and start them, and Linux refuses to start a file another
/// process holds open for writing ("Text file busy").
static ALONE: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ALONE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a program prints and does, as `prettier` would on its arguments.
const PRETTIER: &str = r#"#!/bin/sh
printf '%s\n' "prettier $*"
echo "Checking formatting..."
status=0
seen=
for arg in "$@"; do
  if [ -n "$seen" ]; then
    case "$arg" in
      *unformatted*) echo "[warn] $arg"; status=1 ;;
      *unreadable*) echo "[error] $arg: SyntaxError: Unexpected token (1:1)"; status=2 ;;
    esac
  fi
  [ "$arg" = "--" ] && seen=1
done
[ "$status" -ne 0 ] && echo "[warn] Code style issues found in the above file. Run Prettier with --write to fix."
exit "$status"
"#;

const JQ: &str = r#"#!/bin/sh
for last in "$@"; do :; done
case "$last" in
  *two*) if [ "$1" = "--exit-status" ]; then exit 1; else echo 2; exit 0; fi ;;
  *broken*) echo "jq: error: parse error: Expected another key-value pair at line 1, column 22" >&2; exit 2 ;;
esac
exit 0
"#;

const BIOME: &str = r#"#!/bin/sh
printf '%s\n' "biome $*"
for arg in "$@"; do
  case "$arg" in
    *quoted*) echo "::error title=parse,file=$(pwd -P)/$arg,line=1,endLine=1,col=3,endColumn=9::JSON standard does not allow single quoted strings"; exit 1 ;;
  esac
done
exit 0
"#;

const TAPLO: &str = r#"#!/bin/sh
printf '%s\n' "taplo $*"
input=$(cat)
case "$1:$input" in
  format:*unformatted*) echo "ERROR operation failed error=the input was not properly formatted"; exit 1 ;;
  lint:*show*) exit 1 ;;
  lint:*invalid*)
    printf '%s\n' "error[x]: invalid TOML" "  ┌─ nowhere" "  ┌─ -:16" "  ┌─ -:a:b" \
      "  ┌─ -:2:5" "error[x" "warning: careful" " WARN something" "ERRORx" "error"
    exit 1 ;;
esac
exit 0
"#;

const LYCHEE: &str = r####"#!/bin/sh
printf '%s\n' "lychee $*"
while [ $# -gt 0 ]; do
  case "$1" in
    --output) out=$2; shift ;;
    --files-from) list=$2; shift ;;
    --config) config=$2; shift ;;
  esac
  shift
done
echo "config: $(cat "$config")"
echo "lines: $(cat "$list")"
if grep -q broken "$list"; then
  printf '%s\n' "# Summary" "" "## Errors per input" "" "### Errors in README-broken.md" "" \
    "* [ERROR] <file:///x/missing.md> (at 3:11) | File not found" \
    "* [ERROR] <file:///x/other.md> | File not found" "" "### Redirects in README.md" "" \
    "* something" > "$out"
  exit 2
fi
printf '%s\n' "# Summary" > "$out"
exit 0
"####;

/// lychee as the HTML check calls it: every configuration it is handed, the
/// folder a link from the root of a site is resolved below, and a report with
/// a broken link when a page asks for one.
const LYCHEE_PAGES: &str = r####"#!/bin/sh
printf '%s\n' "lychee $*"
while [ $# -gt 0 ]; do
  case "$1" in
    --output) out=$2; shift ;;
    --files-from) list=$2; shift ;;
    --config) echo "config: $(cat "$2")"; shift ;;
    --root-dir) root=$2; shift ;;
  esac
  shift
done
echo "lines: $(cat "$list")"
set -- "$root"/*
if [ -d "$root" ] && [ "$1" = "$root/*" ]; then echo "root: an empty folder"; fi
if grep -q broken "$list"; then
  printf '%s\n' "# Summary" "" "## Errors per input" "" "### Errors in broken.html" "" \
    "* [ERROR] <file:///x/missing.html> (at 11:29) | File not found" > "$out"
  exit 2
fi
printf '%s\n' "# Summary" > "$out"
exit 0
"####;

/// Node.js, which prints its version or runs the script it is handed the way
/// that script would: Stylelint or html-validate, which print each finding as
/// a plain line and as an annotation, or markdownlint-cli2, which names every
/// file from the folder it starts in, three levels below the fixture.
const NODE: &str = r##"#!/bin/sh
if [ "$1" = "--version" ]; then echo v24.0.0; exit 0; fi
echo "node $1 $2 TOOLS=$TOOLS"
case "$3" in
  *'load("stylelint"'*)
    echo "default: $(grep -c -e '- stylelint-config-standard' "$DEFAULT") configurations extended"
    echo "nothing ignored: [$(cat "$NOTHING")]"
    status=0
    for f in $(tr '\0' ' ' < "$LIST"); do
      case "$f" in
        *styled*)
          echo "$f:2:10 error Expected \"#00aa77\" to be \"#0a7\" (color-hex-length)"
          echo "::error file=$f,line=2,col=10::Expected \"#00aa77\" to be \"#0a7\" (color-hex-length)"
          status=1 ;;
        *warned*)
          echo "$f:1:1 warning Unexpected empty block (block-no-empty)"
          echo "::warning file=$f,line=1,col=1::Unexpected empty block (block-no-empty)" ;;
        *missing*)
          echo "::error file=$f::Stylelint cannot check it: Could not find \"stylelint-config-clean-order\""
          status=1 ;;
      esac
    done
    echo "Rules: the pipeline default"
    exit "$status" ;;
  *'"html-validate", "dist"'*)
    echo "default: $(grep -c 'html-validate:' "$DEFAULT") presets"
    echo "page rules: $(grep -c '^export' "$RULES") exports"
    status=0
    for f in $(tr '\0' ' ' < "$LIST"); do
      case "$f" in
        *invalid*)
          echo "$f:3:4 error no-implicit-button-type: <button> is missing recommended \"type\" attribute"
          echo "::error file=$f,line=3,col=4::no-implicit-button-type: <button> is missing recommended \"type\" attribute"
          status=1 ;;
        *loose*)
          echo "$f:2:1 warning no-trailing-whitespace: Trailing whitespace"
          echo "::warning file=$f,line=2,col=1::no-trailing-whitespace: Trailing whitespace" ;;
        *plugin*)
          echo "::error file=$f::html-validate cannot check it: Failed to load plugin \"html-validate-angular\""
          status=1 ;;
      esac
    done
    exit "$status" ;;
esac
echo "Rules: the pipeline default"
status=0
for f in $(tr '\0' ' ' < "$LIST"); do
  case "$f" in
    *heading*) echo "../../../repo/$f:3 error MD001/heading-increment Heading levels should only increment by one level at a time"; status=1 ;;
    *style*) echo "../../../repo/$f:1:1 error MD003/heading-style Heading style"; status=1 ;;
  esac
done
exit "$status"
"##;

/// The Python of yamllint's environment, which runs the linter the way the
/// script would.
const PYTHON: &str = r#"#!/bin/sh
echo "python $1 OWN=$OWN"
status=0
for f in $(tr '\0' ' ' < "$LIST"); do
  case "$f" in
    *duplicate*)
      echo "::group::$f"
      echo "::error file=$f,line=2,col=1::2:1 [key-duplicates] duplication of key \"name\" in mapping"
      echo "::endgroup::"
      status=1 ;;
  esac
done
exit "$status"
"#;

/// actionlint, which prints nothing but its JSON format, as the real one
/// does, and so writes what it was called with beside itself.
const ACTIONLINT: &str = r#"#!/bin/sh
printf '%s\n' "actionlint $*" > "$0.args"
status=0
for arg in "$@"; do
  case "$arg" in
    *broken*) echo "[{\"message\":\"\\\"runs-on\\\" section is missing in job \\\"test\\\"\",\"filepath\":\"$arg\",\"line\":10,\"column\":3,\"kind\":\"syntax-check\",\"snippet\":\"x\"}]"; status=1 ;;
    *unreadable*) echo "could not read \"$arg\": permission denied"; status=3 ;;
  esac
done
[ "$status" -eq 0 ] && echo "[]"
exit "$status"
"#;

const JSONSCHEMA: &str = r#"#!/bin/sh
printf '%s\n' "check-jsonschema $*"
for last in "$@"; do :; done
if grep -q schedules "$last"; then
  echo "Schema validation errors were encountered."
  echo "  $last::\$.updates[0]: Additional properties are not allowed ('schedules' was unexpected)"
  echo "  elsewhere.yml::\$.x: not this file"
  exit 1
fi
echo "ok -- validation done"
"#;

/// dockerfmt, which says what dockerfmt says about a file of that name.
const DOCKERFMT: &str = r#"#!/bin/sh
echo "dockerfmt $*"
for last in "$@"; do :; done
case "$last" in
  *unformatted*) echo "$last is not formatted"; echo "other is not formatted"; exit 1 ;;
  *left*)
    echo "$last:2: left unformatted: BuildKit would read it differently" >&2
    echo "$last:9: left unformatted: beyond the end" >&2
    echo "$last:x: left unformatted: no number" >&2
    exit 0 ;;
  *unreadable*)
    echo "dockerfmt: $last: line 3: unknown instruction: FORM" >&2
    echo "dockerfmt: $last: no such thing" >&2
    exit 1 ;;
  *warned*) echo "dockerfmt: warning: indent_size 3 is not used" >&2; exit 0 ;;
  *crashing*) echo "panic: somewhere" >&2; exit 3 ;;
  *killed*) kill -9 $$ ;;
esac
exit 0
"#;

/// Hadolint: with `--verbose` and no file, the configuration it would run
/// with, as the words in the configuration file ask; with a file, the JSON
/// report that file's name asks for, written to the last `--output`.
const HADOLINT: &str = r#"#!/bin/sh
outputs=; file=; verbose=; seen=
while [ $# -gt 0 ]; do
  if [ -n "$seen" ]; then file=$1; shift; continue; fi
  case "$1" in
    --verbose) verbose=1 ;;
    --config) config=$2; shift ;;
    --output) outputs="$outputs $2"; shift ;;
    --) seen=1 ;;
  esac
  shift
done
if [ -n "$verbose" ]; then
  echo "never shown"
  words=$(cat "$config")
  case "$words" in
    *broken*) echo '"Error parsing your config file at line 1"' >&2; exit 0 ;;
    *silent*) echo "Configuration:" >&2; exit 0 ;;
  esac
  echo "Configuration:" >&2
  case "$words" in *nofail*) echo "  no fail: True" >&2 ;; *) echo "  no fail: False" >&2 ;; esac
  case "$words" in
    *ignore*) echo "  failure threshold: ignore" >&2 ;;
    *error*) echo "  failure threshold: error" >&2 ;;
    *) echo "  failure threshold: info" >&2 ;;
  esac
  case "$words" in *formats*) printf '%s\n' "  output format:" "    - tty" "    - sarif" "  verbose: True" >&2 ;; esac
  exit 0
fi
echo "hadolint --config $config with $(export -p | grep -c HADOLINT_) HADOLINT variable(s)"
echo "hadolint about $file" >&2
names=
for out in $outputs; do names="$names ${out##*/}"; last=$out; done
echo "outputs:$names"
for out in $outputs; do [ "$out" = "$last" ] || echo tty > "$out"; done
case "$file" in
  *noreport*) exit 1 ;;
  *garbage*) echo "not json" > "$last" ;;
  *object*) echo "{}" > "$last" ;;
  *latest*)
    printf '%s\n' '[{"line":1,"code":"DL3007","message":"Using latest is prone to errors","column":1,"file":"x","level":"warning"},' \
      '{"line":2,"code":"DL3059","message":"Multiple consecutive RUN","column":1,"level":"info"},' \
      '{"line":3,"code":"SC1001","message":"odd%\nline","column":"5","level":"style"},' \
      '{"code":"X","level":null}]' > "$last"
    exit 1 ;;
  *) echo "[]" > "$last" ;;
esac
exit 0
"#;

/// A program whose interpreter is not there: starting it fails.
const GARBAGE: &str = "#!/nonexistent/interpreter\n";

/// A computer with a folder of programs, a cache, a folder for what `run`
/// writes and a repository, all without links in their paths.
struct Fixture {
    _dir: TempDir,
    base: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a directory");
        let base = fs::canonicalize(dir.path()).expect("the directory");
        for sub in ["bin", "cache", "tmp", "repo"] {
            fs::create_dir_all(base.join(sub)).expect("a folder");
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        for tool in ["cat", "tr", "grep"] {
            let found = find::search(tool, &path, false).expect("a tool of the system");
            std::os::unix::fs::symlink(found, base.join("bin").join(tool)).expect("a link");
        }
        Self { _dir: dir, base }
    }

    fn root(&self) -> PathBuf {
        self.base.join("repo")
    }

    fn file(&self, path: &str, content: &str) -> &Self {
        let file = self.root().join(path);
        fs::create_dir_all(file.parent().expect("a folder")).expect("a folder");
        fs::write(file, content).expect("a file");
        self
    }

    /// A program on the computer.
    fn computer(&self, name: &str, script: &str) -> &Self {
        executable(&self.base.join("bin").join(name), script);
        self
    }

    /// A pinned program, at this path below its version folder.
    fn pinned(&self, name: &str, version: &str, below: &str, script: &str) -> &Self {
        let dir = self.folder(name, version);
        executable(&dir.join(below), script);
        fs::write(dir.join(find::MARK), "").expect("the mark");
        self
    }

    fn folder(&self, name: &str, version: &str) -> PathBuf {
        self.base.join("cache/programs").join(name).join(version)
    }

    /// The programs of every check, on the computer, and those that are taken
    /// only pinned, pinned.
    fn everything(&self) -> &Self {
        self.computer("prettier", PRETTIER)
            .computer("jq", JQ)
            .computer("biome", BIOME)
            .computer("taplo", TAPLO)
            .computer("lychee", LYCHEE)
            .computer("node", NODE)
            .computer("actionlint", ACTIONLINT)
            .computer("check-jsonschema", JSONSCHEMA)
            .computer("dockerfmt", DOCKERFMT)
            .computer("hadolint", HADOLINT)
            .pinned(
                "html-validate",
                "11.21.1",
                "node_modules/.bin/html-validate",
                "#!/bin/sh\n",
            )
            .pinned(
                "markdownlint-cli2",
                "0.23.2",
                "node_modules/.bin/markdownlint-cli2",
                "#!/bin/sh\n",
            )
            .pinned(
                "stylelint",
                "18.0.0",
                "node_modules/.bin/stylelint",
                "#!/bin/sh\n",
            )
            .pinned("yamllint", "1.38.0", "bin/yamllint", "#!/bin/sh\n")
            .pinned("yamllint", "1.38.0", "bin/python", PYTHON)
    }

    fn env(&self) -> Environment {
        Environment {
            computer: "linux-x86_64".to_owned(),
            platform: Some(Platform::LinuxX86_64),
            windows: false,
            search_path: self.base.join("bin").into_os_string(),
            folder: Some(self.base.join("cache")),
            protocols: "=file".to_owned(),
            answers: None,
        }
    }

    fn run(&self, check: Check, files: &[&str], workflow: bool) -> (u8, String) {
        self.run_with(
            &Register::builtin().expect("the register"),
            check,
            files,
            workflow,
        )
    }

    fn run_with(
        &self,
        register: &Register,
        check: Check,
        files: &[&str],
        workflow: bool,
    ) -> (u8, String) {
        self.run_in(register, &self.base.join("tmp"), check, files, workflow)
    }

    fn run_in(
        &self,
        register: &Register,
        temp: &Path,
        check: Check,
        files: &[&str],
        workflow: bool,
    ) -> (u8, String) {
        let env = self.env();
        let root = self.root();
        let context = Context {
            register,
            env: &env,
            root: &root,
            temp,
            workflow,
        };
        let paths: Vec<RelativePath> = files
            .iter()
            .map(|file| RelativePath::new(file).expect("a path"))
            .collect();
        let refs: Vec<&RelativePath> = paths.iter().collect();
        let mut out = Vec::new();
        let code = run::run(&context, check, &RelativePath::root(), &refs, &mut out);
        (code, String::from_utf8(out).expect("UTF-8"))
    }
}

fn executable(file: &Path, script: &str) {
    fs::create_dir_all(file.parent().expect("a folder")).expect("a folder");
    fs::write(file, script).expect("a program");
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).expect("executable");
}

fn has(text: &str, line: &str) {
    assert!(
        text.lines().any(|found| found == line),
        "no line {line:?} in\n{text}"
    );
}

#[test]
fn json_files_parse_and_are_formatted() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    fixture
        .file("a.json", "{}\n")
        .file("b.jsonc", "{}\n")
        .file("Pipfile.lock", "{}\n");
    let (code, text) = fixture.run(Check::Json, &["Pipfile.lock", "a.json", "b.jsonc"], true);
    assert_eq!(code, 0, "{text}");
    has(&text, "3 JSON file(s) to check:");
    has(&text, "1 .jsonc file(s) parsed.");
    let root = fixture.root();
    let biome = text
        .lines()
        .find(|line| line.starts_with("biome "))
        .unwrap();
    assert!(biome.starts_with("biome lint --config-path="), "{biome}");
    assert!(biome.ends_with(" --reporter=github -- b.jsonc"), "{biome}");
    let prettier = text
        .lines()
        .find(|line| line.starts_with("prettier "))
        .unwrap();
    assert!(
        prettier.starts_with("prettier --check --no-color --with-node-modules --ignore-path "),
        "{prettier}"
    );
    assert!(
        prettier.ends_with(
            " --no-require-pragma --no-check-ignore-pragma --range-start=0 --range-end=Infinity \
             -- a.json b.jsonc"
        ),
        "{prettier}"
    );
    assert!(!text.contains(&root.display().to_string()), "{text}");
    // What `run` wrote for the programs is gone again.
    assert_eq!(fs::read_dir(fixture.base.join("tmp")).unwrap().count(), 0);
}

#[test]
fn every_kind_of_json_finding_is_reported() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    let files = [
        "broken.json",
        "quoted.jsonc",
        "two.json",
        "unformatted.json",
        "unreadable.geojson",
    ];
    for file in files {
        fixture.file(file, "x\n");
    }
    let (code, text) = fixture.run(Check::Json, &files, true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "::error file=broken.json::not valid JSON: jq: error: parse error: Expected another \
         key-value pair at line 1, column 22",
        "::error file=two.json::not valid JSON: the file holds 2 values, a JSON document is \
         exactly one",
        "::error title=parse,file=quoted.jsonc,line=1,endLine=1,col=3,endColumn=9::JSON standard \
         does not allow single quoted strings",
        "[warn] unformatted.json",
        "::error file=unformatted.json::not formatted the way Prettier writes it",
        "::error file=unreadable.geojson::Prettier cannot read it: SyntaxError: Unexpected token \
         (1:1)",
    ] {
        has(&text, line);
    }
    // Not a file: no finding on it.
    assert!(!text.contains("::error file=Code style"), "{text}");

    // Elsewhere a finding reads as a line that starts with its place.
    let (code, text) = fixture.run(Check::Json, &files, false);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "two.json: error: not valid JSON: the file holds 2 values, a JSON document is exactly one",
    );
    has(
        &text,
        "quoted.jsonc:1:3: error[parse]: JSON standard does not allow single quoted strings",
    );
}

#[test]
fn lockfiles_are_parsed_and_not_formatted() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    fixture.file("Pipfile.lock", "{}\n");
    let (code, text) = fixture.run(Check::Json, &["Pipfile.lock"], true);
    assert_eq!(code, 0, "{text}");
    has(&text, "No .jsonc file to parse.");
    has(
        &text,
        "No file to format-check: lockfiles are parsed, not formatted.",
    );
    assert!(!text.contains("prettier "), "{text}");
}

#[test]
fn nothing_to_check_is_said_and_passes() {
    let _alone = alone();
    let fixture = Fixture::new();
    let (code, text) = fixture.run(Check::Yaml, &[], true);
    assert_eq!(code, 0, "{text}");
    assert_eq!(text, "No YAML file below '.'; there is nothing to check.\n");
}

#[test]
fn a_missing_program_fails_the_check_and_names_setup() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.file("a.json", "{}\n");
    let (code, text) = fixture.run(Check::Json, &["a.json"], true);
    assert_eq!(code, 1, "{text}");
    for program in ["jq 1.8.1", "biome 2.5.15", "prettier 3.9.6"] {
        has(
            &text,
            &format!(
                "::error::{program} is neither pinned in the tool's folder nor on the computer; \
                 `nebensound-repo setup` installs it"
            ),
        );
    }

    fixture.file("a.toml", "a = 1\n");
    let (code, text) = fixture.run(Check::Toml, &["a.toml"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::taplo 0.10.0 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );

    // Two programs are taken pinned only, and one pinned with npm needs its
    // runtime.
    fixture
        .computer("markdownlint-cli2", "#!/bin/sh\n")
        .computer("lychee", LYCHEE)
        .pinned("prettier", "3.9.6", "node_modules/.bin/prettier", PRETTIER)
        .file("README.md", "# x\n");
    let (code, text) = fixture.run(Check::Markdown, &["README.md"], false);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "error: markdownlint-cli2 0.23.2 is not pinned in the tool's folder, and the check calls \
         the internals of the pinned copy only; `nebensound-repo setup --pinned` installs it",
    );
    has(
        &text,
        "error: prettier 3.9.6 needs node 14.0.0 or later, and there is none; `nebensound-repo \
         setup --with-runtimes` installs it",
    );
}

#[test]
fn a_pinned_program_on_npm_starts_with_its_runtime_in_front() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .pinned(
            "prettier",
            "3.9.6",
            "node_modules/.bin/prettier",
            "#!/bin/sh\necho \"PATH=$PATH\"\n",
        )
        .file("a.yaml", "a: 1\n");
    let (code, text) = fixture.run(Check::Yaml, &["a.yaml"], true);
    assert_eq!(code, 0, "{text}");
    let bin = fixture.base.join("bin").display().to_string();
    has(&text, &format!("PATH={bin}:{bin}"));
}

#[test]
fn toml_files_are_formatted_and_pass_taplo_lint_against_their_schema() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .file(
            ".nebensound/repo.toml",
            "#:schema https://x/s.json\n# show\n",
        )
        .file("Cargo.toml", "# A manifest, show it.\n\n[package]\n")
        .file(
            "own/Cargo.toml",
            "\n# show it.\n#:schema https://x/c.json\n",
        )
        .file("Cargo.lock", "show it\n")
        .file("settings.toml", "show = 1\n");
    let files = [
        ".nebensound/repo.toml",
        "Cargo.lock",
        "Cargo.toml",
        "own/Cargo.toml",
        "settings.toml",
    ];
    // Taplo's output is shown only with a finding, so the lint fails here.
    let (code, text) = fixture.run(Check::Toml, &files, true);
    assert_eq!(code, 1, "{text}");
    let lints: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("taplo lint"))
        .collect();
    assert_eq!(lints.len(), 5, "{text}");
    assert!(
        lints[0].contains(" --schema file://")
            && lints[0].contains("/schemas/nebensound-declaration.json "),
        "{text}"
    );
    assert!(!lints[1].contains("--schema"), "{text}");
    assert!(lints[2].contains("/schemas/cargo.json "), "{text}");
    assert!(!lints[3].contains("--schema"), "{text}");
    assert!(lints[0].ends_with(" --colors never -"), "{text}");
    // A lockfile is linted, not format-checked.
    fixture.file("Cargo.lock", "unformatted\n");
    let (code, text) = fixture.run(Check::Toml, &["Cargo.lock"], true);
    assert_eq!(code, 0, "{text}");
    fixture.file("settings.toml", "unformatted\n");
    let (code, text) = fixture.run(Check::Toml, &["settings.toml"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "taplo format --check --colors never --stdin-filepath settings.toml -",
    );
}

#[test]
fn every_kind_of_toml_finding_is_reported_with_taplos_diagnostic() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .file("unformatted.toml", "unformatted\n")
        .file("invalid.lock", "invalid\n");
    let (code, text) = fixture.run(Check::Toml, &["invalid.lock", "unformatted.toml"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=invalid.lock,line=2,col=5::fails taplo lint%0Aerror[x]: invalid \
         TOML%0Awarning: careful%0AWARN something",
    );
    has(
        &text,
        "::error file=unformatted.toml::does not parse, or is not formatted the way Taplo writes \
         it; 'taplo format' fixes the latter%0AERROR operation failed error=the input was not \
         properly formatted",
    );
    let (_, text) = fixture.run(Check::Toml, &["unformatted.toml"], false);
    has(
        &text,
        "unformatted.toml: error: does not parse, or is not formatted the way Taplo writes it; \
         'taplo format' fixes the latter",
    );
    has(
        &text,
        "ERROR operation failed error=the input was not properly formatted",
    );
}

#[test]
fn a_pipfile_is_formatted_and_linted_and_its_finding_names_the_standard_input_form() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .file("Pipfile", "show = 1\n")
        .file("a%b/Pipfile", "unformatted\n");
    // Linted like any TOML file. Taplo's output is shown only with a finding,
    // so the lint fails here.
    let (code, text) = fixture.run(Check::Toml, &["Pipfile"], true);
    assert_eq!(code, 1, "{text}");
    has(&text, "::error file=Pipfile::fails taplo lint");
    // No lockfile: a Pipfile is format-checked as well. `taplo format
    // a%b/Pipfile` would skip the file, so the hint names the standard-input
    // form; the path in it is escaped like the rest of the message.
    let (code, text) = fixture.run(Check::Toml, &["a%b/Pipfile"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "taplo format --check --colors never --stdin-filepath a%b/Pipfile -",
    );
    has(
        &text,
        "::error file=a%25b/Pipfile::does not parse, or is not formatted the way Taplo writes \
         it; 'taplo format --stdin-filepath a%25b/Pipfile - < a%25b/Pipfile' prints the fixed \
         file%0AERROR operation failed error=the input was not properly formatted",
    );
    let (_, text) = fixture.run(Check::Toml, &["a%b/Pipfile"], false);
    has(
        &text,
        "a%b/Pipfile: error: does not parse, or is not formatted the way Taplo writes it; \
         'taplo format --stdin-filepath a%b/Pipfile - < a%b/Pipfile' prints the fixed file",
    );
}

#[test]
fn a_file_that_cannot_be_read_is_a_finding() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything().file("locked.toml", "a = 1\n");
    let file = fixture.root().join("locked.toml");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&file).is_ok() {
        // Whoever can read every file reads this one too.
        return;
    }
    let (code, text) = fixture.run(Check::Toml, &["locked.toml"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error file=locked.toml::cannot be read: "),
        "{text}"
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();

    fixture
        .file("lychee.toml", "x = 1\n")
        .file("README.md", "# x\n");
    let config = fixture.root().join("lychee.toml");
    fs::set_permissions(&config, fs::Permissions::from_mode(0o000)).unwrap();
    let (code, text) = fixture.run(Check::Markdown, &["README.md"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error file=lychee.toml::cannot be read: "),
        "{text}"
    );
}

#[test]
fn markdown_files_are_formatted_follow_the_rules_and_link_somewhere() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .file("README.md", "# x\n")
        .file("lychee.toml", "max_retries = 1\n");
    let (code, text) = fixture.run(Check::Markdown, &["README.md"], true);
    assert_eq!(code, 0, "{text}");
    let folder = fixture.folder("markdownlint-cli2", "0.23.2");
    has(
        &text,
        &format!("node --input-type=module --eval TOOLS={}", folder.display()),
    );
    has(&text, "Rules: the pipeline default");
    has(&text, "config: max_retries = 1");
    has(&text, "lines: README.md");
    let lychee = text
        .lines()
        .find(|line| line.starts_with("lychee "))
        .unwrap();
    assert!(
        lychee.starts_with(
            "lychee --offline --include-fragments --no-progress --hidden --no-ignore --config "
        ),
        "{lychee}"
    );
    assert!(
        lychee.contains(&format!(
            " --root-dir {} --files-from ",
            fixture.root().display()
        )),
        "{lychee}"
    );
    assert!(lychee.contains(" --format markdown --output "), "{lychee}");
}

#[test]
fn every_kind_of_markdown_finding_is_reported() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    let files = [
        "README-broken.md",
        "heading.md",
        "style.md",
        "unformatted.md",
    ];
    for file in files {
        fixture.file(file, "# x\n");
    }
    // A linked configuration counts as none.
    std::os::unix::fs::symlink("elsewhere.toml", fixture.root().join("lychee.toml")).unwrap();
    let (code, text) = fixture.run(Check::Markdown, &files, true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "::error file=unformatted.md::not formatted the way Prettier writes it",
        "heading.md:3 error MD001/heading-increment Heading levels should only increment by one \
         level at a time",
        "::error file=heading.md,line=3::MD001/heading-increment Heading levels should only \
         increment by one level at a time",
        "::error file=style.md,line=1,col=1::MD003/heading-style Heading style",
        "* [ERROR] <file:///x/missing.md> (at 3:11) | File not found",
        "::error file=README-broken.md,line=3,col=11::[ERROR] <file:///x/missing.md> | File not \
         found",
        "::error file=README-broken.md::[ERROR] <file:///x/other.md> | File not found",
        "config: ",
    ] {
        has(&text, line);
    }
    // A redirect is no broken link.
    assert!(!text.contains("::error file=README.md"), "{text}");
}

#[test]
fn yaml_files_are_formatted_and_pass_their_linters() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .computer("shellcheck", "#!/bin/sh\n")
        .file(".yamllint.yaml", "rules: {}\n")
        .file(".github/workflows/ci.yml", "on: push\n")
        .file(".github/dependabot.yml", "version: 2\n")
        .file("config.yaml", "a: 1\n");
    let files = [
        ".github/dependabot.yml",
        ".github/workflows/ci.yml",
        "config.yaml",
    ];
    let (code, text) = fixture.run(Check::Yaml, &files, true);
    assert_eq!(code, 0, "{text}");
    has(&text, "1 of them workflow(s).");
    has(&text, "Dependabot configuration: .github/dependabot.yml");
    has(
        &text,
        "Rules: the pipeline default, and laid over it .yamllint.yaml",
    );
    has(&text, "python -c OWN=.yamllint.yaml");
    let bin = fixture.base.join("bin");
    has(
        &text,
        &format!(
            "ShellCheck: {}, so actionlint reads every `run:` script with it",
            bin.join("shellcheck").display()
        ),
    );
    has(
        &text,
        "pyflakes is not on the search path, so actionlint reads no Python `run:` script with it",
    );
    has(
        &fs::read_to_string(bin.join("actionlint.args")).unwrap(),
        "actionlint -ignore ^specifying action \"\\$/[^\"]+\" in invalid format because ref is \
         missing -ignore ^reusable workflow call \"\\$/[^\"]+\" at \"uses\" is not following the \
         format -format {{json .}} -- .github/workflows/ci.yml",
    );
    has(
        &text,
        "check-jsonschema --builtin-schema vendor.dependabot --color never -- \
         .github/dependabot.yml",
    );
}

#[test]
fn every_kind_of_yaml_finding_is_reported() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .file(".github/workflows/broken.yaml", "on: push\n")
        .file(".github/dependabot.yaml", "schedules: x\n")
        .file("duplicate.yaml", "a: 1\n")
        .file("unformatted.yml", "a: 1\n");
    let files = [
        ".github/dependabot.yaml",
        ".github/workflows/broken.yaml",
        "duplicate.yaml",
        "unformatted.yml",
    ];
    let (code, text) = fixture.run(Check::Yaml, &files, true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "::error file=unformatted.yml::not formatted the way Prettier writes it",
        "::group::duplicate.yaml",
        "::error file=duplicate.yaml,line=2,col=1::2:1 [key-duplicates] duplication of key \
         \"name\" in mapping",
        "::error file=.github/workflows/broken.yaml,line=10,col=3::\"runs-on\" section is missing \
         in job \"test\" [syntax-check]",
        "::error file=.github/dependabot.yaml::$.updates[0]: Additional properties are not \
         allowed ('schedules' was unexpected)",
        "Rules: the pipeline default",
        "ShellCheck is not on the search path, so actionlint reads no `run:` script with it",
    ] {
        has(&text, line);
    }
    assert!(!text.contains("not this file\n::error"), "{text}");

    // Elsewhere a group is its name, and its end nothing.
    let (_, text) = fixture.run(Check::Yaml, &["duplicate.yaml"], false);
    has(&text, "duplicate.yaml");
    has(
        &text,
        "duplicate.yaml:2:1: error: 2:1 [key-duplicates] duplication of key \"name\" in mapping",
    );
    assert!(!text.contains("endgroup"), "{text}");
}

#[test]
fn programs_only_some_files_need_are_looked_for_only_then() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .computer("prettier", PRETTIER)
        .pinned("yamllint", "1.38.0", "bin/yamllint", "#!/bin/sh\n")
        .pinned("yamllint", "1.38.0", "bin/python", PYTHON)
        .file("a.yml", "a: 1\n")
        .file(".github/workflows/ci.yml", "on: push\n")
        .file(".github/dependabot.yml", "version: 2\n");
    let (code, text) = fixture.run(Check::Yaml, &["a.yml"], true);
    assert_eq!(code, 0, "{text}");
    let (code, text) = fixture.run(
        Check::Yaml,
        &[".github/dependabot.yml", ".github/workflows/ci.yml"],
        true,
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::actionlint 1.7.12 is neither"),
        "{text}"
    );
    assert!(
        text.contains("::error::check-jsonschema 0.39.0 is neither"),
        "{text}"
    );
}

#[test]
fn a_program_that_cannot_be_started_fails_the_check() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .computer("jq", GARBAGE)
        .computer("biome", GARBAGE)
        .file("a.json", "{}\n")
        .file("b.jsonc", "{}\n");
    let (code, text) = fixture.run(Check::Json, &["a.json", "b.jsonc"], true);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("::error::jq cannot be started: "), "{text}");
    assert!(
        text.contains("::error::Biome cannot be started: "),
        "{text}"
    );
}

#[test]
fn without_a_folder_for_the_programs_nothing_runs() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    let file = fixture.base.join("a file");
    fs::write(&file, "").unwrap();
    let register = Register::builtin().unwrap();
    for (check, name) in [
        (Check::Css, "a.css"),
        (Check::Dockerfile, "Dockerfile"),
        (Check::Html, "a.html"),
        (Check::Json, "a.json"),
        (Check::Markdown, "a.md"),
        (Check::Toml, "a.toml"),
        (Check::Yaml, "a.yml"),
    ] {
        fixture.file(name, "x\n");
        let (code, text) = fixture.run_in(&register, &file, check, &[name], true);
        assert_eq!(code, 1, "{text}");
        assert!(
            text.contains("::error::no folder can be made in "),
            "{text}"
        );
    }
}

#[test]
fn markdownlint_needs_a_runtime_from_the_register() {
    let _alone = alone();
    let fixture = Fixture::new();
    let source = nebensound_repo::register::BUILTIN_REGISTER;
    let start = source.find("[program.markdownlint-cli2]").unwrap();
    let end = source.find("[program.prettier]").unwrap();
    let native = format!(
        "{}[program.markdownlint-cli2]\nversion = \"0.23.2\"\ncommand = \"markdownlint-cli2\"\n\n\
         [program.markdownlint-cli2.download.linux-x86_64]\nurl = \"https://example.invalid/m\"\n\
         digest = \"sha256:{}\"\nexecutable = \"markdownlint-cli2\"\n\n{}",
        &source[..start],
        "0".repeat(64),
        &source[end..]
    );
    let register = Register::parse(&native).unwrap();
    fixture
        .everything()
        .pinned(
            "markdownlint-cli2",
            "0.23.2",
            "markdownlint-cli2",
            "#!/bin/sh\n",
        )
        .file("README.md", "# x\n");
    let (code, text) = fixture.run_with(&register, Check::Markdown, &["README.md"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::markdownlint-cli2 runs on Node.js, and the register gives it no runtime to run \
         on",
    );
}

#[test]
fn workflow_commands_read_plainly_elsewhere() {
    for (line, expected) in [
        ("plain text", Some("plain text")),
        ("::endgroup::", None),
        ("::group::a%3Ab", Some("a:b")),
        ("::error::alone", Some("error: alone")),
        (
            "::warning file=a%2Cb.md,line=3,col=4,title=T%3Ax::one%0Atwo %25",
            Some("a,b.md:3:4: warning[T:x]: one\ntwo %"),
        ),
        ("::notice file=a.md,odd::x", Some("a.md: notice: x")),
        ("::notice file=a.md,odd=1::x", Some("a.md: notice: x")),
        ("::errors::x", Some("::errors::x")),
        ("::error x", Some("::error x")),
        ("::error file=a:b::x", Some("::error file=a:b::x")),
        ("::debug::x", Some("::debug::x")),
    ] {
        assert_eq!(run::plain(line).as_deref(), expected, "{line:?}");
    }
    assert_eq!(
        run::annotation(Some("a,b:c%.md"), Some(("3", None)), "one\ntwo\r%"),
        "::error file=a%2Cb%3Ac%25.md,line=3::one%0Atwo%0D%25"
    );
    assert_eq!(run::annotation(None, None, "x"), "::error::x");
}

#[test]
fn helpers_take_paths_and_lists_apart() {
    assert_eq!(
        run::relative(Path::new("/a/b/c"), Path::new("/a/d")),
        PathBuf::from("../../d")
    );
    let long = "x".repeat(10_000);
    let files = vec![long.as_str(); 7];
    let chunks = run::chunks(&files);
    assert_eq!(
        chunks.iter().map(Vec::len).collect::<Vec<_>>(),
        [2, 2, 2, 1]
    );
    let huge = "y".repeat(run::ARGUMENTS + 1);
    assert_eq!(run::chunks(&[huge.as_str(), "z"]).len(), 2);
    assert_eq!(Check::of("json"), Some(Check::Json));
    assert_eq!(Check::of("rust"), None);
    for (check, name) in [
        (Check::Css, "css"),
        (Check::Dockerfile, "dockerfile"),
        (Check::Html, "html"),
        (Check::Json, "json"),
        (Check::Markdown, "markdown"),
        (Check::Toml, "toml"),
        (Check::Yaml, "yaml"),
    ] {
        assert_eq!(check.name(), name);
        assert_eq!(Check::of(name), Some(check));
    }
}

#[test]
fn the_link_check_is_handed_every_path_as_lychee_reads_it() {
    for (file, line) in [
        ("docs/a.md", "docs/a.md"),
        ("[docs]/a*?{}.md", "[[]docs[]]/a[*][?][{][}].md"),
        ("#a.md", "[#]a.md"),
        (" a.md ", "[ ]a.md[ ]"),
        ("\ta.md\u{b}", "[\t]a.md[\u{b}]"),
        ("mailto:a.md", "[m]ailto:a.md"),
        ("dir/mailto:a.md", "dir/mailto:a.md"),
        ("1:a.md", "1:a.md"),
        ("é:a.md", "é:a.md"),
    ] {
        assert_eq!(markdown::lychee_line(file), line, "{file:?}");
    }
    assert_eq!(
        markdown::lychee_lines(&["a.md".to_owned(), "b c.md".to_owned()]),
        "a.md\nb c.md\n"
    );
}

#[test]
fn findings_of_markdownlint_and_lychee_are_taken_apart() {
    for (line, expected) in [
        ("a.md:3 error MD001 x", Some(("a.md", "3", None, "MD001 x"))),
        (
            "a.md:3:7 error MD001 x",
            Some(("a.md", "3", Some("7"), "MD001 x")),
        ),
        (
            "a:1.md:3 error MD001 x",
            Some(("a:1.md", "3", None, "MD001 x")),
        ),
        ("a.md:3:x error MD001 x", None),
        ("a.md:3 error ", None),
        (":3 error x", None),
        ("a.md:x error y", None),
        ("a.md:3 warning x", None),
        ("no place at all", None),
    ] {
        assert_eq!(markdown::markdownlint_finding(line), expected, "{line:?}");
    }
    let found = markdown::broken(
        "# Summary\n### Errors in a.md\n* [ERROR] <u> (at 1:2) | gone\n## Other\n* [ERROR] <v>\n\
         ### Timeouts in b.md\n* [TIMEOUT] <w> (at x:2)\n* [TIMEOUT] <w> (at 2)\n* [404] <w> (at 3:4\n",
    );
    let link = |file: &str, place: Option<(&str, &str)>, message: &str| markdown::Broken {
        file: file.to_owned(),
        place: place.map(|(row, column)| (row.to_owned(), column.to_owned())),
        message: message.to_owned(),
    };
    assert_eq!(
        found,
        [
            link("a.md", Some(("1", "2")), "[ERROR] <u> | gone"),
            link("b.md", None, "[TIMEOUT] <w> (at x:2)"),
            link("b.md", None, "[TIMEOUT] <w> (at 2)"),
            link("b.md", None, "[404] <w> (at 3:4"),
        ]
    );
}

#[test]
fn schema_directives_and_locations_are_read_as_taplo_reads_them() {
    for (text, expected) in [
        ("#:schema https://x\n[a]\n", true),
        ("\n  \r\n# c\n\t#:schema\thttps://x\n", true),
        ("#:schema\r\n", true),
        ("#:schema\n", false),
        ("#:schemas x\n", false),
        ("[a]\n#:schema https://x\n", false),
        ("", false),
    ] {
        assert_eq!(toml::directive(text), expected, "{text:?}");
    }
    assert_eq!(
        toml::with_id("{\n  \"a\": 1\n}\n", "file:///s/a.json"),
        "{\"$id\": \"file:///s/a.json\",\n  \"a\": 1\n}\n"
    );
    assert_eq!(toml::with_id("[]", "file:///x"), "[]");
    for (name, schema) in toml::SCHEMAS {
        let value: serde_json::Value =
            serde_json::from_str(&toml::with_id(schema, "file:///x")).unwrap();
        assert_eq!(value["$id"], "file:///x", "{name}");
    }
    assert!(!run::json::BIOME.is_empty());
}

#[test]
fn workflows_and_schema_findings_are_recognized() {
    for (file, expected) in [
        (".github/workflows/ci.yml", true),
        (".github/workflows/ci.yaml", true),
        (".github/workflows/.yml", false),
        (".github/workflows/a/ci.yml", false),
        ("workflows/ci.yml", false),
        (".github/workflows/ci.json", false),
    ] {
        assert_eq!(yaml::is_workflow(file), expected, "{file:?}");
    }
    let file = ".github/dependabot.yml";
    for (line, expected) in [
        (
            "  .github/dependabot.yml::$.a: wrong",
            Some(("$.a", "wrong")),
        ),
        (".github/dependabot.yml::$.a: wrong", None),
        ("  other.yml::$.a: wrong", None),
        ("  .github/dependabot.yml:$.a: wrong", None),
        ("  .github/dependabot.yml::$.a wrong", None),
        ("  .github/dependabot.yml::$.a:wrong", None),
        ("  .github/dependabot.yml::a: wrong", None),
        ("  .github/dependabot.yml::$.a: ", None),
    ] {
        assert_eq!(yaml::schema_finding(line, file), expected, "{line:?}");
    }
}

#[test]
fn the_call_refuses_what_it_does_not_run() {
    let _alone = alone();
    let (text, code) = call(&["nebensound-repo", "run", "rust", "."]);
    assert_eq!(code, 2, "{text}");
    assert!(
        text.contains(
            "`run` carries out the checks css, dockerfile, html, json, markdown, toml and yaml; \
             \"rust\" is not"
        ),
        "{text}"
    );
    let (text, code) = call(&["nebensound-repo", "run", "json", "/abs"]);
    assert_eq!(code, 2, "{text}");

    let broken = support::repository(&[
        (".nebensound/repo.toml", QUIET),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    let root = broken.path().to_str().unwrap();
    let (text, code) = call(&["nebensound-repo", "-C", root, "run", "json", "."]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        "nothing is checked: phase 0 found errors, and `check` says which\n"
    );

    let fine = support::repository(&[(".nebensound/repo.toml", QUIET)]);
    let root = fine.path().to_str().unwrap();
    let (text, code) = call(&["nebensound-repo", "-C", root, "run", "json", "docs"]);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("the plan does not start the json check at docs"),
        "{text}"
    );
    let (text, code) = call(&["nebensound-repo", "-C", root, "run", "json", "."]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(text, "No JSON file below '.'; there is nothing to check.\n");
}

#[test]
fn the_binary_runs_a_check_with_the_programs_of_the_computer() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .computer("taplo", TAPLO)
        .file(".nebensound/repo.toml", QUIET)
        .file("unformatted.toml", "unformatted\n");
    support::git(&fixture.root(), &["init", "--quiet"]);
    support::git(&fixture.root(), &["add", "--all"]);
    let git = find::search("git", &std::env::var_os("PATH").unwrap_or_default(), false).unwrap();
    std::os::unix::fs::symlink(git, fixture.base.join("bin/git")).unwrap();
    fs::create_dir_all(fixture.base.join("home")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nebensound-repo"))
        .args(["run", "toml", "--", "."])
        .current_dir(fixture.root())
        .env("PATH", fixture.base.join("bin"))
        .env("HOME", fixture.base.join("home"))
        .env("TMPDIR", fixture.base.join("tmp"))
        .env("GITHUB_ACTIONS", "true")
        .env_remove("XDG_CACHE_HOME")
        .output()
        .expect("the built binary");
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{text}");
    has(&text, "2 TOML file(s) to check:");
    assert!(
        text.contains("::error file=unformatted.toml::does not parse"),
        "{text}"
    );
    assert_eq!(fs::read_dir(fixture.base.join("tmp")).unwrap().count(), 0);
}

#[test]
fn setup_takes_the_checks_it_is_named() {
    let _alone = alone();
    let register = Register::builtin().unwrap();
    let names =
        |names: &[&str]| -> Vec<String> { names.iter().map(|name| (*name).to_owned()).collect() };
    let checks =
        nebensound_repo::setup::named(&register, &names(&["yaml", "json", "yaml"])).unwrap();
    let shown: Vec<String> = checks.iter().map(ToString::to_string).collect();
    assert_eq!(shown, ["json", "yaml"]);
    assert_eq!(
        nebensound_repo::setup::named(&register, &names(&["json", "nonsense"])),
        Err("nonsense".to_owned())
    );

    let repo = support::repository(&[(".nebensound/repo.toml", QUIET)]);
    let root = repo.path().to_str().unwrap();
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        root,
        "setup",
        "--check",
        "nonsense",
    ]);
    assert_eq!(code, 2, "{text}");
    assert_eq!(text, "the register has no check \"nonsense\"\n");
    let (text, _) = call(&["nebensound-repo", "-C", root, "setup", "--check", "toml"]);
    assert!(text.starts_with("Checks: toml\n"), "{text}");
}

#[test]
fn a_missing_program_leaves_the_other_parts_of_its_check_running() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    for program in ["jq", "lychee", "actionlint"] {
        fs::remove_file(fixture.base.join("bin").join(program)).unwrap();
    }
    fixture
        .file("unformatted.json", "{}\n")
        .file("heading.md", "# x\n")
        .file(".github/workflows/ci.yml", "on: push\n")
        .file("duplicate.yaml", "a: 1\n");

    let (code, text) = fixture.run(Check::Json, &["unformatted.json"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::jq 1.8.1 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    has(
        &text,
        "::error file=unformatted.json::not formatted the way Prettier writes it",
    );

    let (code, text) = fixture.run(Check::Markdown, &["heading.md"], true);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("::error::lychee "), "{text}");
    has(
        &text,
        "::error file=heading.md,line=3::MD001/heading-increment Heading levels should only \
         increment by one level at a time",
    );
    assert!(!text.contains("lychee --"), "{text}");

    let files = [".github/workflows/ci.yml", "duplicate.yaml"];
    let (code, text) = fixture.run(Check::Yaml, &files, true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::actionlint 1.7.12 is neither"),
        "{text}"
    );
    has(
        &text,
        "::error file=duplicate.yaml,line=2,col=1::2:1 [key-duplicates] duplication of key \
         \"name\" in mapping",
    );
}

#[test]
fn actionlint_findings_are_escaped_and_other_output_is_kept() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .file(".github/workflows/a,broken.yml", "on: push\n")
        .file(".github/workflows/unreadable.yml", "on: push\n");
    let (code, text) = fixture.run(Check::Yaml, &[".github/workflows/a,broken.yml"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=.github/workflows/a%2Cbroken.yml,line=10,col=3::\"runs-on\" section is \
         missing in job \"test\" [syntax-check]",
    );
    let (_, text) = fixture.run(Check::Yaml, &[".github/workflows/a,broken.yml"], false);
    has(
        &text,
        ".github/workflows/a,broken.yml:10:3: error: \"runs-on\" section is missing in job \
         \"test\" [syntax-check]",
    );
    let (code, text) = fixture.run(Check::Yaml, &[".github/workflows/unreadable.yml"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "could not read \".github/workflows/unreadable.yml\": permission denied",
    );
}

#[test]
fn actionlint_json_is_read_list_by_list() {
    let finding = |file: &str| yaml::ActionlintFinding {
        message: "m".to_owned(),
        filepath: file.to_owned(),
        line: 1,
        column: 2,
        kind: "k".to_owned(),
    };
    let one =
        r#"[{"message":"m","filepath":"a.yml","line":1,"column":2,"kind":"k","end_column":3}]"#;
    let two = r#"[{"message":"m","filepath":"b.yml","line":1,"column":2,"kind":"k"}]"#;
    assert_eq!(
        yaml::actionlint_findings(&format!("{one}\n[]\n{two}\n")),
        Some(vec![finding("a.yml"), finding("b.yml")])
    );
    assert_eq!(yaml::actionlint_findings(""), Some(Vec::new()));
    assert_eq!(yaml::actionlint_findings("[]\nnot json\n"), None);
    assert_eq!(yaml::actionlint_findings(r#"[{"message":"m"}]"#), None);
}

#[test]
fn dockerfiles_are_formatted_and_pass_hadolint() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    fixture
        .file("Dockerfile", "FROM scratch\n")
        .file("images/app.dockerfile", "FROM scratch\n");
    let files = ["Dockerfile", "images/app.dockerfile"];
    let (code, text) = fixture.run(Check::Dockerfile, &files, true);
    assert_eq!(code, 0, "{text}");
    for line in [
        "2 Dockerfile(s) to check:",
        "dockerfmt --check -- Dockerfile",
        "dockerfmt --check -- images/app.dockerfile",
        "Rules: Hadolint's own",
        "Failure threshold: info",
        "hadolint about images/app.dockerfile",
    ] {
        has(&text, line);
    }
    // The empty configuration, from the folder of what the programs are
    // handed, which is gone again; what the configuration printed on its
    // standard output is not shown.
    assert!(
        text.contains("hadolint --config ") && text.contains("/hadolint.yaml with "),
        "{text}"
    );
    assert!(!text.contains("never shown"), "{text}");
    assert_eq!(fs::read_dir(fixture.base.join("tmp")).unwrap().count(), 0);

    let (code, text) = fixture.run(Check::Dockerfile, &[], true);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text,
        "No Dockerfile below '.'; there is nothing to check.\n"
    );
}

#[test]
fn every_kind_of_dockerfmt_finding_is_reported() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    let files = [
        "unformatted.dockerfile",
        "left.dockerfile",
        "unreadable.dockerfile",
        "warned.dockerfile",
        "crashing.dockerfile",
        "killed.dockerfile",
    ];
    for file in files {
        fixture.file(file, "FROM scratch\n  run\techo hi\n");
    }
    let (code, text) = fixture.run(Check::Dockerfile, &files, true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "unformatted.dockerfile is not formatted",
        "::error file=unformatted.dockerfile::not formatted the way dockerfmt writes it; \
         'dockerfmt --write' fixes it",
        "left.dockerfile:2: left unformatted: BuildKit would read it differently",
        "::error file=left.dockerfile,line=2::dockerfmt left the RUN instruction on this line \
         unformatted (BuildKit would read it differently), so it was not checked; write it so \
         that dockerfmt can format it, or put '# dockerfmt-ignore' on the line before it to \
         leave it as written on purpose",
        "::error file=left.dockerfile,line=9::dockerfmt left the instruction on this line \
         unformatted (beyond the end), so it was not checked; write it so that dockerfmt can \
         format it, or put '# dockerfmt-ignore' on the line before it to leave it as written on \
         purpose",
        "::error file=left.dockerfile::left.dockerfile:x: left unformatted: no number",
        "::error file=unreadable.dockerfile,line=3::dockerfmt cannot read it: unknown \
         instruction: FORM",
        "::error file=unreadable.dockerfile::dockerfmt cannot read it: no such thing",
        "::warning file=warned.dockerfile::dockerfmt is indent_size 3 is not used",
        "::error file=crashing.dockerfile::dockerfmt failed on it with exit status 3; its output \
         stands in the log",
        "::error file=killed.dockerfile::dockerfmt failed on it (signal: 9 (SIGKILL)); its \
         output stands in the log",
    ] {
        has(&text, line);
    }
    // Another file's name is no finding on this one.
    assert!(!text.contains("::error file=other"), "{text}");

    // A warning alone fails nothing; elsewhere it reads as a line.
    let (code, text) = fixture.run(Check::Dockerfile, &["warned.dockerfile"], false);
    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "warned.dockerfile: warning: dockerfmt is indent_size 3 is not used",
    );

    // A dockerfmt that cannot be started.
    fixture.computer("dockerfmt", GARBAGE);
    let (code, text) = fixture.run(Check::Dockerfile, &["warned.dockerfile"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::dockerfmt cannot be started: "),
        "{text}"
    );
}

#[test]
fn hadolint_findings_are_judged_against_the_threshold() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    fixture.file("latest.dockerfile", "FROM x:latest\n");
    let (code, text) = fixture.run(Check::Dockerfile, &["latest.dockerfile"], true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "::error file=latest.dockerfile,line=1,col=1::DL3007 (warning): Using latest is prone to \
         errors",
        "::error file=latest.dockerfile,line=2,col=1::DL3059 (info): Multiple consecutive RUN",
        "::warning file=latest.dockerfile,line=3,col=5::SC1001 (style): odd%25%0Aline",
        "::warning file=latest.dockerfile,line=null,col=null::X (null): null",
        "hadolint about latest.dockerfile",
    ] {
        has(&text, line);
    }

    // The repository's configuration, at the root, with a threshold that
    // leaves every finding a warning.
    fixture.file(".hadolint.yml", "threshold error\n");
    let (code, text) = fixture.run(Check::Dockerfile, &["latest.dockerfile"], true);
    assert_eq!(code, 0, "{text}");
    has(&text, "Rules: Hadolint's own, with .hadolint.yml");
    has(&text, "Failure threshold: error");
    has(
        &text,
        "::warning file=latest.dockerfile,line=1,col=1::DL3007 (warning): Using latest is prone \
         to errors",
    );
    assert!(
        text.contains("hadolint --config .hadolint.yml with "),
        "{text}"
    );

    // `.hadolint.yaml` comes first. `ignore` and `no-fail` are set aside, and
    // every output format the configuration names goes into a file of its
    // own before the report.
    fixture.file(".hadolint.yaml", "ignore nofail formats\n");
    let (code, text) = fixture.run(Check::Dockerfile, &["latest.dockerfile"], true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "Rules: Hadolint's own, with .hadolint.yaml",
        "A failure threshold of none or ignore is set aside; Hadolint's default, info, holds.",
        "no-fail is set aside: a finding at or above the threshold fails the check.",
        "Failure threshold: info",
        "outputs: discarded-0 discarded-1 report",
    ] {
        has(&text, line);
    }
}

#[test]
fn a_hadolint_that_says_too_little_fails_the_check() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    for file in [
        "noreport.dockerfile",
        "garbage.dockerfile",
        "object.dockerfile",
    ] {
        fixture.file(file, "FROM scratch\n");
        let (code, text) = fixture.run(Check::Dockerfile, &[file], true);
        assert_eq!(code, 1, "{text}");
        has(
            &text,
            &format!(
                "::error file={file}::Hadolint gave no report for it; its output stands in the log"
            ),
        );
    }

    fixture.file(".hadolint.yaml", "broken\n");
    let (code, text) = fixture.run(Check::Dockerfile, &["object.dockerfile"], true);
    assert_eq!(code, 1, "{text}");
    has(&text, "\"Error parsing your config file at line 1\"");
    has(
        &text,
        "::error file=.hadolint.yaml::Hadolint cannot read this configuration and would run \
         without it; its message stands in the log",
    );
    assert!(!text.contains("hadolint about"), "{text}");

    fixture.file(".hadolint.yaml", "silent\n");
    let (code, text) = fixture.run(Check::Dockerfile, &["object.dockerfile"], true);
    assert_eq!(code, 1, "{text}");
    has(&text, "Configuration:");
    has(
        &text,
        "::error::Hadolint printed no failure threshold for .hadolint.yaml; the configuration it \
         runs with is not known",
    );

    // A Hadolint that cannot be started, and one that cannot be started a
    // second time, after it printed its configuration.
    fixture.computer("hadolint", GARBAGE);
    let (code, text) = fixture.run(Check::Dockerfile, &["object.dockerfile"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::Hadolint cannot be started: "),
        "{text}"
    );
    fixture.computer(
        "hadolint",
        "#!/bin/sh\necho '  failure threshold: info' >&2\nprintf '#!/nonexistent/x\\n' > \"$0\"\n",
    );
    let (code, text) = fixture.run(Check::Dockerfile, &["object.dockerfile"], true);
    assert_eq!(code, 1, "{text}");
    has(&text, "Failure threshold: info");
    assert!(
        text.contains("::error::Hadolint cannot be started: "),
        "{text}"
    );
}

#[test]
fn without_its_programs_the_dockerfile_check_fails() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.file("unformatted.dockerfile", "FROM scratch\n");
    fixture.computer("dockerfmt", DOCKERFMT);
    let (code, text) = fixture.run(Check::Dockerfile, &["unformatted.dockerfile"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::hadolint 2.15.1 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    // The other part still runs.
    has(
        &text,
        "::error file=unformatted.dockerfile::not formatted the way dockerfmt writes it; \
         'dockerfmt --write' fixes it",
    );
    fs::remove_file(fixture.base.join("bin/dockerfmt")).unwrap();
    fixture.computer("hadolint", HADOLINT);
    let (code, text) = fixture.run(Check::Dockerfile, &["unformatted.dockerfile"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::dockerfmt 1.0.0 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    has(&text, "Failure threshold: info");
}

#[test]
fn what_dockerfmt_prints_is_taken_apart() {
    let file = "a,b.dockerfile";
    for (line, expected) in [
        (
            "a,b.dockerfile is not formatted",
            Some(dockerfile::Said::Unformatted),
        ),
        ("a,b.dockerfile is not formatted!", None),
        ("x a,b.dockerfile is not formatted", None),
    ] {
        assert_eq!(dockerfile::said_out(line, file), expected, "{line:?}");
    }
    let left = |line: Option<&str>, reason: &str| dockerfile::Said::LeftUnformatted {
        line: line.map(str::to_owned),
        reason: reason.to_owned(),
    };
    let unreadable = |line: Option<&str>, detail: &str| dockerfile::Said::Unreadable {
        line: line.map(str::to_owned),
        detail: detail.to_owned(),
    };
    for (line, expected) in [
        (
            "a,b.dockerfile:12: left unformatted: why",
            Some(left(Some("12"), "why")),
        ),
        (
            "a,b.dockerfile:: left unformatted: why",
            Some(left(None, "why")),
        ),
        (
            "a,b.dockerfile:1x: left unformatted: why",
            Some(left(None, "why")),
        ),
        (
            "a,b.dockerfile:3:4: left unformatted: a: left unformatted: b",
            Some(left(Some("3"), "a: left unformatted: b")),
        ),
        ("a,b.dockerfile: left unformatted: why", None),
        ("a,b.dockerfile:3 left unformatted: why", None),
        (
            "dockerfmt: a,b.dockerfile: line 7: bad",
            Some(unreadable(Some("7"), "bad")),
        ),
        (
            "dockerfmt: a,b.dockerfile: line x: bad",
            Some(unreadable(None, "line x: bad")),
        ),
        (
            "dockerfmt: a,b.dockerfile: line : bad",
            Some(unreadable(None, "line : bad")),
        ),
        ("dockerfmt: a,b.dockerfile:bad", None),
        (
            "dockerfmt: warning: careful",
            Some(dockerfile::Said::Warning("careful".to_owned())),
        ),
        ("dockerfmt: other: bad", None),
        ("anything else", None),
    ] {
        assert_eq!(dockerfile::said_err(line, file), expected, "{line:?}");
    }
    let text = "FROM scratch\n\n \t run  echo\nexpose 80";
    for (number, expected) in [
        ("1", Some("FROM")),
        ("2", None),
        ("3", Some("RUN")),
        ("04", Some("EXPOSE")),
        ("5", None),
        ("0", None),
        ("", None),
        ("x", None),
    ] {
        assert_eq!(
            dockerfile::instruction(text, number).as_deref(),
            expected,
            "{number:?}"
        );
    }
}

#[test]
fn what_hadolint_prints_with_verbose_is_read_as_it_reads_it() {
    let settings = dockerfile::settings(
        "Configuration:\n  no fail: True\n  failure threshold: style\n  output format:\n    - \
         tty\n  output format:\n    - json\n  verbose: True\n  output format:\n    - sarif\n",
    );
    assert_eq!(
        settings,
        dockerfile::Settings {
            unreadable: false,
            threshold: Some("style".to_owned()),
            no_fail: true,
            formats: 2,
        }
    );
    let settings = dockerfile::settings(
        "\"Error parsing your config file\n  failure threshold: \n  failure threshold: \
         Info\n  no fail: true\n  output format:\n    - tty",
    );
    assert_eq!(
        settings,
        dockerfile::Settings {
            unreadable: true,
            threshold: None,
            no_fail: false,
            formats: 1,
        }
    );
    for (level, rank) in [
        ("error", 1),
        ("warning", 2),
        ("info", 3),
        ("style", 4),
        ("ignore", 5),
        ("", 5),
    ] {
        assert_eq!(dockerfile::rank(level), rank, "{level:?}");
    }
    assert_eq!(
        dockerfile::CONFIGURATIONS,
        [".hadolint.yaml", ".hadolint.yml"]
    );
    assert_eq!(
        run::annotation_at(Level::Warning, Some("a,b"), Some(("1", Some("2"))), "m%"),
        "::warning file=a%2Cb,line=1,col=2::m%25"
    );
    assert_eq!(
        run::annotation_at(Level::Warning, None, None, "m"),
        "::warning::m"
    );
}

#[test]
fn hadolint_reads_no_variable_of_its_own_from_the_computer() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .computer("dockerfmt", DOCKERFMT)
        .computer("hadolint", HADOLINT)
        .file(
            ".nebensound/repo.toml",
            "[repo]\naudience = \"public\"\npipeline = { check = [\"dockerfile\"], dependabot =              [\"docker\"] }\n",
        )
        .file("Dockerfile", "FROM scratch\n");
    support::git(&fixture.root(), &["init", "--quiet"]);
    support::git(&fixture.root(), &["add", "--all"]);
    let git = find::search("git", &std::env::var_os("PATH").unwrap_or_default(), false).unwrap();
    std::os::unix::fs::symlink(git, fixture.base.join("bin/git")).unwrap();
    fs::create_dir_all(fixture.base.join("home")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nebensound-repo"))
        .args(["run", "dockerfile", "--", "."])
        .current_dir(fixture.root())
        .env("PATH", fixture.base.join("bin"))
        .env("HOME", fixture.base.join("home"))
        .env("TMPDIR", fixture.base.join("tmp"))
        .env("GITHUB_ACTIONS", "true")
        .env("HADOLINT_NOFAIL", "1")
        .env("HADOLINT_FAILURE_THRESHOLD", "error")
        .env_remove("XDG_CACHE_HOME")
        .output()
        .expect("the built binary");
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{text}");
    has(&text, "1 Dockerfile(s) to check:");
    assert!(text.contains(" with 0 HADOLINT variable(s)"), "{text}");
}

#[test]
fn stylesheets_are_formatted_and_pass_stylelint() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .file("styles/a.css", "a {}\n")
        .file("styles/b.scss", "$a: 1;\n");
    let files = ["styles/a.css", "styles/b.scss"];
    let (code, text) = fixture.run(Check::Css, &files, true);
    assert_eq!(code, 0, "{text}");
    has(&text, "2 stylesheet(s) to check:");
    let prettier = text
        .lines()
        .find(|line| line.starts_with("prettier "))
        .unwrap();
    assert!(
        prettier.starts_with("prettier --check --no-color --with-node-modules --ignore-path "),
        "{prettier}"
    );
    assert!(
        prettier.ends_with(
            " --no-require-pragma --no-check-ignore-pragma --range-start=0 --range-end=Infinity \
             -- styles/a.css styles/b.scss"
        ),
        "{prettier}"
    );
    // Stylelint runs on its runtime, from its pinned folder, with the
    // default compiled into the tool and an empty ignore file.
    let folder = fixture.folder("stylelint", "18.0.0");
    has(
        &text,
        &format!("node --input-type=module --eval TOOLS={}", folder.display()),
    );
    has(&text, "default: 2 configurations extended");
    has(&text, "nothing ignored: []");
    has(&text, "Rules: the pipeline default");
    assert!(css::STYLELINT.contains("ignoreFiles: []"));

    let (code, text) = fixture.run(Check::Css, &[], true);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text,
        "No stylesheet below '.'; there is nothing to check.\n"
    );
}

#[test]
fn every_kind_of_css_finding_is_reported() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything();
    let files = [
        "missing.css",
        "styled.css",
        "unformatted.scss",
        "unreadable.css",
        "warned.css",
    ];
    for file in files {
        fixture.file(file, "a {}\n");
    }
    let (code, text) = fixture.run(Check::Css, &files, true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "::error file=unformatted.scss::not formatted the way Prettier writes it",
        "::error file=unreadable.css::Prettier cannot read it: SyntaxError: Unexpected token (1:1)",
        "styled.css:2:10 error Expected \"#00aa77\" to be \"#0a7\" (color-hex-length)",
        "::error file=styled.css,line=2,col=10::Expected \"#00aa77\" to be \"#0a7\" \
         (color-hex-length)",
        "::warning file=warned.css,line=1,col=1::Unexpected empty block (block-no-empty)",
        "::error file=missing.css::Stylelint cannot check it: Could not find \
         \"stylelint-config-clean-order\"",
    ] {
        has(&text, line);
    }

    // A warning fails nothing.
    let (code, text) = fixture.run(Check::Css, &["warned.css"], true);
    assert_eq!(code, 0, "{text}");

    // Elsewhere, an annotation reads as a line that starts with the place.
    let (code, text) = fixture.run(Check::Css, &["styled.css", "warned.css"], false);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "styled.css:2:10: error: Expected \"#00aa77\" to be \"#0a7\" (color-hex-length)",
    );
    has(
        &text,
        "warned.css:1:1: warning: Unexpected empty block (block-no-empty)",
    );
}

#[test]
fn without_its_programs_the_css_check_fails() {
    let _alone = alone();
    let fixture = Fixture::new();
    // Stylelint is taken pinned only: a copy on the computer does not count.
    fixture
        .computer("prettier", PRETTIER)
        .computer("node", NODE)
        .computer("stylelint", "#!/bin/sh\n")
        .file("unformatted.css", "a {}\n");
    let (code, text) = fixture.run(Check::Css, &["unformatted.css"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::stylelint 18.0.0 is not pinned in the tool's folder, and the check calls the \
         internals of the pinned copy only; `nebensound-repo setup --pinned` installs it",
    );
    // The other part still runs.
    has(
        &text,
        "::error file=unformatted.css::not formatted the way Prettier writes it",
    );

    fs::remove_file(fixture.base.join("bin/prettier")).unwrap();
    fixture
        .pinned(
            "stylelint",
            "18.0.0",
            "node_modules/.bin/stylelint",
            "#!/bin/sh\n",
        )
        .file("styled.css", "a {}\n");
    let (code, text) = fixture.run(Check::Css, &["styled.css"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::prettier 3.9.6 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    has(
        &text,
        "::error file=styled.css,line=2,col=10::Expected \"#00aa77\" to be \"#0a7\" \
         (color-hex-length)",
    );

    // A pinned Stylelint needs its runtime.
    fs::remove_file(fixture.base.join("bin/node")).unwrap();
    let (code, text) = fixture.run(Check::Css, &["styled.css"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::stylelint 18.0.0 needs node 22.22.2 or later, and there is none; \
         `nebensound-repo setup --with-runtimes` installs it",
    );
}

#[test]
fn stylelint_needs_a_runtime_from_the_register() {
    let _alone = alone();
    let fixture = Fixture::new();
    let source = nebensound_repo::register::BUILTIN_REGISTER;
    let start = source.find("[program.stylelint]").unwrap();
    let end = source.find("[program.taplo]").unwrap();
    let native = format!(
        "{}[program.stylelint]\nversion = \"18.0.0\"\ncommand = \"stylelint\"\n\n\
         [program.stylelint.download.linux-x86_64]\nurl = \"https://example.invalid/s\"\n\
         digest = \"sha256:{}\"\nexecutable = \"stylelint\"\n\n{}",
        &source[..start],
        "0".repeat(64),
        &source[end..]
    );
    let register = Register::parse(&native).unwrap();
    fixture
        .everything()
        .pinned("stylelint", "18.0.0", "stylelint", "#!/bin/sh\n")
        .file("a.css", "a {}\n");
    let (code, text) = fixture.run_with(&register, Check::Css, &["a.css"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::Stylelint runs on Node.js, and the register gives it no runtime to run on",
    );
}

/// The line of the output that starts with `prefix`.
fn line_of<'t>(text: &'t str, prefix: &str) -> &'t str {
    text.lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting with {prefix:?} in\n{text}"))
}

#[test]
fn pages_are_formatted_pass_html_validate_and_link_somewhere() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .computer("lychee", LYCHEE_PAGES)
        .file("index.html", "<!doctype html>\n")
        .file("docs/header.htm", "<header></header>\n")
        .file("lychee.toml", "max_retries = 1\n");
    let files = ["docs/header.htm", "index.html"];
    let (code, text) = fixture.run(Check::Html, &files, true);
    assert_eq!(code, 0, "{text}");
    has(&text, "2 HTML file(s) to check:");
    has(&text, "docs/header.htm");
    let prettier = line_of(&text, "prettier ");
    assert!(
        prettier.starts_with("prettier --check --no-color --with-node-modules --ignore-path "),
        "{prettier}"
    );
    assert!(
        prettier.ends_with(
            " --no-require-pragma --no-check-ignore-pragma --range-start=0 --range-end=Infinity \
             -- docs/header.htm index.html"
        ),
        "{prettier}"
    );
    // html-validate runs on its runtime, from its pinned folder, with the
    // default and the page rules compiled into the tool.
    let folder = fixture.folder("html-validate", "11.21.1");
    has(
        &text,
        "Rules: the pipeline default, with the repository's configuration and the page rules \
         laid over it",
    );
    has(
        &text,
        &format!("node --input-type=module --eval TOOLS={}", folder.display()),
    );
    has(&text, "default: 3 presets");
    has(&text, "page rules: 3 exports");
    assert!(html::HTML_VALIDATE.contains("pathToFileURL(RULES)"));
    assert!(html::PAGE_RULES.contains("export default function pageRules(Rule)"));
    // The link check reads the repository's configuration, then the one that
    // leaves out what lies below the empty folder links from the root of a
    // site are resolved below.
    let lychee = line_of(&text, "lychee ");
    assert!(
        lychee.starts_with(
            "lychee --offline --include-fragments --no-progress --hidden --no-ignore --config "
        ),
        "{lychee}"
    );
    let tmp = fixture.base.join("tmp");
    let root = lychee
        .split(" --root-dir ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .unwrap();
    assert!(
        root.starts_with(&format!("{}/nebensound-repo-run-", tmp.display()))
            && root.ends_with("/root"),
        "{lychee}"
    );
    assert!(
        lychee.contains("/lychee-rooted.toml --root-dir "),
        "{lychee}"
    );
    assert!(lychee.contains(" --format markdown --output "), "{lychee}");
    has(&text, "config: max_retries = 1");
    has(
        &text,
        &format!("config: {}", html::excluded(Path::new(root)).trim_end()),
    );
    has(&text, "root: an empty folder");
    has(&text, "lines: docs/header.htm");
    // What `run` wrote for the programs is gone again.
    assert_eq!(fs::read_dir(&tmp).unwrap().count(), 0);

    let (code, text) = fixture.run(Check::Html, &[], true);
    assert_eq!(code, 0, "{text}");
    assert_eq!(text, "No HTML file below '.'; there is nothing to check.\n");
}

#[test]
fn every_kind_of_html_finding_is_reported() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.everything().computer("lychee", LYCHEE_PAGES);
    let files = [
        "broken.html",
        "invalid.html",
        "loose.html",
        "plugin.html",
        "unformatted.html",
        "unreadable.htm",
    ];
    for file in files {
        fixture.file(file, "<p></p>\n");
    }
    let (code, text) = fixture.run(Check::Html, &files, true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "::error file=unformatted.html::not formatted the way Prettier writes it",
        "::error file=unreadable.htm::Prettier cannot read it: SyntaxError: Unexpected token (1:1)",
        "invalid.html:3:4 error no-implicit-button-type: <button> is missing recommended \"type\" \
         attribute",
        "::error file=invalid.html,line=3,col=4::no-implicit-button-type: <button> is missing \
         recommended \"type\" attribute",
        "::warning file=loose.html,line=2,col=1::no-trailing-whitespace: Trailing whitespace",
        "::error file=plugin.html::html-validate cannot check it: Failed to load plugin \
         \"html-validate-angular\"",
        // The report stands in the output, and the broken link is a finding
        // at lychee's line and column.
        "* [ERROR] <file:///x/missing.html> (at 11:29) | File not found",
        "::error file=broken.html,line=11,col=29::[ERROR] <file:///x/missing.html> | File not \
         found",
        "config: ",
    ] {
        has(&text, line);
    }

    // A warning fails nothing.
    let (code, text) = fixture.run(Check::Html, &["loose.html"], true);
    assert_eq!(code, 0, "{text}");

    // Elsewhere, an annotation reads as a line that starts with the place.
    let (code, text) = fixture.run(Check::Html, &["broken.html", "loose.html"], false);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "broken.html:11:29: error: [ERROR] <file:///x/missing.html> | File not found",
    );
    has(
        &text,
        "loose.html:2:1: warning: no-trailing-whitespace: Trailing whitespace",
    );
}

#[test]
fn links_from_the_root_of_a_site_are_left_out_below_the_folder_without_links() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .everything()
        .computer("lychee", LYCHEE_PAGES)
        .file("index.html", "<p></p>\n");
    // The temporary folder behind a link, as on macOS, in a folder whose name
    // holds what a pattern or TOML would read otherwise.
    let real = fixture.base.join("t.m+p(x)'s\"$");
    fs::create_dir(&real).unwrap();
    let link = fixture.base.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let register = Register::builtin().unwrap();
    let (code, text) = fixture.run_in(&register, &link, Check::Html, &["index.html"], true);
    assert_eq!(code, 0, "{text}");
    let lychee = line_of(&text, "lychee ");
    let root = format!("{}/nebensound-repo-run-", real.display());
    assert!(lychee.contains(&format!(" --root-dir {root}")), "{lychee}");
    let config = line_of(&text, "config: exclude");
    let table: ::toml::Table = ::toml::from_str(config.trim_start_matches("config: ")).unwrap();
    let pattern = table["exclude"][0].as_str().unwrap();
    let escaped = ::regex::escape(&real.display().to_string());
    assert!(
        pattern.starts_with(&format!("^file://{escaped}/nebensound\\-repo\\-run\\-"))
            && pattern.ends_with("/root/"),
        "{pattern}"
    );
    // The pattern takes every character literally.
    let address = ::regex::Regex::new(pattern).unwrap();
    let below = lychee
        .split(" --root-dir ")
        .nth(1)
        .and_then(|rest| rest.split(" --files-from").next())
        .unwrap();
    assert!(
        address.is_match(&format!("file://{below}/login")),
        "{pattern}"
    );
    assert!(!address.is_match(&format!("file://{}/login", link.display())));

    // A folder that is not there is taken as it is written.
    assert_eq!(
        html::rooted(Path::new("/nowhere/at/all")),
        PathBuf::from("/nowhere/at/all/root")
    );
    let table: ::toml::Table = ::toml::from_str(&html::excluded(Path::new("/a.b/r"))).unwrap();
    assert_eq!(table["exclude"][0].as_str(), Some("^file:///a\\.b/r/"));
}

#[test]
fn without_its_programs_the_html_check_fails() {
    let _alone = alone();
    let fixture = Fixture::new();
    // html-validate is taken pinned only: a copy on the computer does not
    // count.
    fixture
        .computer("prettier", PRETTIER)
        .computer("node", NODE)
        .computer("html-validate", "#!/bin/sh\n")
        .file("unformatted.html", "<p></p>\n");
    let (code, text) = fixture.run(Check::Html, &["unformatted.html"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::html-validate 11.21.1 is not pinned in the tool's folder, and the check calls \
         the internals of the pinned copy only; `nebensound-repo setup --pinned` installs it",
    );
    has(
        &text,
        "::error::lychee 0.24.2 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    // The other part still runs.
    has(
        &text,
        "::error file=unformatted.html::not formatted the way Prettier writes it",
    );

    fs::remove_file(fixture.base.join("bin/prettier")).unwrap();
    fixture
        .pinned(
            "html-validate",
            "11.21.1",
            "node_modules/.bin/html-validate",
            "#!/bin/sh\n",
        )
        .computer("lychee", LYCHEE_PAGES)
        .file("invalid.html", "<p></p>\n");
    let (code, text) = fixture.run(Check::Html, &["invalid.html"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::prettier 3.9.6 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    has(
        &text,
        "::error file=invalid.html,line=3,col=4::no-implicit-button-type: <button> is missing \
         recommended \"type\" attribute",
    );
    has(&text, "root: an empty folder");

    // A pinned html-validate needs its runtime.
    fs::remove_file(fixture.base.join("bin/node")).unwrap();
    let (code, text) = fixture.run(Check::Html, &["invalid.html"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::html-validate 11.21.1 needs node 22.22.0 or later, and there is none; \
         `nebensound-repo setup --with-runtimes` installs it",
    );

    // An unreadable lychee.toml stops the check before anything runs.
    fixture.file("lychee.toml", "x = 1\n");
    let config = fixture.root().join("lychee.toml");
    fs::set_permissions(&config, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&config).is_err() {
        let (code, text) = fixture.run(Check::Html, &["invalid.html"], true);
        assert_eq!(code, 1, "{text}");
        assert!(
            text.contains("::error file=lychee.toml::cannot be read: "),
            "{text}"
        );
        assert!(!text.contains("lychee --"), "{text}");
    }
}

#[test]
fn html_validate_needs_a_runtime_from_the_register() {
    let _alone = alone();
    let fixture = Fixture::new();
    let source = nebensound_repo::register::BUILTIN_REGISTER;
    let start = source.find("[program.html-validate]").unwrap();
    let end = source.find("[program.html-validate.download.any]").unwrap();
    let end = end + source[end..].find("\n\n").unwrap() + 2;
    let native = format!(
        "{}[program.html-validate]\nversion = \"11.21.1\"\ncommand = \"html-validate\"\n\n\
         [program.html-validate.download.linux-x86_64]\nurl = \"https://example.invalid/h\"\n\
         digest = \"sha256:{}\"\nexecutable = \"html-validate\"\n\n{}",
        &source[..start],
        "0".repeat(64),
        &source[end..]
    );
    let register = Register::parse(&native).unwrap();
    fixture
        .everything()
        .computer("lychee", LYCHEE_PAGES)
        .pinned("html-validate", "11.21.1", "html-validate", "#!/bin/sh\n")
        .file("a.html", "<p></p>\n");
    let (code, text) = fixture.run_with(&register, Check::Html, &["a.html"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::html-validate runs on Node.js, and the register gives it no runtime to run on",
    );
}
