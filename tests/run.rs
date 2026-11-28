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

use nebensound_repo::Register;
use nebensound_repo::console::Console;
use nebensound_repo::lychee_config;
use nebensound_repo::output::Output;
use nebensound_repo::path::RelativePath;
use nebensound_repo::programs::Platform;
use nebensound_repo::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER};
use nebensound_repo::repository::Repository;
use nebensound_repo::run::{
    self, Check, Context, Level, Packages, css, dockerfile, html, markdown, toml, yaml,
};
use nebensound_repo::setup::{Environment, find};
use support::{QUIET, call, program::executable};
use tempfile::TempDir;

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
echo "starts in: $(pwd -P)"
if [ -e .lycheeignore ]; then echo "a .lycheeignore where it starts"; fi
if grep -q broken "$list"; then
  printf '%s\n' "# Summary" "" "## Errors per input" "" "### Errors in $(grep broken "$list")" "" \
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
  printf '%s\n' "# Summary" "" "## Errors per input" "" "### Errors in $(grep broken "$list")" "" \
    "* [ERROR] <file:///x/missing.html> (at 11:29) | File not found" > "$out"
  exit 2
fi
printf '%s\n' "# Summary" > "$out"
exit 0
"####;

/// A page that carries everything a page must carry.
const PAGE: &str = "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\" />\n    \
                    <title>A page</title>\n    <meta name=\"description\" content=\"A page.\" />\n  \
                    </head>\n  <body>\n    <h1>A page</h1>\n    <img src=\"dot.svg\" alt=\"\" />\n  \
                    </body>\n</html>\n";

/// A page that carries nothing a page must carry: no title and no description
/// in its `<head>` on line 3, and no `<h1>` in its `<body>` on line 6.
const BARE: &str = "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\" />\n  \
                    </head>\n  <body>\n    <p>Nothing a page must carry.</p>\n  </body>\n</html>\n";

/// Node.js, which prints its version or runs the script it is handed the way
/// that script would: Stylelint, which hands each finding over as data, or
/// markdownlint, which writes what it finds for the tool to read.
const NODE: &str = r##"#!/bin/sh
if [ "$1" = "--version" ]; then echo v24.0.0; exit 0; fi
echo "node $1 $2 TOOLS=$TOOLS"
case "$3" in
  *'load("stylelint"'*)
    echo "stylelint job: $(cat "$JOB")"
    echo "nothing ignored: [$(cat "$NOTHING")]"
    status=0
    for f in $(grep -oE '"[^"]*\.s?css"' "$JOB" | tr -d '"'); do
      case "$f" in
        *styled*)
          echo "{\"level\":\"error\",\"file\":\"$f\",\"line\":2,\"column\":10,\"message\":\"Expected \\\"#00aa77\\\" to be \\\"#0a7\\\" (color-hex-length)\"}" >> "$FINDINGS"
          status=1 ;;
        *warned*)
          echo "{\"level\":\"warning\",\"file\":\"$f\",\"line\":1,\"column\":1,\"message\":\"Unexpected empty block (block-no-empty)\"}" >> "$FINDINGS" ;;
        *missing*)
          echo "{\"level\":\"error\",\"file\":\"$f\",\"message\":\"Stylelint cannot check it: Could not find \\\"stylelint-config-clean-order\\\"\"}" >> "$FINDINGS"
          status=1 ;;
        *malformed*) echo "{\"level\":\"loud\",\"message\":\"x\"}" >> "$FINDINGS" ;;
        *vanished*) printf '\377\n' >> "$FINDINGS" ;;
      esac
    done
    exit "$status" ;;
esac
echo "job: $(cat "$JOB")"
entries=""
for f in $(grep -o '"[^"]*\.md"' "$JOB" | tr -d '"'); do
  case "$f" in
    *heading*) entry="\"$f\":[{\"lineNumber\":3,\"ruleNames\":[\"MD001\",\"heading-increment\"],\"ruleDescription\":\"Heading levels should only increment by one level at a time\",\"errorDetail\":\"Expected: h2; Actual: h3\",\"errorContext\":null,\"errorRange\":null,\"severity\":\"error\"}]" ;;
    *style*) entry="\"$f\":[{\"lineNumber\":1,\"ruleNames\":[\"MD003\",\"heading-style\"],\"ruleDescription\":\"Heading style\",\"errorDetail\":\"\",\"errorContext\":\"# x\",\"errorRange\":[1,3]}]" ;;
    *warned*) entry="\"$f\":[{\"lineNumber\":2,\"ruleNames\":[\"MD009\",\"no-trailing-spaces\"],\"ruleDescription\":\"Trailing spaces\",\"errorRange\":[0,1],\"severity\":\"warning\"}]" ;;
    *) continue ;;
  esac
  entries="${entries:+$entries,}$entry"
done
case "$(cat "$JOB")" in
  *'"frontMatter":"("'*) refused='"Invalid regular expression: /(/u: Unterminated group"' ;;
  *) refused=null ;;
esac
case "$entries" in
  *unreadable*) echo "not JSON" > "$RESULTS" ;;
  *) printf '{"frontMatter":%s,"groups":[{%s}]}' "$refused" "$entries" > "$RESULTS" ;;
esac
exit 0
"##;

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
            .computer("taplo", TAPLO)
            .computer("lychee", LYCHEE)
            .computer("node", NODE)
            .computer("actionlint", ACTIONLINT)
            .computer("dockerfmt", DOCKERFMT)
            .computer("hadolint", HADOLINT)
            .pinned(
                "markdownlint",
                "0.41.1",
                "node_modules/markdownlint/package.json",
                "{}\n",
            )
            .pinned(
                "stylelint",
                "18.0.0",
                "node_modules/.bin/stylelint",
                "#!/bin/sh\n",
            )
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

    /// What `run` writes, as it writes it.
    fn log(&self, check: Check, files: &[&str], workflow: bool) -> (u8, String) {
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
        let repository = Repository::at(&root);
        // Every file of the repository counts as tracked and kept, so that
        // the Markdown check finds the files of markdownlint's configuration.
        let all = every_file(&root);
        let context = Context {
            register,
            env: &env,
            root: &root,
            temp,
            packages: Packages {
                repository: &repository,
                tracked: &all,
                kept: &all,
                excluded: &[],
                orders: &[],
            },
        };
        let paths: Vec<RelativePath> = files
            .iter()
            .map(|file| RelativePath::new(file).expect("a path"))
            .collect();
        let refs: Vec<&RelativePath> = paths.iter().collect();
        let mut out = Vec::new();
        let mut console = Console::new(&mut out, workflow);
        let code = run::run(&context, check, &RelativePath::root(), &refs, &mut console);
        let code = console.end(code);
        (code, String::from_utf8(out).expect("UTF-8"))
    }

    /// What `run` writes in a workflow run, without the lines that stop and
    /// resume the runner's processing of workflow commands.
    fn run(&self, check: Check, files: &[&str], workflow: bool) -> (u8, String) {
        let (code, text) = self.log(check, files, workflow);
        (code, support::printed(&text))
    }
}

/// Every file below `root`, as a path from it.
fn every_file(root: &Path) -> Vec<RelativePath> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).expect("a folder") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let relative = path.strip_prefix(root).expect("below the root");
                found.push(RelativePath::new(&relative.to_string_lossy()).expect("a path"));
            }
        }
    }
    found.sort();
    found
}

fn has(text: &str, line: &str) {
    assert!(
        text.lines().any(|found| found == line),
        "no line {line:?} in\n{text}"
    );
}

#[test]
fn json_files_parse_and_are_formatted() {
    let fixture = Fixture::new();
    fixture.everything();
    fixture
        .file("a.json", "\u{feff}{}\n")
        .file("b.jsonc", "{\n  // a comment\n  \"a\": [1, 2,],\n}\n")
        .file("Pipfile.lock", "{}\n");
    let (code, text) = fixture.run(Check::Json, &["Pipfile.lock", "a.json", "b.jsonc"], true);
    assert_eq!(code, 0, "{text}");
    has(&text, "3 JSON file(s) to check:");
    has(&text, "3 file(s) parsed, 1 of them as JSONC.");
    let root = fixture.root();
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
             --no-config -- a.json b.jsonc"
        ),
        "{prettier}"
    );
    assert!(!text.contains(&root.display().to_string()), "{text}");
    // What `run` wrote for the programs is gone again.
    assert_eq!(fs::read_dir(fixture.base.join("tmp")).unwrap().count(), 0);
}

/// Prettier, as the JSON check starts it, when the repository configures
/// it: once for each group of files with the same options, each handed as a
/// flag, and with `--no-config`, so that it looks for no file of its own.
#[test]
fn prettier_gets_the_options_of_each_file_from_the_tool() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .file(
            ".editorconfig",
            "root = true\n\n[*]\nindent_style = space\nindent_size = 4\n",
        )
        .file(
            ".prettierrc.yaml",
            "printWidth: 100\noverrides:\n  - files: \"fixtures/deep/*.json\"\n    options:\n      \
             useTabs: true\n",
        )
        .file("a.json", "{}\n")
        .file("fixtures/deep/b.json", "{}\n")
        .file("fixtures/c.json", "{}\n");
    let files = ["a.json", "fixtures/c.json", "fixtures/deep/b.json"];

    let (code, text) = fixture.run(Check::Json, &files, true);

    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "Prettier formats 2 file(s) with --print-width=100 --tab-width=4 --use-tabs=false.",
    );
    has(
        &text,
        "Prettier formats 1 file(s) with --print-width=100 --tab-width=4 --use-tabs=true.",
    );
    let started: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("prettier "))
        .map(|line| line.split(" --range-end=Infinity ").nth(1).unwrap())
        .collect();
    assert_eq!(
        started,
        [
            "--no-config --print-width=100 --tab-width=4 --use-tabs=false -- a.json \
             fixtures/c.json",
            "--no-config --print-width=100 --tab-width=4 --use-tabs=true -- fixtures/deep/b.json",
        ]
    );
}

/// A configuration in JavaScript, a plugin and an entry of `overrides` that
/// names a parser only a plugin would bring: each is named and fails the
/// check, and none of them reaches Prettier, which formats with what is
/// left. A parser Prettier brings itself reaches it.
#[test]
fn what_prettier_is_not_handed_fails_the_check_by_name() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .file(
            "prettier.config.mjs",
            "export default { plugins: [\"./plugin.mjs\"] };\n",
        )
        .file(
            ".prettierrc.json",
            "{ \"plugins\": [\"./plugin.mjs\"], \"semi\": false, \"requirePragma\": false, \
             \"overrides\": [{ \"files\": \"*.json\", \"options\": { \"parser\": \"toml\" } }, \
             { \"files\": \"b.json\", \"options\": { \"parser\": \"json5\" } }] }",
        )
        .file("a.json", "{}\n")
        .file("b.json", "{}\n");

    let (code, text) = fixture.run(Check::Json, &["a.json", "b.json"], true);

    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=prettier.config.mjs::not read: the tool reads the configuration of prettier \
         only as data, from one file at the root of the repository (`.prettierrc`, \
         `.prettierrc.json`, `.prettierrc.yml`, `.prettierrc.yaml`, `.prettierrc.json5`, \
         `.prettierrc.toml`), and never runs this one",
    );
    has(
        &text,
        "::error file=.prettierrc.json::`plugins` is not handed to Prettier: it names modules that \
         would be loaded and run, and the check runs no code of the repository",
    );
    has(
        &text,
        "::error file=.prettierrc.json::`overrides[0].options.parser` is not the name of a parser \
         Prettier brings itself; any other would need a plugin, so it is not handed to Prettier",
    );
    assert!(!text.contains("`requirePragma`"), "{text}");
    has(&text, "Prettier formats 1 file(s) with --semi=false.");
    has(
        &text,
        "Prettier formats 1 file(s) with --parser=json5 --semi=false.",
    );
    let started: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("prettier "))
        .map(|line| line.split(" --range-end=Infinity ").nth(1).unwrap())
        .collect();
    assert_eq!(
        started,
        [
            "--no-config --semi=false -- a.json",
            "--no-config --parser=json5 --semi=false -- b.json",
        ]
    );
    assert!(!text.contains("--plugin"), "{text}");
    assert!(!text.contains("--parser=toml"), "{text}");

    // A repository with only an `.editorconfig` that sets nothing Prettier
    // uses: Prettier formats with its own defaults, and says so.
    fs::remove_file(fixture.root().join("prettier.config.mjs")).unwrap();
    fs::remove_file(fixture.root().join(".prettierrc.json")).unwrap();
    fixture.file(".editorconfig", "[*]\ncharset = utf-8\n");
    let (code, text) = fixture.run(Check::Json, &["a.json"], true);
    assert_eq!(code, 0, "{text}");
    has(&text, "Prettier formats 1 file(s) with its own defaults.");
}

#[test]
fn every_kind_of_json_finding_is_reported() {
    let fixture = Fixture::new();
    fixture.everything();
    let files = [
        "broken.json",
        "empty.jsonc",
        "folder.json",
        "latin.jsonc",
        "quoted.jsonc",
        "two.json",
        "unformatted.json",
        "unreadable.geojson",
    ];
    fixture
        .file("broken.json", "{ \"name\": \"fixture\", }\n")
        .file("empty.jsonc", "// nothing but a comment\n")
        .file("folder.json/inside", "")
        .file("quoted.jsonc", "{ 'name': \"fixture\" }\n")
        .file("two.json", "{} {}\n")
        .file("unformatted.json", "{}\n")
        .file("unreadable.geojson", "{}\n");
    fs::write(fixture.root().join("latin.jsonc"), b"\"caf\xe9\"\n").unwrap();
    let (code, text) = fixture.run(Check::Json, &files, true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "::error file=broken.json,line=1,col=22::not valid JSON: key must be a string",
        "::error file=empty.jsonc::not valid JSONC: the file holds no value, a JSONC document is \
         exactly one",
        "::error file=latin.jsonc::not valid JSONC: it is not UTF-8 (invalid utf-8 sequence of 1 \
         bytes from index 4)",
        "::error file=quoted.jsonc,line=1,col=3::not valid JSONC: Single-quoted strings are not \
         allowed",
        "::error file=two.json::not valid JSON: the file holds 2 values, a JSON document is \
         exactly one",
        "[warn] unformatted.json",
        "::error file=unformatted.json::not formatted the way Prettier writes it",
        "::error file=unreadable.geojson::Prettier cannot read it: SyntaxError: Unexpected token \
         (1:1)",
        "8 file(s) parsed, 3 of them as JSONC.",
    ] {
        has(&text, line);
    }
    assert!(
        text.contains("::error file=folder.json::cannot be read: "),
        "{text}"
    );
    // Not a file: no finding on it.
    assert!(!text.contains("::error file=Code style"), "{text}");

    // Elsewhere a finding reads as a line that starts with its place.
    let (code, text) = fixture.run(Check::Json, &files, false);
    assert_eq!(code, 1, "{text}");
    assert!(!text.contains("::error"), "{text}");
    has(
        &text,
        "two.json: error: not valid JSON: the file holds 2 values, a JSON document is exactly one",
    );
    has(
        &text,
        "quoted.jsonc:1:3: error: not valid JSONC: Single-quoted strings are not allowed",
    );
}

#[test]
fn json_is_judged_as_strictly_as_before() {
    let strict = |text: &str| run::json::strict_finding(text.as_bytes()).map(|found| found.message);
    let commented =
        |text: &str| run::json::commented_finding(text.as_bytes()).map(|found| found.message);
    // What jq passed: one value, also after a byte order mark, also a number
    // beyond the range of a double, also bytes that are no UTF-8 in a string.
    for text in ["{}", "\u{feff}[1]", "1e400", "{\"a\": 1, \"a\": 2}"] {
        assert_eq!(strict(text), None, "{text:?}");
    }
    assert_eq!(
        run::json::strict_finding(b"{\"a\": \"\xff\"}"),
        None,
        "a byte that is no UTF-8"
    );
    for (text, expected) in [
        (
            "",
            "not valid JSON: the file holds 0 values, a JSON document is exactly one",
        ),
        (
            "{}{}",
            "not valid JSON: the file holds 2 values, a JSON document is exactly one",
        ),
        ("[1,]", "not valid JSON: expected value"),
        ("// a comment\n{}", "not valid JSON: expected value"),
        // What jq took and RFC 8259 does not: the names of numbers that are
        // none. They are findings now.
        ("[NaN]", "not valid JSON: expected value"),
        ("nan", "not valid JSON: expected ident"),
        ("[Infinity]", "not valid JSON: expected value"),
    ] {
        assert_eq!(strict(text).as_deref(), Some(expected), "{text:?}");
    }
    assert_eq!(
        run::json::strict_finding(b"{\"a\": 1,\n}"),
        Some(run::Finding {
            position: Some((2, 1)),
            message: "not valid JSON: key must be a string".to_owned(),
        })
    );
    // What Biome passed as JSONC: comments and trailing commas, a byte order
    // mark, and a key twice.
    for text in [
        "// one\n/* two */ {\"a\": [1, 2,],}",
        "\u{feff}{}",
        "{\"a\": 1, \"a\": 2}",
    ] {
        assert_eq!(commented(text), None, "{text:?}");
    }
    // What it refused.
    for (text, position) in [
        ("{a: 1}", (1, 2)),
        ("{'a': 1}", (1, 2)),
        ("[0x1F]", (1, 2)),
        ("[NaN]", (1, 2)),
        ("[+1]", (1, 2)),
        ("[.5]", (1, 2)),
        ("{\"a\": 1 \"b\": 2}", (1, 8)),
        ("{} {}", (1, 4)),
        ("[1,,2]", (1, 4)),
    ] {
        let found = run::json::commented_finding(text.as_bytes());
        assert!(
            found
                .as_ref()
                .is_some_and(|found| found.message.starts_with("not valid JSONC: ")),
            "{text:?}: {found:?}"
        );
        assert_eq!(
            found.and_then(|found| found.position),
            Some(position),
            "{text:?}"
        );
    }
}

#[test]
fn lockfiles_are_parsed_and_not_formatted() {
    let fixture = Fixture::new();
    fixture.everything();
    fixture.file("Pipfile.lock", "{}\n");
    let (code, text) = fixture.run(Check::Json, &["Pipfile.lock"], true);
    assert_eq!(code, 0, "{text}");
    has(&text, "1 file(s) parsed, 0 of them as JSONC.");
    has(
        &text,
        "No file to format-check: lockfiles are parsed, not formatted.",
    );
    assert!(!text.contains("prettier "), "{text}");
}

#[test]
fn nothing_to_check_is_said_and_passes() {
    let fixture = Fixture::new();
    let (code, text) = fixture.run(Check::Yaml, &[], true);
    assert_eq!(code, 0, "{text}");
    assert_eq!(text, "No YAML file below '.'; there is nothing to check.\n");
}

#[test]
fn outside_a_workflow_run_every_call_of_run_opens_with_the_gate() {
    let outside = "Not running in GitHub Actions, so the result can differ from the gate's, \
                   which is the one that counts.";
    assert_eq!(run::OUTSIDE, outside);
    // Read as a plain line, it is no finding.
    assert_eq!(run::plain(outside).as_deref(), Some(outside));

    let fine = support::repository(&[(".nebensound/repo.toml", QUIET)]);
    let root = fine.path().to_str().unwrap();
    let call_with = |output: Output| {
        let mut out = Vec::new();
        let code = nebensound_repo::run_against(
            ["nebensound-repo", "-C", root, "run", "json", "."],
            BUILTIN_REGISTER,
            BUILTIN_DEFAULTS,
            output,
            &mut out,
        );
        (String::from_utf8(out).unwrap(), code)
    };
    let nothing = "No JSON file below '.'; there is nothing to check.\n";
    // Elsewhere and in a terminal the line comes first.
    for output in [Output::Elsewhere, Output::Terminal] {
        let (text, code) = call_with(output);
        assert_eq!(code, 0, "{text}");
        assert_eq!(text, format!("{outside}\n{nothing}"));
    }
    // A workflow run does not get it: the gate's log stays as it was.
    let (text, code) = call_with(Output::Workflow);
    assert_eq!(code, 0, "{text}");
    assert_eq!(support::printed(&text), nothing);
    // Another command does not get it either.
    let (text, _) = call(&["nebensound-repo", "-C", root, "files", "json", "."]);
    assert!(!text.contains(outside), "{text}");
}

#[test]
fn a_missing_program_fails_the_check_and_names_setup() {
    let fixture = Fixture::new();
    fixture.file("a.json", "{}\n");
    let (code, text) = fixture.run(Check::Json, &["a.json"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::prettier 3.9.6 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    // The parsing needs no program and runs all the same.
    has(&text, "1 file(s) parsed, 0 of them as JSONC.");

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
        .computer("markdownlint", "#!/bin/sh\n")
        .computer("lychee", LYCHEE)
        .pinned("prettier", "3.9.6", "node_modules/.bin/prettier", PRETTIER)
        .file("README.md", "# x\n");
    let (code, text) = fixture.run(Check::Markdown, &["README.md"], false);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "error: markdownlint 0.41.1 is not pinned in the tool's folder, and the check calls the \
         internals of the pinned copy only; `nebensound-repo setup --pinned` installs it",
    );
    has(
        &text,
        "error: prettier 3.9.6 needs node 14.0.0 or later, and there is none; `nebensound-repo \
         setup --with-runtimes` installs it",
    );
}

#[test]
fn a_pinned_program_on_npm_starts_with_its_runtime_in_front() {
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
    let fixture = Fixture::new();
    fixture
        .everything()
        .file("README.md", "# x\n")
        .file("lychee.toml", "max_retries = 1\n");
    let (code, text) = fixture.run(Check::Markdown, &["README.md"], true);
    assert_eq!(code, 0, "{text}");
    let folder = fixture.folder("markdownlint", "0.41.1");
    has(
        &text,
        &format!("node --input-type=module --eval TOOLS={}", folder.display()),
    );
    assert!(
        text.contains("Markdownlint: the default, nothing laid over it"),
        "{text}"
    );
    has(
        &text,
        "job: {\"frontMatter\":null,\"noInlineConfig\":false,\"groups\":[{\"config\":\
         {\"MD013\":false},\"files\":[\"README.md\"]}]}",
    );
    has(&text, "config: max_retries = 1");
    has(
        &text,
        &format!("lines: {}/README.md", fixture.root().display()),
    );
    // lychee starts in the folder of what it is handed, where there is no
    // `.lycheeignore` of the repository.
    let starts = line_of(&text, "starts in: ");
    assert!(
        starts.starts_with(&format!(
            "starts in: {}/nebensound-repo-run-",
            fixture.base.join("tmp").display()
        )),
        "{starts}"
    );
    assert!(!text.contains("a .lycheeignore where it starts"), "{text}");
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
         level at a time [Expected: h2; Actual: h3]",
        "::error file=heading.md,line=3::MD001/heading-increment Heading levels should only \
         increment by one level at a time [Expected: h2; Actual: h3]",
        "style.md:1:1 error MD003/heading-style Heading style [Context: \"# x\"]",
        "::error file=style.md,line=1,col=1::MD003/heading-style Heading style [Context: \"# \
         x\"]",
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

/// The configuration lychee was handed, as the fake one printed it: from
/// the line `config: ` up to the line `lines: `.
fn handed_config(text: &str) -> ::toml::Table {
    let start = text.find("config: ").expect("a configuration");
    let rest = &text[start + "config: ".len()..];
    let end = rest.find("\nlines: ").expect("the lines after it");
    ::toml::from_str(&rest[..end]).expect("TOML")
}

#[test]
fn the_link_check_hands_lychee_the_settings_it_knows_and_the_lycheeignore() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .file("README.md", "# x\n")
        .file(
            "lychee.toml",
            "max_retries = 2\nexclude = ['^https://a\\.example/']\n\n[hosts.\"b.example\"]\nconcurrency = 1\n",
        )
        .file(
            ".lycheeignore",
            "# a comment\n   # one after white space\n\n^https://c\\.example/\r\n",
        );
    let (code, text) = fixture.run(Check::Markdown, &["README.md"], true);
    assert_eq!(code, 0, "{text}");
    let config = handed_config(&text);
    let expected: ::toml::Table = ::toml::from_str(
        "max_retries = 2\nexclude = ['^https://a\\.example/', '^https://c\\.example/']\n\n\
         [hosts.\"b.example\"]\nconcurrency = 1\n",
    )
    .unwrap();
    assert_eq!(config, expected, "{text}");
    // lychee starts where the repository's `.lycheeignore` is out of its
    // reach.
    assert!(!text.contains("a .lycheeignore where it starts"), "{text}");

    // Without a `lychee.toml`, the `.lycheeignore` alone.
    fs::remove_file(fixture.root().join("lychee.toml")).unwrap();
    let (code, text) = fixture.run(Check::Markdown, &["README.md"], true);
    assert_eq!(code, 0, "{text}");
    let expected: ::toml::Table = ::toml::from_str("exclude = ['^https://c\\.example/']").unwrap();
    assert_eq!(handed_config(&text), expected, "{text}");
}

#[test]
fn a_key_that_starts_a_command_ends_the_run_or_reads_other_files_fails_by_name() {
    let fixture = Fixture::new();
    fixture.everything().file("unformatted.md", "# x\n");
    for (key, why) in lychee_config::REFUSED {
        fixture.file("lychee.toml", &format!("max_retries = 1\n{key} = 1\n"));
        let (code, text) = fixture.run(Check::Markdown, &["unformatted.md"], true);
        assert_eq!(code, 1, "{text}");
        has(
            &text,
            &format!("::error file=lychee.toml::`{key}` is not handed to lychee: it {why}"),
        );
        // lychee does not start, and the other parts of the check still run.
        assert!(!text.contains("lychee --"), "{text}");
        has(
            &text,
            "::error file=unformatted.md::not formatted the way Prettier writes it",
        );
    }

    // A key the tool does not know, such as a slip of the pen, as well; and
    // every such key is named.
    fixture.file(
        "lychee.toml",
        "exclude_paths = []\npreprocess = { command = \"sh\" }\n",
    );
    let (code, text) = fixture.run(Check::Markdown, &["unformatted.md"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=lychee.toml::`exclude_paths` is not a setting the link check knows, so it is \
         not handed to lychee",
    );
    assert!(
        text.contains("::error file=lychee.toml::`preprocess` "),
        "{text}"
    );
    assert!(!text.contains("lychee --"), "{text}");

    // The HTML check shares the link check.
    fixture
        .computer("lychee", LYCHEE_PAGES)
        .file("lychee.toml", "dump = true\n")
        .file("index.html", "<p></p>\n");
    let (code, text) = fixture.run(Check::Html, &["index.html"], true);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("::error file=lychee.toml::`dump` "), "{text}");
    assert!(!text.contains("lychee --"), "{text}");
}

#[test]
fn a_lychee_configuration_the_tool_cannot_read_fails_by_name() {
    let fixture = Fixture::new();
    fixture.everything().file("README.md", "# x\n");
    for (source, message) in [
        ("x = ", "is no TOML: "),
        ("exclude = 'a'", "`exclude` is not a list of text"),
        ("exclude = [1]", "`exclude` is not a list of text"),
        ("exclude_path = 'a'", "`exclude_path` is not a list of text"),
        ("exclude_path = ['(']", "`exclude_path`: "),
    ] {
        fixture.file("lychee.toml", source);
        let (code, text) = fixture.run(Check::Markdown, &["README.md"], true);
        assert_eq!(code, 1, "{source}: {text}");
        assert!(
            text.contains(&format!("::error file=lychee.toml::{message}")),
            "{source}: {text}"
        );
        assert!(!text.contains("lychee --"), "{source}: {text}");
    }
}

#[test]
fn exclude_path_takes_out_what_it_matches_from_the_root() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .file("docs/a.md", "# x\n")
        .file("b.md", "# x\n")
        .file("lychee.toml", "exclude_path = ['^docs/']\n");
    let (code, text) = fixture.run(Check::Markdown, &["b.md", "docs/a.md"], true);
    assert_eq!(code, 0, "{text}");
    has(&text, &format!("lines: {}/b.md", fixture.root().display()));
    let left_out = format!("{}/docs/a.md", fixture.root().display());
    assert!(
        !text.lines().any(|line| line.ends_with(&left_out)),
        "{text}"
    );
    // exclude_path is the tool's, not lychee's.
    assert!(handed_config(&text).is_empty(), "{text}");

    // Every file out, lychee does not start.
    fixture.file("lychee.toml", "exclude_path = ['.']\n");
    let (code, text) = fixture.run(Check::Markdown, &["b.md", "docs/a.md"], true);
    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "lychee is not started: `exclude_path` in lychee.toml takes out every file it would read",
    );
    assert!(!text.contains("lychee --"), "{text}");
}

#[test]
fn a_lycheeignore_is_read_only_as_it_stands_in_the_repository() {
    let fixture = Fixture::new();
    fixture.everything().file("README.md", "# x\n");
    let ignore = fixture.root().join(".lycheeignore");
    std::os::unix::fs::symlink("/etc/hosts", &ignore).unwrap();
    let (code, text) = fixture.run(Check::Markdown, &["README.md"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=.lycheeignore::is a link or no regular file; the link check reads it only \
         as it stands in the repository",
    );
    assert!(!text.contains("lychee --"), "{text}");

    fs::remove_file(&ignore).unwrap();
    fixture.file(".lycheeignore", "a\n");
    fs::set_permissions(&ignore, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&ignore).is_ok() {
        // Whoever can read every file reads this one too.
        return;
    }
    let (code, text) = fixture.run(Check::Markdown, &["README.md"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error file=.lycheeignore::cannot be read: "),
        "{text}"
    );
}

#[test]
fn yaml_files_parse_are_formatted_and_pass_their_linters() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .computer("shellcheck", "#!/bin/sh\n")
        .file(".yamllint.yaml", "rules: {}\n")
        .file(".github/workflows/ci.yml", "on: push\n")
        .file(
            ".github/dependabot.yml",
            "version: 2\nupdates:\n  - package-ecosystem: cargo\n    directory: /\n    \
             schedule:\n      interval: weekly\n",
        )
        .file(
            "config.yaml",
            "\u{feff}a: &a 1\nb: *a\nm:\n  <<: {x: 1}\n  <<: {y: 2}\n",
        );
    let files = [
        ".github/dependabot.yml",
        ".github/workflows/ci.yml",
        "config.yaml",
    ];
    let (code, text) = fixture.run(Check::Yaml, &files, true);
    assert_eq!(code, 0, "{text}");
    has(&text, "1 of them workflow(s).");
    has(&text, "Dependabot configuration: .github/dependabot.yml");
    has(&text, "3 file(s) parsed.");
    has(
        &text,
        ".github/dependabot.yml against the Dependabot schema: 0 finding(s).",
    );
    // A configuration of yamllint is not read.
    assert!(!text.contains(".yamllint.yaml"), "{text}");
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
         format -ignore ^unexpected key \"queue\" for \"concurrency\" section\\.  -ignore \
         ^property \"workflow_(repository|sha)\" is not defined in object type \\{check_run_id: \
         number;  -format {{json .}} -- .github/workflows/ci.yml",
    );
}

#[test]
fn every_kind_of_yaml_finding_is_reported() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .file(".github/workflows/broken.yaml", "on: push\n")
        .file(
            ".github/dependabot.yaml",
            "version: 2\nupdates:\n  - package-ecosystem: cargo\n    directory: /\n    \
             schedules:\n      interval: weekly\n",
        )
        .file("duplicate.yaml", "name: a\nname: b\n")
        .file("alias.yaml", "a: *x\n")
        .file("folder.yaml/inside", "")
        .file("unformatted.yml", "a: 1\n");
    let files = [
        ".github/dependabot.yaml",
        ".github/workflows/broken.yaml",
        "alias.yaml",
        "duplicate.yaml",
        "folder.yaml",
        "unformatted.yml",
    ];
    let (code, text) = fixture.run(Check::Yaml, &files, true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "::error file=unformatted.yml::not formatted the way Prettier writes it",
        "::error file=duplicate.yaml,line=2,col=1::the key \"name\" stands twice in this \
         mapping: a mapping holds each key once",
        "::error file=alias.yaml,line=1,col=4::not valid YAML: the alias \"*x\" names no anchor \
         declared before it in its document",
        "::error file=.github/workflows/broken.yaml,line=10,col=3::\"runs-on\" section is missing \
         in job \"test\" [syntax-check]",
        "::error file=.github/dependabot.yaml::$.updates[0]: Additional properties are not \
         allowed ('schedules' was unexpected)",
        "::error file=.github/dependabot.yaml::$.updates[0]: \"schedule\" is a required property",
        ".github/dependabot.yaml against the Dependabot schema: 2 finding(s).",
        "ShellCheck is not on the search path, so actionlint reads no `run:` script with it",
    ] {
        has(&text, line);
    }
    assert!(
        text.contains("::error file=folder.yaml::cannot be read: "),
        "{text}"
    );

    // Elsewhere a finding is a line.
    let (_, text) = fixture.run(Check::Yaml, &["duplicate.yaml"], false);
    has(
        &text,
        "duplicate.yaml:2:1: error: the key \"name\" stands twice in this mapping: a mapping \
         holds each key once",
    );
    assert!(!text.contains("endgroup"), "{text}");
}

#[test]
fn a_key_of_a_dependabot_configuration_that_is_no_text_is_held_against_the_schema_as_text() {
    let fixture = Fixture::new();
    fixture.everything().file(
        ".github/dependabot.yml",
        "version: 2\nupdates:\n  - package-ecosystem: cargo\n    directory: /\n    \
         schedule:\n      interval: weekly\n1: x\n",
    );
    let (code, text) = fixture.run(Check::Yaml, &[".github/dependabot.yml"], true);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("('1' was unexpected)"), "{text}");
}

#[test]
fn a_dependabot_configuration_that_does_not_parse_is_not_held_against_the_schema() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .file(".github/dependabot.yml", "version: 2\nversion: 2\n");
    let (code, text) = fixture.run(Check::Yaml, &[".github/dependabot.yml"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=.github/dependabot.yml,line=2,col=1::the key \"version\" stands twice in \
         this mapping: a mapping holds each key once",
    );
    assert!(!text.contains("against the Dependabot schema"), "{text}");

    // Nor is one that cannot be read.
    let dir = fixture.root().join(".github/dependabot.yml");
    fs::remove_file(&dir).unwrap();
    fs::create_dir(&dir).unwrap();
    let (code, text) = fixture.run(Check::Yaml, &[".github/dependabot.yml"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error file=.github/dependabot.yml::cannot be read: "),
        "{text}"
    );
    assert!(!text.contains("against the Dependabot schema"), "{text}");
}

#[test]
fn yaml_is_judged_by_the_tool() {
    let findings = |text: &str| -> Vec<(Option<(usize, usize)>, String)> {
        yaml::findings(text.as_bytes())
            .into_iter()
            .map(|found| (found.position, found.message))
            .collect()
    };
    let twice = |key: &str| {
        format!("the key \"{key}\" stands twice in this mapping: a mapping holds each key once")
    };
    // Valid YAML, also in several documents, with keys that only look alike,
    // merge keys, the same key in two mappings, aliases and lists.
    for text in [
        "",
        "a: 1\n---\na: 2\n",
        "true: 1\nTrue: 2\n",
        "b: &b {x: 1}\nm:\n  <<: *b\n  <<: *b\n",
        "a:\n  x: 1\nb:\n  x: 1\n",
        "- &a x\n- *a\n- [1, {k: v}]\n",
        "? [a]\n: 1\n? [a]\n: 2\n",
        "\u{feff}a: 1\n",
    ] {
        assert_eq!(findings(text), [], "{text:?}");
    }
    // Every key twice, at the place of the second, also in a flow mapping and
    // when one of them is quoted.
    assert_eq!(
        findings("a: 1\n\"a\": 2\n{b: 1, b: 2}: x\nn:\n  1: a\n  '1': b\n  1: c\n"),
        [
            (Some((2, 1)), twice("a")),
            (Some((3, 8)), twice("b")),
            (Some((6, 3)), twice("1")),
            (Some((7, 3)), twice("1")),
        ]
    );
    // A key whose value is a collection, and the key after it.
    assert_eq!(
        findings("a: 1\na:\n  b: 2\nc: [1]\nc: *x\n"),
        [
            (Some((2, 1)), twice("a")),
            (Some((5, 1)), twice("c")),
            (
                Some((5, 4)),
                "not valid YAML: the alias \"*x\" names no anchor declared before it in its \
                 document"
                    .to_owned()
            ),
        ]
    );
    // An anchor counts in its own document only.
    assert_eq!(
        findings("x: &a 1\n---\ny: [*a]\n"),
        [(
            Some((3, 5)),
            "not valid YAML: the alias \"*a\" names no anchor declared before it in its document"
                .to_owned()
        )]
    );
    assert_eq!(
        findings("a: [1,\n"),
        [(
            Some((2, 1)),
            "not valid YAML: while parsing a node, did not find expected node content".to_owned()
        )]
    );
    assert_eq!(
        yaml::findings(b"a: \xff\n"),
        [run::Finding {
            position: None,
            message: "not valid YAML: it is not UTF-8 (invalid utf-8 sequence of 1 bytes from \
                      index 3)"
                .to_owned(),
        }]
    );
}

#[test]
fn a_dependabot_configuration_is_held_against_its_schema() {
    let messages = |schema: &str, text: &[u8]| -> Vec<String> {
        yaml::schema_findings(schema, text)
            .into_iter()
            .map(|found| found.message)
            .collect()
    };
    let valid = b"version: 2\nupdates:\n  - package-ecosystem: github-actions\n    directory: \
                  /\n    schedule:\n      interval: weekly\n";
    assert_eq!(messages(yaml::DEPENDABOT, valid), Vec::<String>::new());
    // A merge key brings its mapping in, as YAML 1.1 reads it, instead of
    // standing as a key of its own that the schema does not know.
    let merged = b"version: 2\nupdates:\n  - package-ecosystem: npm\n    directory: /\n    <<: \
                   &weekly\n      schedule:\n        interval: weekly\n  - package-ecosystem: \
                   github-actions\n    directory: /\n    <<: *weekly\n";
    assert_eq!(messages(yaml::DEPENDABOT, merged), Vec::<String>::new());
    assert_eq!(
        messages(yaml::DEPENDABOT, b"version: \"2\"\nupdates: []\n"),
        [
            "$.version: \"2\" is not of type \"integer\"",
            "$.version: 2 was expected"
        ]
    );
    assert_eq!(
        messages(yaml::DEPENDABOT, b""),
        ["$: null is not of type \"object\""]
    );
    // What YAML reads as one key, though the file writes it twice in two
    // ways, stops the loading.
    assert_eq!(
        messages(yaml::DEPENDABOT, b"true: 1\nTrue: 2\n"),
        ["not valid YAML: Boolean(true): duplicated key in mapping"]
    );
    assert_eq!(
        messages(yaml::DEPENDABOT, b"a: \xff\n"),
        ["not valid YAML: it is not UTF-8 (invalid utf-8 sequence of 1 bytes from index 3)"]
    );
    let refused = |schema: &str| messages(schema, b"a: 1\n").join("\n");
    assert!(
        refused("not json").starts_with("the schema does not read: "),
        "{}",
        refused("not json")
    );
    assert!(
        refused(r#"{"type": 1}"#).starts_with("the schema cannot be used: "),
        "{}",
        refused(r#"{"type": 1}"#)
    );
}

#[test]
fn merge_keys_are_applied_before_the_schema() {
    let messages = |text: &str| -> Vec<String> {
        let schema = r#"{"properties": {"m": {
            "properties": {
                "a": {"const": "own"}, "b": {"const": "first"},
                "c": {"const": "second"}, "d": {"const": "deep"}
            },
            "required": ["a", "b", "c", "d"],
            "additionalProperties": false
        }}}"#;
        yaml::schema_findings(schema, text.as_bytes())
            .into_iter()
            .map(|found| found.message)
            .collect()
    };
    // The mapping's own key wins over a merged one, the earlier mapping of a
    // list over the later one, and a merged mapping's own merge key counts.
    let text = "d: &d {d: deep}\nf: &f {a: first, b: first, <<: *d}\ns: &s {b: second, c: \
                second}\nm:\n  a: own\n  <<: [*f, *s]\n";
    assert_eq!(messages(text), Vec::<String>::new());
    // A merge key whose value is no mapping, nor a list of them, stays a key.
    for value in ["1", "[{a: own}, 1]"] {
        let found = messages(&format!(
            "m: {{a: own, b: first, c: second, d: deep, <<: {value}}}\n"
        ));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("'<<' was unexpected"), "{found:?}");
    }
}

#[test]
fn a_place_in_a_value_reads_as_a_json_path() {
    let location: jsonschema::paths::Location = [
        jsonschema::paths::LocationSegment::Property("updates".into()),
        jsonschema::paths::LocationSegment::Index(0),
        jsonschema::paths::LocationSegment::Property("package-ecosystem".into()),
    ]
    .into_iter()
    .collect();
    assert_eq!(yaml::json_path(&location), "$.updates[0].package-ecosystem");
    assert_eq!(yaml::json_path(&jsonschema::paths::Location::new()), "$");
}

#[test]
fn programs_only_some_files_need_are_looked_for_only_then() {
    let fixture = Fixture::new();
    fixture
        .computer("prettier", PRETTIER)
        .file("a.yml", "a: 1\n")
        .file(".github/workflows/ci.yml", "on: push\n");
    let (code, text) = fixture.run(Check::Yaml, &["a.yml"], true);
    assert_eq!(code, 0, "{text}");
    let (code, text) = fixture.run(Check::Yaml, &[".github/workflows/ci.yml"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::actionlint 1.7.12 is neither"),
        "{text}"
    );
}

#[test]
fn a_program_that_cannot_be_started_fails_the_check() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .computer("prettier", GARBAGE)
        .file("a.json", "{}\n")
        .file("b.jsonc", "{}\n");
    let (code, text) = fixture.run(Check::Json, &["a.json", "b.jsonc"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::Prettier cannot be started: "),
        "{text}"
    );
}

#[test]
fn without_a_folder_for_the_programs_nothing_runs() {
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
    let fixture = Fixture::new();
    let source = nebensound_repo::register::BUILTIN_REGISTER;
    let start = source.find("[program.markdownlint]").unwrap();
    let end = source.find("[program.prettier]").unwrap();
    let native = format!(
        "{}[program.markdownlint]\nversion = \"0.41.1\"\ncommand = \"markdownlint\"\n\n\
         [program.markdownlint.download.linux-x86_64]\nurl = \"https://example.invalid/m\"\n\
         digest = \"sha256:{}\"\nexecutable = \"markdownlint\"\n\n{}",
        &source[..start],
        "0".repeat(64),
        &source[end..]
    );
    let register = Register::parse(&native).unwrap();
    fixture
        .everything()
        .pinned("markdownlint", "0.41.1", "markdownlint", "#!/bin/sh\n")
        .file("README.md", "# x\n");
    let (code, text) = fixture.run_with(&register, Check::Markdown, &["README.md"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::markdownlint runs on Node.js, and the register gives it no runtime to run on",
    );
}

#[test]
fn markdownlint_gets_the_rules_of_each_file_and_every_problem_fails_the_check() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .file("README.md", "# x\n")
        .file(".github/PULL_REQUEST_TEMPLATE.md", "## x\n")
        .file("warned.md", "# x\n")
        .file(
            ".markdownlint-cli2.jsonc",
            "{ \"config\": { \"MD033\": false }, \"customRules\": [\"./rule.mjs\"], \
             \"frontMatter\": \"^---$\", \"noInlineConfig\": true, \
             \"overrides\": [{ \"filter\": [\".github/**\"], \"combine\": \"merge\", \
             \"config\": { \"MD041\": false } }] }",
        )
        .file("docs/.markdownlint.mjs", "export default {};\n");
    let files = [".github/PULL_REQUEST_TEMPLATE.md", "README.md", "warned.md"];
    let (code, text) = fixture.run(Check::Markdown, &files, true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "job: {\"frontMatter\":\"^---$\",\"noInlineConfig\":true,\"groups\":[{\"config\":\
         {\"MD013\":false,\"MD033\":false,\"MD041\":false},\"files\":[\".github/\
         PULL_REQUEST_TEMPLATE.md\"]},{\"config\":{\"MD013\":false,\"MD033\":false},\
         \"files\":[\"README.md\",\"warned.md\"]}]}",
    );
    // The lines of the rules report, printed by the check.
    for line in [
        "laid over the default rule by rule",
        "for `.github/**`: laid over the default and the rules above, rule by rule",
    ] {
        assert!(
            text.lines()
                .any(|found| found.starts_with("      .markdownlint-cli2.jsonc")
                    && found.ends_with(line)),
            "{line}: {text}"
        );
    }
    has(
        &text,
        "::error file=.markdownlint-cli2.jsonc::`customRules` is not handed to markdownlint: it \
         names modules of rules, which would be loaded and run, and the Markdown check runs no \
         code of the repository",
    );
    assert!(
        text.contains("::error file=docs/.markdownlint.mjs::not read: "),
        "{text}"
    );
    has(
        &text,
        "warned.md:2 warning MD009/no-trailing-spaces Trailing spaces",
    );
    has(
        &text,
        "::warning file=warned.md,line=2::MD009/no-trailing-spaces Trailing spaces",
    );
}

#[test]
fn a_warning_of_markdownlint_fails_nothing() {
    let fixture = Fixture::new();
    fixture.everything().file("warned.md", "# x\n");
    let (code, text) = fixture.run(Check::Markdown, &["warned.md"], true);
    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "::warning file=warned.md,line=2::MD009/no-trailing-spaces Trailing spaces",
    );
}

#[test]
fn a_front_matter_javascript_does_not_read_fails_at_its_file_and_the_rest_still_runs() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .file(".markdownlint-cli2.jsonc", "{ \"frontMatter\": \"(\" }")
        .file("heading.md", "# x\n");
    let (code, text) = fixture.run(Check::Markdown, &["heading.md"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=.markdownlint-cli2.jsonc::`frontMatter` is not a pattern JavaScript reads, \
         so it is not handed to markdownlint, which skips the front matter it knows by default: \
         Invalid regular expression: /(/u: Unterminated group",
    );
    has(
        &text,
        "::error file=heading.md,line=3::MD001/heading-increment Heading levels should only \
         increment by one level at a time [Expected: h2; Actual: h3]",
    );
}

#[test]
fn what_markdownlint_found_has_to_read() {
    let fixture = Fixture::new();
    fixture.everything().file("unreadable-heading.md", "# x\n");
    let (code, text) = fixture.run(Check::Markdown, &["unreadable-heading.md"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::what markdownlint found does not read: "),
        "{text}"
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
    // A line break in a name is written in the runner's own escape, as phase
    // 0 writes it (`console::property`).
    assert_eq!(
        run::annotation(Some("docs/a\nb.md"), None, "m"),
        "::error file=docs/a%0Ab.md::m"
    );
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
    assert_eq!(Check::of("shell"), None);
    for (check, name) in [
        (Check::Css, "css"),
        (Check::Dockerfile, "dockerfile"),
        (Check::Html, "html"),
        (Check::Json, "json"),
        (Check::Markdown, "markdown"),
        (Check::Python, "python"),
        (Check::Rust, "rust"),
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
        // A drive, which lychee reads as a path.
        ("a:b.md", "a:b.md"),
        (r"C:\repo/a.md", r"C:\repo/a.md"),
        (r"\\server\share/a.md", r"\\server\share/a.md"),
    ] {
        assert_eq!(markdown::lychee_line(file), line, "{file:?}");
    }
    // Absolute, the root in front: a glob character in the root is taken
    // literally as well.
    assert_eq!(
        markdown::lychee_lines(
            Path::new("/r[1]"),
            &["a.md".to_owned(), "b c.md".to_owned(), "#c.md ".to_owned()]
        ),
        "/r[[]1[]]/a.md\n/r[[]1[]]/b c.md\n/r[[]1[]]/#c.md[ ]\n"
    );
    // A Windows verbatim root loses its prefix, which would make every line
    // a glob naming no file.
    for (root, written) in [
        ("/r", "/r"),
        (r"\\?\C:\repo", r"C:\repo"),
        (r"\\?\UNC\server\share\repo", r"\\server\share\repo"),
    ] {
        assert_eq!(markdown::lychee_root(Path::new(root)), written, "{root:?}");
    }
    assert_eq!(
        markdown::lychee_lines(Path::new(r"\\?\C:\repo"), &["a.md".to_owned()]),
        "C:\\repo/a.md\n"
    );
}

#[test]
fn findings_of_markdownlint_and_lychee_are_taken_apart() {
    let finding: markdown::Lint = serde_json::from_str(
        "{\"lineNumber\":3,\"ruleNames\":[\"MD001\",\"heading-increment\"],\
         \"ruleDescription\":\"x\",\"errorDetail\":\"d\",\"errorContext\":\"c\",\
         \"errorRange\":[7,2],\"fixInfo\":null,\"severity\":\"error\"}",
    )
    .unwrap();
    assert_eq!(
        finding.message(),
        "MD001/heading-increment x [d] [Context: \"c\"]"
    );
    assert_eq!(finding.column(), Some(7));
    let bare: markdown::Lint = serde_json::from_str(
        "{\"lineNumber\":1,\"ruleNames\":[\"MD041\"],\"ruleDescription\":\"y\",\
         \"errorRange\":[]}",
    )
    .unwrap();
    assert_eq!(bare.message(), "MD041 y");
    assert_eq!(bare.column(), None);
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
    assert!(yaml::DEPENDABOT.contains("\"package-ecosystem\""));
}

#[test]
fn workflows_are_recognized() {
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
}

#[test]
fn the_call_refuses_what_it_does_not_run() {
    // Every call opens with the line that the gate decides, also one that
    // ends in a refusal.
    let opens = |text: &str| {
        assert_eq!(text.lines().next(), Some(run::OUTSIDE), "{text}");
    };
    let (text, code) = call(&["nebensound-repo", "run", "shell", "."]);
    assert_eq!(code, 2, "{text}");
    opens(&text);
    assert!(
        text.contains(
            "`run` carries out the checks css, dockerfile, html, json, markdown, python, rust, \
             toml and yaml; \"shell\" is not"
        ),
        "{text}"
    );
    let (text, code) = call(&["nebensound-repo", "run", "json", "/abs"]);
    assert_eq!(code, 2, "{text}");
    opens(&text);

    let broken = support::repository(&[
        (".nebensound/repo.toml", QUIET),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    let root = broken.path().to_str().unwrap();
    let (text, code) = call(&["nebensound-repo", "-C", root, "run", "json", "."]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        text,
        format!(
            "{}\nnothing is checked: phase 0 found errors, and `check` says which\n",
            run::OUTSIDE
        )
    );

    let fine = support::repository(&[(".nebensound/repo.toml", QUIET)]);
    let root = fine.path().to_str().unwrap();
    let (text, code) = call(&["nebensound-repo", "-C", root, "run", "json", "docs"]);
    assert_eq!(code, 1, "{text}");
    opens(&text);
    assert!(
        text.contains("the plan does not start the json check at docs"),
        "{text}"
    );
    let (text, code) = call(&["nebensound-repo", "-C", root, "run", "json", "."]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text,
        format!(
            "{}\nNo JSON file below '.'; there is nothing to check.\n",
            run::OUTSIDE
        )
    );
}

#[test]
fn the_binary_runs_a_check_with_the_programs_of_the_computer() {
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
    // A workflow run's log, as `GITHUB_ACTIONS` and a pipe make it, opens
    // with the check itself, in a stopped stretch.
    assert!(
        support::printed(&text).starts_with("2 TOML file(s) to check:\n"),
        "{text}"
    );
    assert!(text.starts_with("::stop-commands::"), "{text}");
    assert!(
        text.contains("::error file=unformatted.toml::does not parse"),
        "{text}"
    );
    assert_eq!(fs::read_dir(fixture.base.join("tmp")).unwrap().count(), 0);
}

#[test]
fn setup_takes_the_checks_it_is_named() {
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
    let fixture = Fixture::new();
    fixture.everything();
    for program in ["lychee", "actionlint"] {
        fs::remove_file(fixture.base.join("bin").join(program)).unwrap();
    }
    fixture
        .file("heading.md", "# x\n")
        .file(".github/workflows/ci.yml", "on: push\n")
        .file("duplicate.yaml", "name: a\nname: b\n");

    let (code, text) = fixture.run(Check::Markdown, &["heading.md"], true);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("::error::lychee "), "{text}");
    has(
        &text,
        "::error file=heading.md,line=3::MD001/heading-increment Heading levels should only \
         increment by one level at a time [Expected: h2; Actual: h3]",
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
        "::error file=duplicate.yaml,line=2,col=1::the key \"name\" stands twice in this \
         mapping: a mapping holds each key once",
    );
}

#[test]
fn actionlint_findings_are_escaped_and_other_output_is_kept() {
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
             --no-config -- styles/a.css styles/b.scss"
        ),
        "{prettier}"
    );
    // Stylelint runs on its runtime, from its pinned folder, with the
    // default compiled into the tool, by package, and an empty ignore file:
    // the plain stylesheet with the standard configuration, the SCSS file
    // with its counterpart and syntax as well.
    let folder = fixture.folder("stylelint", "18.0.0");
    has(
        &text,
        &format!("node --input-type=module --eval TOOLS={}", folder.display()),
    );
    has(
        &text,
        "stylelint job: {\"groups\":[\
         {\"extends\":[\"stylelint-config-standard\"],\"customSyntax\":null,\
         \"config\":{\"rules\":{}},\"files\":[\"styles/a.css\"]},\
         {\"extends\":[\"stylelint-config-standard\",\"stylelint-config-standard-scss\"],\
         \"customSyntax\":\"postcss-scss\",\"config\":{\"rules\":{}},\
         \"files\":[\"styles/b.scss\"]}]}",
    );
    has(&text, "nothing ignored: []");
    assert!(
        text.contains("Stylelint: the default, nothing laid over it"),
        "{text}"
    );
    // The script hands Stylelint a configuration object and the packages
    // by their place, and looks for no configuration of its own.
    for written in [
        "config,",
        "ignoreFiles: []",
        "disableDefaultIgnores: true",
        "group.extends.map(pinned)",
        "config.customSyntax = pinned(group.customSyntax)",
    ] {
        assert!(css::STYLELINT.contains(written), "{written}");
    }
    for absent in ["cosmiconfig", "configBasedir", "configFile", "explorer"] {
        assert!(!css::STYLELINT.contains(absent), "{absent}");
    }

    let (code, text) = fixture.run(Check::Css, &[], true);
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        text,
        "No stylesheet below '.'; there is nothing to check.\n"
    );
}

#[test]
fn the_css_check_hands_stylelint_the_rules_of_the_repository_and_refuses_what_loads_code() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .file(
            ".stylelintrc.json",
            "{ \"defaultSeverity\": \"error\", \"rules\": { \"color-hex-length\": \"long\" },\n\
             \"overrides\": [{ \"files\": [\"legacy/**\"], \"rules\": { \"color-named\": null } }],\n\
             \"plugins\": [\"./plugin.mjs\"] }\n",
        )
        .file("stylelint.config.mjs", "export default {};\n")
        .file("package.json", "{ \"stylelint\": { \"rules\": {} } }\n")
        .file("legacy/old.css", "a {}\n")
        .file("site.css", "a {}\n");
    let (code, text) = fixture.run(Check::Css, &["legacy/old.css", "site.css"], true);
    assert_eq!(code, 1, "{text}");
    // Every problem with the configuration is a finding at its file, and
    // the rest of it is handed over.
    has(
        &text,
        "::error file=.stylelintrc.json::`plugins` is not handed to Stylelint: it names modules \
         of rules, which would be loaded and run, and the CSS check runs no code of the \
         repository",
    );
    has(
        &text,
        "::error file=stylelint.config.mjs::not read: the tool reads the configuration of \
         stylelint only as data, from one file at the root of the repository (`.stylelintrc`, \
         `.stylelintrc.json`, `.stylelintrc.yaml`, `.stylelintrc.yml`), and never runs this one",
    );
    has(
        &text,
        "::error file=package.json::not read: the tool reads the configuration of stylelint \
         only as data, from one file at the root of the repository, and not from the \
         `stylelint` key of `package.json`",
    );
    has(
        &text,
        "stylelint job: {\"groups\":[\
         {\"extends\":[\"stylelint-config-standard\"],\"customSyntax\":null,\
         \"config\":{\"defaultSeverity\":\"error\",\
         \"rules\":{\"color-hex-length\":\"long\",\"color-named\":null}},\
         \"files\":[\"legacy/old.css\"]},\
         {\"extends\":[\"stylelint-config-standard\"],\"customSyntax\":null,\
         \"config\":{\"defaultSeverity\":\"error\",\"rules\":{\"color-hex-length\":\"long\"}},\
         \"files\":[\"site.css\"]}]}",
    );
    // The check prints what the rules report shows.
    assert!(
        text.contains("Stylelint: the default, with these laid over it"),
        "{text}"
    );
    assert!(
        text.contains("for `legacy/**`: laid over the default and the rules above, rule by rule"),
        "{text}"
    );
}

#[test]
fn every_kind_of_css_finding_is_reported() {
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
fn a_finding_a_script_hands_over_that_does_not_read_fails_the_check() {
    let fixture = Fixture::new();
    fixture.everything();
    fixture.file("malformed.css", "a {}\n");
    let (code, text) = fixture.run(Check::Css, &["malformed.css"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "::error::Stylelint handed over a finding that does not read (unknown variant `loud`"
        ),
        "{text}"
    );

    fixture.file("vanished.css", "a {}\n");
    let (code, text) = fixture.run(Check::Css, &["vanished.css"], true);
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(
            "::error::the findings of Stylelint cannot be read: stream did not contain valid \
             UTF-8"
        ),
        "{text}"
    );
}

#[test]
fn a_program_and_the_repository_print_only_inside_a_stopped_stretch() {
    let fixture = Fixture::new();
    fixture.everything();
    let files = ["a##[warning]x-unformatted.css", "styled.css"];
    for file in files {
        fixture.file(file, "a {}\n");
    }
    let (code, text) = fixture.log(Check::Css, &files, true);
    assert_eq!(code, 1, "{text}");
    // Every line outside a stretch is a command of the tool's own, and the
    // annotations follow what the programs printed.
    assert_eq!(
        support::outside(&text),
        [
            "::error file=a##[warning]x-unformatted.css::not formatted the way Prettier writes it",
            "::error file=styled.css,line=2,col=10::Expected \"#00aa77\" to be \"#0a7\" \
             (color-hex-length)",
        ],
        "{text}"
    );
    assert_eq!(text.matches("::stop-commands::").count(), 1, "{text}");
    assert!(
        text.contains("\n[warn] a##[warning]x-unformatted.css\n"),
        "{text}"
    );
}

#[test]
fn without_its_programs_the_css_check_fails() {
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
fn pages_are_formatted_carry_what_a_page_carries_and_link_somewhere() {
    let fixture = Fixture::new();
    fixture
        .everything()
        .computer("lychee", LYCHEE_PAGES)
        .file("index.html", PAGE)
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
             --no-config -- docs/header.htm index.html"
        ),
        "{prettier}"
    );
    // The page rules are the tool's own, and start no program.
    has(
        &text,
        "Page rules: every page has a title, a description, exactly one <h1>, and alternative \
         text on every image",
    );
    assert!(!text.contains("node "), "{text}");
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
    has(
        &text,
        &format!("lines: {}/docs/header.htm", fixture.root().display()),
    );
    // What `run` wrote for the programs is gone again.
    assert_eq!(fs::read_dir(&tmp).unwrap().count(), 0);

    let (code, text) = fixture.run(Check::Html, &[], true);
    assert_eq!(code, 0, "{text}");
    assert_eq!(text, "No HTML file below '.'; there is nothing to check.\n");
}

#[test]
fn every_kind_of_html_finding_is_reported() {
    let fixture = Fixture::new();
    fixture.everything().computer("lychee", LYCHEE_PAGES);
    let files = [
        "bare.html",
        "broken.html",
        "folder.html",
        "unformatted.html",
        "unreadable.htm",
    ];
    for file in files {
        fixture.file(file, "<p></p>\n");
    }
    fixture.file("bare.html", BARE);
    fs::remove_file(fixture.root().join("folder.html")).unwrap();
    fs::create_dir(fixture.root().join("folder.html")).unwrap();
    let (code, text) = fixture.run(Check::Html, &files, true);
    assert_eq!(code, 1, "{text}");
    for line in [
        "::error file=unformatted.html::not formatted the way Prettier writes it",
        "::error file=unreadable.htm::Prettier cannot read it: SyntaxError: Unexpected token (1:1)",
        // A page without what it must carry, each finding at its element.
        "::error file=bare.html,line=3,col=4::page/title: the page has no title: give it a \
         <title> element with text in it",
        "::error file=bare.html,line=3,col=4::page/description: the page has no description: \
         give it <meta name=\"description\" content=\"...\"> with text in the content",
        "::error file=bare.html,line=6,col=4::page/one-h1: the page has no <h1>, and a page has \
         exactly one",
        // The report stands in the output, and the broken link is a finding
        // at lychee's line and column.
        "* [ERROR] <file:///x/missing.html> (at 11:29) | File not found",
        "::error file=broken.html,line=11,col=29::[ERROR] <file:///x/missing.html> | File not \
         found",
        "config: ",
    ] {
        has(&text, line);
    }
    assert!(
        text.contains("::error file=folder.html::cannot be read: "),
        "{text}"
    );

    // A fragment is no page, and passes the page rules.
    let (code, text) = fixture.run(Check::Html, &["broken.html"], true);
    assert_eq!(code, 1, "{text}");
    assert!(!text.contains("page/"), "{text}");

    // Elsewhere, an annotation reads as a line that starts with the place.
    let (code, text) = fixture.run(Check::Html, &["bare.html"], false);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "bare.html:6:4: error: page/one-h1: the page has no <h1>, and a page has exactly one",
    );
}

#[test]
fn links_from_the_root_of_a_site_are_left_out_below_the_folder_without_links() {
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
    let fixture = Fixture::new();
    fixture
        .computer("prettier", PRETTIER)
        .file("unformatted.html", "<p></p>\n")
        .file("bare.html", BARE);
    let (code, text) = fixture.run(Check::Html, &["bare.html", "unformatted.html"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::lychee 0.24.2 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    // The other parts still run.
    has(
        &text,
        "::error file=unformatted.html::not formatted the way Prettier writes it",
    );
    has(
        &text,
        "::error file=bare.html,line=6,col=4::page/one-h1: the page has no <h1>, and a page has \
         exactly one",
    );

    fs::remove_file(fixture.base.join("bin/prettier")).unwrap();
    fixture
        .computer("lychee", LYCHEE_PAGES)
        .file("index.html", PAGE);
    let (code, text) = fixture.run(Check::Html, &["index.html"], true);
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::prettier 3.9.6 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    has(&text, "root: an empty folder");
    assert!(!text.contains("page/"), "{text}");

    // An unreadable lychee.toml stops the link check before it runs.
    fixture.file("lychee.toml", "x = 1\n");
    let config = fixture.root().join("lychee.toml");
    fs::set_permissions(&config, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&config).is_err() {
        let (code, text) = fixture.run(Check::Html, &["index.html"], true);
        assert_eq!(code, 1, "{text}");
        assert!(
            text.contains("::error file=lychee.toml::cannot be read: "),
            "{text}"
        );
        assert!(!text.contains("lychee --"), "{text}");
    }
}
