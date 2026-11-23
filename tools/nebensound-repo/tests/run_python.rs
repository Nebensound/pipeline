//! `nebensound-repo run python`: the Python check, run from the tool.
//!
//! No real uv, Ruff, Pyright or Python runs here. Each is a shell script that
//! notes how it was called and acts the way the fixture tells it: a call
//! fails when a line of the fixture's `fail` file is part of its arguments,
//! Ruff and Pyright print the reports the fixture wrote, `uv run` leaves the
//! test report and the coverage data the fixture wrote where the check reads
//! them, and `uv build` leaves the wheel the fixture built. That the real
//! programs find what the block found is shown by the cases of the block,
//! which `cargo xtask check --all` runs through `run`. Hence Unix only: the
//! gate runs on Linux.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs::{self, File};
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use nebensound_repo::Register;
use nebensound_repo::names::ProgramName;
use nebensound_repo::path::RelativePath;
use nebensound_repo::programs::{Algorithm, Digest, Platform};
use nebensound_repo::repository::Repository;
use nebensound_repo::run::python::{self, coverage, pyright, pytest, ruff, wheel};
use nebensound_repo::run::{self, Check, Context, Packages};
use nebensound_repo::setup::{Environment, fetch, find};
use serde_json::{Value, json};
use tempfile::TempDir;

/// Held by every test of this file that starts programs: Linux refuses to
/// start a file another process holds open for writing ("Text file busy").
static ALONE: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ALONE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What every program starts with: where the fixture is, a note of the call,
/// and whether the call fails.
const HEAD: &str = r#"#!/bin/sh
here=@HERE@
echo "$(basename "$0") $* in $(pwd -P)" >> "$here/calls"
failing() {
  [ -f "$here/fail" ] || return 1
  while IFS= read -r pattern; do
    [ -n "$pattern" ] || continue
    case "$1" in *"$pattern"*) return 0 ;; esac
  done < "$here/fail"
  return 1
}
"#;

const UV: &str = r#"
echo "UV_PROJECT_ENVIRONMENT=$UV_PROJECT_ENVIRONMENT UV_PYTHON_PREFERENCE=$UV_PYTHON_PREFERENCE PYTHONPATH=${PYTHONPATH-unset}" >> "$here/calls"
failing "uv $*" && { echo "error: uv failed" >&2; exit 2; }
case "$1 $2" in
  "--version ") echo "uv 0.13.9" ;;
  "python install") echo "Installed Python 3.14.0" ;;
  "python find")
    [ -f "$here/lock-folder" ] && chmod 555 "$(dirname "$UV_CACHE_DIR")"
    cat "$here/found" ;;
  "sync --locked")
    mkdir -p "$UV_PROJECT_ENVIRONMENT/bin"
    cp "$here/bin/python" "$UV_PROJECT_ENVIRONMENT/bin/python" ;;
  "run --no-sync")
    case "$*" in
      *" coverage run "*)
        [ -f "$here/pytest-out" ] && cat "$here/pytest-out"
        for arg in "$@"; do
          case "$arg" in
            --junit-xml=*) [ -f "$here/junit.xml" ] && cp "$here/junit.xml" "${arg#--junit-xml=}" ;;
            --data-file=*)
              data="${arg#--data-file=}"
              [ -f "$here/coverage-data" ] && cp "$here/coverage-data" "$data"
              [ -f "$here/parts" ] && cp "$here/parts" "$data.part" ;;
          esac
        done
        [ -f "$here/pytest-status" ] && exit "$(cat "$here/pytest-status")"
        exit 0 ;;
      *" coverage combine "*)
        for arg in "$@"; do
          case "$arg" in --data-file=*) cat "$here/parts" > "${arg#--data-file=}" ;; esac
        done ;;
      *" coverage json "*)
        last=""
        for arg in "$@"; do last="$arg"; done
        [ -f "$here/coverage.json" ] && cp "$here/coverage.json" "$last"
        echo "Wrote JSON report to $last" ;;
    esac ;;
  "export --frozen")
    for arg in "$@"; do last="$arg"; done
    echo "pytest==9.2" > "$last"
    [ -f "$here/corrupt" ] && echo "[" > pyproject.toml
    [ -f "$here/unreadable" ] && rm pyproject.toml && mkdir pyproject.toml
    [ -f "$here/read-only-early" ] && chmod 444 pyproject.toml ;;
  "build --wheel")
    cp pyproject.toml "$here/built"
    [ -f "$here/read-only" ] && chmod 444 pyproject.toml
    mkdir -p "$4"
    [ -f "$here/wheel.whl" ] && cp "$here/wheel.whl" "$4/sample-0.0.0-py3-none-any.whl"
    echo "Successfully built" ;;
  "venv --quiet")
    mkdir -p "$5/bin"
    cp "$here/bin/python" "$5/bin/python"
    for name in $(cat "$here/commands"); do
      printf '#!/bin/sh\necho "%s $*"\nexit %s\n' "$name" "$(cat "$here/status-$name" 2>/dev/null || echo 0)" > "$5/bin/$name"
      chmod 755 "$5/bin/$name"
    done ;;
esac
exit 0
"#;

const PYTHON: &str = r#"
if [ "$1" = "--version" ]; then cat "$here/version"; exit 0; fi
case "$3" in
  *find_spec*) [ -f "$here/no-pytest" ] && exit 1; exit 0 ;;
  *ast.parse*) grep -qxF "$4" "$here/empty" 2>/dev/null && exit 0; exit 1 ;;
  *import_module*)
    echo "importing $4"
    grep -qxF "$4" "$here/broken" 2>/dev/null && exit 1
    exit 0 ;;
esac
exit 0
"#;

const RUFF: &str = r#"
[ "$1" = "--version" ] && { echo "ruff 0.17.6"; [ -f "$here/ruff-goes" ] && chmod 000 "$0"; exit 0; }
config=""
previous=""
for arg in "$@"; do
  [ "$previous" = "--config" ] && config="$arg"
  previous="$arg"
done
cat "$config" > "$here/ruff-config"
echo "ruff warns on standard error" >&2
cat "$here/ruff-$1.json" 2>/dev/null || echo "[]"
exit "$(cat "$here/ruff-$1-status" 2>/dev/null || echo 0)"
"#;

const PYRIGHT: &str = r#"
[ "$1" = "--version" ] && { echo "pyright 1.1.414"; exit 0; }
cp "$3" "$here/pyright-config"
echo "pyright says something on standard error" >&2
# Pyright passes over a file in a folder whose name starts with a dot.
analysed=$(sed -n 's/.*"include":\[\([^]]*\)\].*/\1/p' "$3" | grep -o '"[^"]*"' | grep -cv '\(^"\|/\)\.')
cat "$here/pyright.json" 2>/dev/null || echo '{"generalDiagnostics": [], "summary": {"filesAnalyzed": '"$analysed"', "errorCount": 0, "warningCount": 0}}'
exit "$(cat "$here/pyright-status" 2>/dev/null || echo 0)"
"#;

fn executable(file: &Path, script: &str) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, script).unwrap();
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

fn path(text: &str) -> RelativePath {
    RelativePath::new(text).unwrap()
}

/// A wheel with these files, each with its content.
fn zipped(file: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(File::create(file).unwrap());
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, content) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(content).unwrap();
    }
    zip.finish().unwrap();
}

/// The wheel of the covered sample: a package and its command.
fn sample_wheel(file: &Path) {
    zipped(
        file,
        &[
            ("sample/__init__.py", b""),
            ("sample/cli.py", b""),
            ("sample-0.0.0.dist-info/METADATA", b"Name: sample\n"),
            (
                "sample-0.0.0.dist-info/entry_points.txt",
                b"[console_scripts]\nsample = sample.cli:main\n",
            ),
        ],
    );
}

const PYPROJECT: &str = "[project]\nname = \"sample\"\nversion = \"0.0.0\"\nrequires-python = \">=3.14\"\ndependencies = []\n\n[project.scripts]\nsample = \"sample.cli:main\"\n\n[build-system]\nrequires = [\"uv_build\"]\nbuild-backend = \"uv_build\"\n";

/// A sample like the covered case of the block, its programs and what it is
/// handed.
struct Fixture {
    base: TempDir,
    unit: &'static str,
    files: Vec<String>,
    tracked: Vec<String>,
    register: Register,
    temp: PathBuf,
    workflow: bool,
}

impl Fixture {
    fn new() -> Self {
        let base = tempfile::tempdir().unwrap();
        let here = fs::canonicalize(base.path()).unwrap();
        let bin = here.join("bin");
        for (name, script) in [
            ("uv", UV),
            ("ruff", RUFF),
            ("pyright", PYRIGHT),
            ("python", PYTHON),
        ] {
            let head = HEAD.replace("@HERE@", &here.to_string_lossy());
            executable(&bin.join(name), &format!("{head}{script}"));
        }
        for dir in ["tmp", "cache", "repo", "utilities", "serve"] {
            fs::create_dir_all(here.join(dir)).unwrap();
        }
        // The utilities the programs above use, and nothing else of the
        // computer, so that a program a test takes away is not found
        // elsewhere.
        let computer = std::env::var_os("PATH").unwrap_or_default();
        for utility in [
            "basename", "cat", "chmod", "cp", "curl", "dirname", "grep", "mkdir", "rm", "sed",
        ] {
            let found = find::search(utility, &computer, false).unwrap();
            std::os::unix::fs::symlink(found, here.join("utilities").join(utility)).unwrap();
        }
        // The wheel of coverage.py the register pins, served from a file.
        let served = here.join("serve/coverage-7.17.4-py3-none-any.whl");
        fs::write(&served, "the wheel of coverage.py").unwrap();
        let digest = fetch::digest_of(&served, Algorithm::Sha256).unwrap();
        let mut register = Register::builtin().unwrap();
        let program = register
            .program
            .get_mut(&ProgramName::new("coverage").unwrap())
            .unwrap();
        for download in program.download.values_mut() {
            download.url = format!("file://{}", served.display());
            download.digest = Digest::new(&format!("sha256:{digest}")).unwrap();
        }
        let fixture = Self {
            base,
            unit: ".",
            files: [
                "src/sample/__init__.py",
                "src/sample/cli.py",
                "src/sample/stub.pyi",
                "tests/test_cli.py",
            ]
            .map(str::to_owned)
            .to_vec(),
            tracked: [
                ".python-version",
                "pyproject.toml",
                "src/sample/__init__.py",
                "src/sample/cli.py",
                "src/sample/legacy.py",
                "src/sample/stub.pyi",
                "tests/test_cli.py",
                "uv.lock",
            ]
            .map(str::to_owned)
            .to_vec(),
            register,
            temp: here.join("tmp"),
            workflow: true,
        };
        fixture
            .file("pyproject.toml", PYPROJECT)
            .file("uv.lock", "version = 1\n")
            .file(".python-version", "3.14\n")
            .file(
                "src/sample/__init__.py",
                "\"\"\"A sample.\"\"\"\n# type: ignore[import]\n",
            )
            .file("src/sample/cli.py", "def main() -> int:\n    return 0\n")
            .file("src/sample/legacy.py", "x = 1\n")
            .file("src/sample/stub.pyi", "")
            .file("tests/test_cli.py", "def test() -> None:\n    pass\n")
            .mark("found", &format!("{}\n", bin.join("python").display()))
            .mark("version", "Python 3.14.0\n")
            .mark("coverage-data", "data")
            .mark("commands", "sample\n")
            .mark(
                "coverage.json",
                &json!({"files": {
                    "src/sample/__init__.py": {"missing_lines": [], "missing_branches": []},
                    "src/sample/cli.py": {"missing_lines": [], "missing_branches": []},
                    "tests/test_cli.py": {"missing_lines": [], "missing_branches": []}
                }})
                .to_string(),
            );
        sample_wheel(&here.join("wheel.whl"));
        fixture
    }

    fn here(&self) -> PathBuf {
        fs::canonicalize(self.base.path()).unwrap()
    }

    fn root(&self) -> PathBuf {
        self.here().join("repo")
    }

    /// Writes a file of the repository.
    fn file(&self, name: &str, content: &str) -> &Self {
        let file = self.root().join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
        self
    }

    /// Writes a file that tells the programs what to do.
    fn mark(&self, name: &str, content: &str) -> &Self {
        fs::write(self.here().join(name), content).unwrap();
        self
    }

    /// Removes a file that tells the programs what to do.
    fn unmark(&self, name: &str) -> &Self {
        fs::remove_file(self.here().join(name)).unwrap();
        self
    }

    /// Makes the calls that hold one of these fail.
    fn fail(&self, patterns: &[&str]) -> &Self {
        self.mark("fail", &format!("{}\n", patterns.join("\n")))
    }

    /// What a program noted.
    fn noted(&self, name: &str) -> String {
        fs::read_to_string(self.here().join(name)).unwrap_or_default()
    }

    fn env(&self) -> Environment {
        Environment {
            computer: "linux-x86_64".to_owned(),
            platform: Some(Platform::LinuxX86_64),
            windows: false,
            search_path: std::env::join_paths([
                self.here().join("bin"),
                self.here().join("utilities"),
            ])
            .unwrap(),
            folder: Some(self.here().join("cache")),
            protocols: "=file".to_owned(),
            answers: None,
        }
    }

    /// Runs the check: its exit code and what it printed.
    fn run(&self) -> (u8, String) {
        let env = self.env();
        let root = self.root();
        let repository = Repository::at(&root);
        let tracked: Vec<RelativePath> = self.tracked.iter().map(|file| path(file)).collect();
        let context = Context {
            register: &self.register,
            env: &env,
            root: &root,
            temp: &self.temp,
            workflow: self.workflow,
            packages: Packages {
                repository: &repository,
                tracked: &tracked,
                excluded: &[],
                orders: &[],
            },
        };
        let files: Vec<RelativePath> = self.files.iter().map(|file| path(file)).collect();
        let refs: Vec<&RelativePath> = files.iter().collect();
        let mut out = Vec::new();
        let code = run::run(&context, Check::Python, &path(self.unit), &refs, &mut out);
        (code, String::from_utf8(out).unwrap())
    }
}

fn has(text: &str, line: &str) {
    assert!(
        text.lines().any(|found| found == line),
        "no line {line:?} in\n{text}"
    );
}

fn starts(text: &str, start: &str) {
    assert!(
        text.lines().any(|found| found.starts_with(start)),
        "no line starts with {start:?} in\n{text}"
    );
}

fn lacks(text: &str, part: &str) {
    assert!(!text.contains(part), "{part:?} in\n{text}");
}

fn groups(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.strip_prefix("::group::"))
        .collect()
}

const ALL: [&str; 9] = [
    "Find uv, Ruff, Pyright and coverage.py",
    "Install the project's Python",
    "Install the project",
    "Every Python file is formatted",
    "Every Python file passes Ruff",
    "Every Python file passes Pyright in strict mode",
    "The tests pass",
    "Every line and branch is covered",
    "The wheel installs and runs",
];

#[test]
fn every_step_runs_in_its_order_and_hands_no_tool_an_excluded_file() {
    let _alone = alone();
    let fixture = Fixture::new();
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    assert_eq!(groups(&text), ALL);
    has(&text, "4 Python file(s) to check:");
    has(
        &text,
        "Taken out by an [[exclude]] entry, and handed to no tool:",
    );
    has(&text, "src/sample/legacy.py");
    has(&text, "uv 0.13.9");
    has(&text, "ruff 0.17.6");
    has(&text, "pyright 1.1.414");
    has(&text, "The uv workspace: .");
    has(&text, "The interpreter: 3.14, as .python-version names it");
    has(&text, "Python 3.14.0");
    has(&text, "Ruff's rules: the pipeline default");
    has(&text, "Every file is formatted the way Ruff writes it.");
    has(&text, "ruff warns on standard error");
    has(&text, "No finding.");
    has(&text, "Pyright: strict mode");
    has(&text, "pyright says something on standard error");
    has(&text, "Pyright: 4 file(s), 0 error(s), 0 warning(s)");
    has(&text, "Every line and branch of 3 file(s) is covered.");
    has(&text, "The wheel gets the placeholder version 0.0.0.");
    has(&text, "Built sample-0.0.0-py3-none-any.whl");
    has(&text, "$ sample --help");
    has(&text, "sample --help");
    has(&text, "$ python -c 'import sample'");
    has(&text, "importing sample");
    lacks(&text, ": failed");
    let root = fixture.root();
    let calls = fixture.noted("calls");
    // uv keeps everything in the check's folder, and no variable of the
    // computer steers the Python.
    assert!(
        calls.contains("UV_PYTHON_PREFERENCE=only-managed PYTHONPATH=unset"),
        "{calls}"
    );
    assert!(calls.contains("uv python install --no-bin in"), "{calls}");
    assert!(
        calls.contains(
            "ruff format --check --no-cache --no-force-exclude --output-format json --config"
        ),
        "{calls}"
    );
    assert!(
        calls.contains(&format!(
            "-- src/sample/__init__.py src/sample/cli.py src/sample/stub.pyi tests/test_cli.py \
             in {}\n",
            root.display()
        )),
        "{calls}"
    );
    assert!(calls.contains("ruff check --no-fix --no-cache"), "{calls}");
    assert!(
        calls.contains(&format!("--ignore={}/src/sample/legacy.py", root.display())),
        "{calls}"
    );
    assert!(
        calls.contains("-m pytest -p no:cacheprovider -o junit_family=xunit1 --junit-xml="),
        "{calls}"
    );
    assert!(
        calls.contains(
            "uv export --frozen --no-default-groups --no-emit-project --no-editable --no-header \
             --output-file"
        ),
        "{calls}"
    );
    assert!(calls.contains("uv pip install --quiet --python"), "{calls}");
    // The default lies beneath, and the project's lowest Python is the target.
    let config = fixture.noted("ruff-config");
    assert!(config.contains("target-version = \"py314\""), "{config}");
    assert!(config.contains("ruff-default.toml"), "{config}");
    // Pyright gets the list, the environment and the interpreter's version.
    let pyright: Value = serde_json::from_str(&fixture.noted("pyright-config")).unwrap();
    assert_eq!(pyright["typeCheckingMode"], "strict");
    assert_eq!(pyright["include"][0], "src/sample/__init__.py");
    assert_eq!(pyright["ignore"], json!([]));
    assert_eq!(pyright["venv"], "environment");
    assert_eq!(pyright["pythonVersion"], "3.14");
    // The wheel is built with the placeholder, and the manifest put back.
    let built = fixture.noted("built");
    assert!(
        built.contains("version = \"0.0.0\"\ndynamic = []"),
        "{built}"
    );
    assert_eq!(
        fs::read_to_string(root.join("pyproject.toml")).unwrap(),
        PYPROJECT
    );
    // The configuration Pyright ran with is gone again.
    let left: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !left.iter().any(|name| name.starts_with(pyright::RUN)),
        "{left:?}"
    );
}

#[test]
fn outside_a_workflow_run_findings_are_plain_lines() {
    let _alone = alone();
    let mut fixture = Fixture::new();
    fixture.workflow = false;
    fixture.mark(
        "ruff-check.json",
        &json!([{"filename": "src/sample/cli.py", "location": {"row": 1, "column": 5}, "code": null, "message": "a message"}])
            .to_string(),
    );
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "src/sample/cli.py:1:5: error: Ruff a message");
    has(&text, "Every Python file passes Ruff: failed");
    lacks(&text, "::group::");
}

#[test]
fn a_unit_without_a_tracked_manifest_or_without_a_python_file() {
    let _alone = alone();
    let mut fixture = Fixture::new();
    fixture.tracked.retain(|file| file != "pyproject.toml");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::check-python checks a uv project, and '.', the unit's path, holds no \
         pyproject.toml that Git tracks; write one there (uv init), or exclude the Python files \
         with an [[exclude]] entry and a reason",
    );
    assert!(groups(&text).is_empty(), "{text}");

    let mut nothing = Fixture::new();
    nothing.files.clear();
    let (code, text) = nothing.run();
    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "No Python file below '.'; there is nothing to check.",
    );
}

#[test]
fn a_unit_below_the_root_finds_its_workspace_and_its_configurations_above_it() {
    let _alone = alone();
    let mut fixture = Fixture::new();
    fixture.unit = "service";
    fixture.files = vec!["service/app.py".to_owned()];
    fixture.tracked = [
        "service/pyproject.toml",
        "service/app.py",
        "uv.lock",
        ".python-version",
    ]
    .map(str::to_owned)
    .to_vec();
    fixture
        .file("service/pyproject.toml", PYPROJECT)
        .file("service/app.py", "x = 1\n")
        .file("ruff.toml", "extend = \"base/ruff.toml\"\nline-length = 90\n")
        .file("base/ruff.toml", "[lint]\nignore = [\"D100\"]\n")
        .file(
            "pyproject.toml",
            "[tool.pyright]\nreportMissingImports = false\n",
        )
        .mark(
            "coverage.json",
            &json!({"files": {"app.py": {"missing_lines": [], "missing_branches": []}}})
                .to_string(),
        )
        .mark(
            "ruff-format.json",
            &json!([{"filename": "service/app.py", "location": {"row": 2, "column": 1}, "code": "unformatted", "message": "Would reformat"}])
                .to_string(),
        )
        .mark("ruff-format-status", "1");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "The uv workspace: .");
    has(&text, "The interpreter: 3.14, as .python-version names it");
    has(
        &text,
        "Ruff's rules: the pipeline default, and laid over it ruff.toml",
    );
    has(
        &text,
        "Pyright: strict mode, with pyproject.toml laid over it",
    );
    has(&text, "service/app.py:2:1: unformatted Would reformat");
    has(
        &text,
        "::error file=service/app.py,line=2,col=1::not formatted the way Ruff writes it; 'ruff \
         format' fixes it",
    );
    has(&text, "Every line and branch of 1 file(s) is covered.");
    let calls = fixture.noted("calls");
    assert!(
        calls.contains(&format!(
            "-- service/app.py in {}\n",
            fixture.root().display()
        )),
        "{calls}"
    );
    let config = fixture.noted("ruff-config");
    assert!(config.contains("line-length = 90"), "{config}");
    assert!(config.contains("target-version = \"py314\""), "{config}");
    let pyright: Value = serde_json::from_str(&fixture.noted("pyright-config")).unwrap();
    assert_eq!(pyright["include"][0], "service/app.py");
    assert_eq!(pyright["reportMissingImports"], false);
}

#[test]
fn the_programs_have_to_be_there() {
    let _alone = alone();
    let fixture = Fixture::new();
    fs::remove_file(fixture.here().join("bin/pyright")).unwrap();
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::pyright 1.1.414 is neither pinned in the tool's folder nor on the computer; \
         `nebensound-repo setup` installs it",
    );
    has(&text, "Find uv, Ruff, Pyright and coverage.py: failed");
    assert_eq!(groups(&text), ALL[..1]);

    let fixture = Fixture::new();
    fixture.fail(&["uv --version"]);
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    assert_eq!(groups(&text), ALL[..1]);

    let mut fixture = Fixture::new();
    for download in fixture
        .register
        .program
        .get_mut(&ProgramName::new("coverage").unwrap())
        .unwrap()
        .download
        .values_mut()
    {
        download.digest = Digest::new(&format!("sha256:{}", "0".repeat(64))).unwrap();
    }
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error::coverage.py 7.17.4 cannot be fetched: the download from file://",
    );

    let mut fixture = Fixture::new();
    fixture
        .register
        .program
        .remove(&ProgramName::new("coverage").unwrap());
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::the register pins no wheel of coverage.py for the python check",
    );
}

#[test]
fn the_interpreter_comes_from_a_pin_beside_a_lockfile() {
    let _alone = alone();
    let fixture = Fixture::new();
    fs::remove_file(fixture.root().join("uv.lock")).unwrap();
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=pyproject.toml::uv finds no uv.lock at '.' or above it: check-python \
         installs the project as its lockfile pins it (uv sync --locked); write it with 'uv \
         lock' and commit it",
    );
    assert_eq!(groups(&text), ALL[..2]);

    let fixture = Fixture::new();
    fs::remove_file(fixture.root().join(".python-version")).unwrap();
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=pyproject.toml::neither '.' nor the root of its uv workspace holds a \
         .python-version: it names the interpreter check-python installs, as \
         rust-toolchain.toml names the toolchain of check-rust; write it with 'uv python pin' \
         and commit it",
    );

    for failing in ["uv python install", "uv python find"] {
        let fixture = Fixture::new();
        fixture.fail(&[failing]);
        let (code, text) = fixture.run();
        assert_eq!(code, 1, "{text}");
        has(&text, "Install the project's Python: failed");
        assert_eq!(groups(&text), ALL[..2]);
    }

    let fixture = Fixture::new();
    fixture.mark("found", "/nonexistent/python\n");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    starts(&text, "::error::python cannot be started:");
    has(&text, "Install the project's Python: failed");

    let fixture = Fixture::new();
    fixture.mark("version", "Python 3.10.12\n");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=.python-version::check-python measures coverage with coverage.py 7.17.4, \
         which runs on Python 3.11.0 or later, and this names Python 3.10.12",
    );
}

#[test]
fn ruff_configurations_that_cannot_be_laid_over_the_default() {
    let _alone = alone();
    for (files, finding) in [
        (
            vec![("ruff.toml", "extend = \"missing.toml\"\n")],
            "::error file=ruff.toml::Ruff's `extend` names missing.toml, which does not exist",
        ),
        (
            vec![("ruff.toml", "extend = \"${NEBENSOUND_NOT_SET}/r.toml\"\n")],
            "::error file=ruff.toml::Ruff's `extend` names ${NEBENSOUND_NOT_SET}/r.toml, which \
             does not exist",
        ),
        (
            vec![("ruff.toml", "extend = 1\n")],
            "::error file=ruff.toml::Ruff's `extend` names 1, which does not exist",
        ),
        (
            vec![
                ("ruff.toml", "extend = \"other.toml\"\n"),
                ("other.toml", "extend = \"ruff.toml\"\n"),
            ],
            "::error file=other.toml::Ruff's `extend` comes back round to ruff.toml",
        ),
        (
            vec![(".ruff.toml", "not toml")],
            "::error file=.ruff.toml::Ruff's configuration cannot be read: ",
        ),
        (
            vec![
                ("ruff.toml", "extend = \"broken.toml\"\n"),
                ("broken.toml", "["),
            ],
            "::error file=broken.toml::Ruff's configuration cannot be read: ",
        ),
    ] {
        let fixture = Fixture::new();
        for (name, content) in &files {
            fixture.file(name, content);
        }
        let (code, text) = fixture.run();
        assert_eq!(code, 1, "{text}");
        starts(&text, finding);
        has(&text, "Install the project's Python: failed");
    }

    let fixture = Fixture::new();
    fixture.file("ruff.toml", "");
    let file = fixture.root().join("ruff.toml");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    let (code, text) = fixture.run();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error file=ruff.toml::Ruff's configuration cannot be read: ",
    );

    // The project's own pyproject.toml, with a target and an extend that
    // names a pyproject.toml without Ruff's table.
    let fixture = Fixture::new();
    fixture
        .file(
            "pyproject.toml",
            &format!(
                "{PYPROJECT}\n[tool.ruff]\nextend = \"base/pyproject.toml\"\ntarget-version = \
                 \"py313\"\n"
            ),
        )
        .file("base/pyproject.toml", "[project]\nname = \"base\"\n");
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "Ruff's rules: the pipeline default, and laid over it pyproject.toml",
    );
    let config = fixture.noted("ruff-config");
    assert!(config.contains("target-version = \"py313\""), "{config}");
}

#[test]
fn ruff_configuration_that_cannot_be_written() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.mark("lock-folder", "");
    let (code, text) = fixture.run();
    for entry in fs::read_dir(&fixture.temp).unwrap() {
        fs::set_permissions(entry.unwrap().path(), fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error::check-python cannot write Ruff's configuration: ",
    );
}

#[test]
fn ruff_that_reports_nothing_readable_fails_by_name() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture
        .mark("ruff-format.json", "not json")
        .mark("ruff-check-status", "2");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "not json");
    has(
        &text,
        "::error::Ruff's formatter could not check the files (it ended with exit status 0); its \
         message stands in the log",
    );
    has(
        &text,
        "::error::Ruff could not lint the files (it ended with exit status 2); its message \
         stands in the log",
    );

    let fixture = Fixture::new();
    fixture
        .mark(
            "ruff-format.json",
            &json!([{"filename": format!("{}/src/sample/cli.py", fixture.root().display()), "location": {"row": 3, "column": 1}, "code": null, "message": "Failed to parse"}])
                .to_string(),
        )
        .mark("ruff-format-status", "2")
        .mark(
            "ruff-check.json",
            &json!([{"filename": "src/sample/cli.py", "location": {"row": 9, "column": 12}, "code": "UP031", "message": "Use format specifiers"}])
                .to_string(),
        )
        .mark("ruff-check-status", "1");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "src/sample/cli.py:3:1: error Failed to parse");
    has(
        &text,
        "::error file=src/sample/cli.py,line=3,col=1::Ruff cannot format it: Failed to parse",
    );
    has(
        &text,
        "::error file=src/sample/cli.py,line=9,col=12::Ruff UP031: Use format specifiers",
    );

    // An exit status alone fails the formatter, without a finding.
    let fixture = Fixture::new();
    fixture.mark("ruff-format-status", "1");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "Every Python file is formatted: failed");

    // Ruff answers for its version, and then cannot be started.
    let fixture = Fixture::new();
    fixture.mark("ruff-goes", "");
    let (code, text) = fixture.run();
    fs::set_permissions(
        fixture.here().join("bin/ruff"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert_eq!(code, 1, "{text}");
    starts(&text, "::error::Ruff cannot be started:");
    has(&text, "Every Python file passes Ruff: failed");
}

#[test]
fn the_project_that_does_not_install_leaves_out_what_needs_it() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.fail(&["uv sync"]);
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=pyproject.toml::uv cannot install the project as its uv.lock pins it (uv \
         sync --locked); its message stands in the log",
    );
    assert_eq!(groups(&text), ALL[..5]);
}

#[test]
fn pyright_lowered_and_its_findings() {
    let _alone = alone();
    let fixture = Fixture::new();
    let root = fixture.root();
    fixture
        .file(
            "pyrightconfig.json",
            "{\n  // a comment\n  \"typeCheckingMode\": \"basic\",\n  \"pythonVersion\": \"3.12\",\n}\n",
        )
        .file(
            "src/sample/cli.py",
            "# pyright: basic\ndef main() -> int:\n    return 0\n",
        )
        .file(
            "tests/test_cli.py",
            "# noqa # type: ignore\ndef test() -> None:\n    pass\n",
        )
        .mark(
            "pyright.json",
            &json!({
                "generalDiagnostics": [
                    {"file": format!("{}/src/sample/cli.py", root.display()), "severity": "error", "message": "Type is wrong", "rule": "reportReturnType", "range": {"start": {"line": 8, "character": 11}}},
                    {"file": format!("{}/src/sample/gone.py", root.display()), "severity": "warning", "message": "Import cycle", "range": {"start": {"line": 0, "character": 0}}},
                    {"severity": "information", "message": "Something to know"}
                ],
                "summary": {"filesAnalyzed": 4, "errorCount": 1, "warningCount": 1}
            })
            .to_string(),
        );
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "Pyright: strict mode, with pyrightconfig.json laid over it",
    );
    has(
        &text,
        "::error file=pyrightconfig.json::typeCheckingMode \"basic\" lowers Pyright's strict \
         mode, which check-python holds every file to; remove it, and switch single rules off \
         where they have to be",
    );
    has(
        &text,
        "::error file=src/sample/cli.py,line=1::'# pyright: basic' lowers Pyright's strict mode \
         for this file, which check-python holds every file to; remove it, and switch single \
         rules off where they have to be ('# pyright: reportX=false')",
    );
    has(
        &text,
        "::error file=tests/test_cli.py,line=1::'# type: ignore' before the first statement \
         takes the whole file out of Pyright, which check-python holds every file to; remove \
         it, and switch single rules off where they have to be ('# pyright: ignore[reportX]' \
         on the line)",
    );
    has(
        &text,
        "src/sample/cli.py:9:12: error: Type is wrong (reportReturnType)",
    );
    has(
        &text,
        "::error file=src/sample/cli.py,line=9,col=12::Pyright reportReturnType: Type is wrong",
    );
    has(
        &text,
        "::warning file=src/sample/gone.py,line=1,col=1::Pyright: Import cycle",
    );
    has(&text, ".:1:1: information: Something to know");
    has(&text, "Pyright: 4 file(s), 1 error(s), 1 warning(s)");
    // `# type: ignore[import]` names rules, and stands below a docstring.
    lacks(&text, "file=src/sample/__init__.py");
    let pyright: Value = serde_json::from_str(&fixture.noted("pyright-config")).unwrap();
    assert_eq!(pyright["typeCheckingMode"], "strict");
    assert_eq!(pyright["pythonVersion"], "3.12");
}

#[test]
fn a_file_pyright_leaves_out_is_named() {
    let _alone = alone();
    let mut fixture = Fixture::new();
    fixture.files.push(".typings/extra.pyi".to_owned());
    fixture.file(".typings/extra.pyi", "");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "Pyright: 4 file(s), 0 error(s), 0 warning(s)");
    has(
        &text,
        "::error::Pyright analysed 4 of the 5 file(s) check-python handed it; each one it left \
         out is named",
    );
    has(
        &text,
        "::error file=.typings/extra.pyi::Pyright did not analyse it, although check-python \
         handed it over by name, as Pyright does with a file in a folder whose name starts with \
         a dot; move it out of such a folder, or take it out with an [[exclude]] entry and its \
         reason",
    );
    lacks(&text, "file=src/sample/cli.py::Pyright did not");
    assert_eq!(pyright::analysed(b"not json"), 0);
    assert_eq!(pyright::analysed(b"{}"), 0);
    assert_eq!(pyright::analysed(b"{\"summary\": {}}"), 0);
    assert_eq!(
        pyright::analysed(b"{\"summary\": {\"filesAnalyzed\": 2}}"),
        2
    );
}

#[test]
fn pyright_that_cannot_run_or_read_its_configuration() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.file("pyrightconfig.json", "[1]");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=pyrightconfig.json::Pyright's configuration is not a table of settings",
    );

    let fixture = Fixture::new();
    fixture.file("pyrightconfig.json", "{");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error file=pyrightconfig.json::Pyright's configuration cannot be read: ",
    );

    let fixture = Fixture::new();
    fixture.file("pyrightconfig.json", "{}");
    let file = fixture.root().join("pyrightconfig.json");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    let (code, text) = fixture.run();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error file=pyrightconfig.json::Pyright's configuration cannot be read: ",
    );

    let fixture = Fixture::new();
    fixture.file(
        "pyrightconfig.json",
        "{\"enableTypeIgnoreComments\": false}",
    );
    fs::create_dir(
        fixture
            .root()
            .join(format!("{}-{}.json", pyright::RUN, std::process::id())),
    )
    .unwrap();
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error::Pyright's configuration cannot be written beside the repository's:",
    );

    let fixture = Fixture::new();
    fixture
        .mark("pyright.json", "Error: no such project")
        .mark("pyright-status", "3");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "Error: no such project");
    has(
        &text,
        "::error::Pyright could not check the files (it ended with exit status 3); its message \
         stands in the log",
    );

    let fixture = Fixture::new();
    fixture.mark("pyright-status", "2");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::Pyright ended with exit status 2; its message stands in the log",
    );

    // A folder named like the configuration is none, so pyproject.toml is.
    let fixture = Fixture::new();
    fs::create_dir(fixture.root().join("pyrightconfig.json")).unwrap();
    fixture.file(
        "pyproject.toml",
        &format!("{PYPROJECT}\n[tool.pyright]\nenableTypeIgnoreComments = false\n"),
    );
    let (code, text) = fixture.run();
    assert_eq!(code, 0, "{text}");
    has(
        &text,
        "Pyright: strict mode, with pyproject.toml laid over it",
    );
}

#[test]
fn tests_that_fail_are_named_on_their_files() {
    let _alone = alone();
    let fixture = Fixture::new();
    let root = fixture.root();
    fixture
        .mark(
            "pytest-out",
            &format!(
                "rootdir: {}, configfile: pyproject.toml\n",
                root.display()
            ),
        )
        .mark("pytest-status", "1")
        .mark(
            "junit.xml",
            "<?xml version=\"1.0\"?><testsuites><testsuite>\
             <testcase classname=\"tests.test_cli\" name=\"test_greet\" file=\"tests/test_cli.py\" line=\"6\">\
             <failure message=\"assert 'a' == 'b'&#10;more\">tests/test_cli.py:8: AssertionError</failure></testcase>\
             <testcase classname=\"tests.test_cli\" name=\"test_other\" file=\"tests/test_cli.py\" line=\"10\">\
             <error message=\"\">elsewhere.py:3: Error</error></testcase>\
             <testcase classname=\"\" name=\"test_nowhere\"><failure/></testcase>\
             <testcase classname=\"tests.test_cli\" name=\"test_passes\" file=\"tests/test_cli.py\" line=\"12\"/>\
             </testsuite></testsuites>",
        );
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=tests/test_cli.py,line=8::pytest: tests.test_cli.test_greet failed: assert \
         'a' == 'b'",
    );
    has(
        &text,
        "::error file=tests/test_cli.py,line=11::pytest: tests.test_cli.test_other errored",
    );
    has(&text, "::error::pytest: test_nowhere failed");
    lacks(&text, "test_passes");
    has(&text, "The tests pass: failed");
    // Coverage still runs after tests that fail.
    has(&text, "Every line and branch of 3 file(s) is covered.");
}

#[test]
fn tests_that_name_nothing_say_why_by_the_exit_status() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.mark("pytest-status", "5");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=pyproject.toml::pytest failed with exit status 5: it collected no test; \
         its output stands in the log",
    );

    let fixture = Fixture::new();
    fixture
        .mark("pytest-status", "1")
        .mark("junit.xml", "<testsuites");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    starts(&text, "::error::pytest's report cannot be read:");

    let fixture = Fixture::new();
    fixture.mark("no-pytest", "");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=pyproject.toml::pytest is not in the project's environment: check-python \
         runs the tests with the pytest the project locks; add it to its dev dependency group \
         (uv add --dev pytest)",
    );
    // The tests ran, so coverage does as well.
    has(&text, "::group::Every line and branch is covered");
}

#[test]
fn coverage_names_every_line_and_branch_and_every_file_it_did_not_measure() {
    let _alone = alone();
    let fixture = Fixture::new();
    let root = fixture.root();
    fixture
        .unmark("coverage-data")
        .mark("parts", "combined")
        .mark(
            "empty",
            &format!("{}/src/sample/__init__.py\n", root.display()),
        )
        .mark(
            "coverage.json",
            &json!({"files": {
                format!("{}/src/sample/cli.py", root.display()): {
                    "missing_lines": [14, 15, 16, 20],
                    "missing_branches": [[12, 14], [18, -1]],
                    "summary": {"percent_covered_display": "80"}
                }
            }})
            .to_string(),
        );
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "src/sample/cli.py: 80% covered");
    has(
        &text,
        "::error file=src/sample/cli.py,line=12::coverage.py: not covered: line(s) 14-16, 20 and \
         branch(es) 12->14, 18->exit",
    );
    has(
        &text,
        "::error file=tests/test_cli.py::coverage.py: measured none of it: no test runs it, or \
         an `omit` setting leaves it out; every line and branch of every file counts",
    );
    lacks(&text, "file=src/sample/__init__.py");
    assert!(
        fixture
            .noted("calls")
            .contains("coverage combine --data-file="),
        "{text}"
    );
}

#[test]
fn coverage_that_recorded_or_reported_nothing() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.unmark("coverage-data");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error::coverage.py recorded nothing: the tests did not run",
    );

    let fixture = Fixture::new();
    fixture
        .unmark("coverage-data")
        .mark("parts", "combined")
        .fail(&["coverage combine"]);
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(&text, "Every line and branch is covered: failed");

    for written in [None, Some("not json")] {
        let fixture = Fixture::new();
        match written {
            Some(text) => fixture.mark("coverage.json", text),
            None => fixture.unmark("coverage.json"),
        };
        let (code, text) = fixture.run();
        assert_eq!(code, 1, "{text}");
        starts(&text, "::error::coverage.py wrote no report:");
    }
}

#[test]
fn the_wheel_is_installed_and_tried() {
    let _alone = alone();
    let fixture = Fixture::new();
    fixture.mark("broken", "sample\n");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=pyproject.toml::smoke test of the wheel: 'import sample' failed in a clean \
         environment; its output stands in the log",
    );

    // A command the wheel names and the clean environment lacks.
    let fixture = Fixture::new();
    fixture.mark("commands", "");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error file=pyproject.toml::smoke test of the wheel: 'sample --help' cannot be started \
         in a clean environment:",
    );

    let fixture = Fixture::new();
    fixture.mark("status-sample", "3");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=pyproject.toml::smoke test of the wheel: 'sample --help' failed with exit \
         status 3 in a clean environment; its output stands in the log",
    );

    let fixture = Fixture::new();
    zipped(
        &fixture.here().join("wheel.whl"),
        &[("sample-0.0.0.dist-info/METADATA", b"")],
    );
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        "::error file=pyproject.toml::smoke test of the wheel: it holds no command and no \
         package to try",
    );

    let fixture = Fixture::new();
    fixture.mark("wheel.whl", "no zip");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error file=pyproject.toml::the wheel cannot be read:",
    );
}

#[test]
fn a_wheel_that_cannot_be_built() {
    let _alone = alone();
    for (manifest, finding) in [
        (
            "[project]\nname = \"sample\"\n",
            "::error file=pyproject.toml::pyproject.toml declares no build backend \
             ([build-system]), or `tool.uv.package = false`, so no wheel can be built: \
             check-python checks every Python unit as an installable package; declare one, such \
             as uv_build",
        ),
        (
            "project = { name = \"sample\" }\n[build-system]\nrequires = []\n",
            "::error file=pyproject.toml::check-python cannot give the wheel its placeholder \
             version: it writes `version` into the [project] table, and this pyproject.toml \
             does not hold its project's fields in one",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.file("pyproject.toml", manifest);
        let (code, text) = fixture.run();
        assert_eq!(code, 1, "{text}");
        has(&text, finding);
        has(&text, "The wheel installs and runs: failed");
    }

    let fixture = Fixture::new();
    fixture.mark("corrupt", "");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error file=pyproject.toml::pyproject.toml cannot be read: ",
    );

    // A manifest that cannot be read is named with the I/O error, not taken
    // for one without a build backend nor for one that does not parse. uv
    // leaves a folder in its place: reading that fails for every user, root
    // included, which `chmod 000` would not stop.
    let fixture = Fixture::new();
    fixture.mark("unreadable", "");
    let (code, text) = fixture.run();
    let error = fs::read_to_string(fixture.root().join("pyproject.toml")).unwrap_err();
    assert_eq!(code, 1, "{text}");
    has(
        &text,
        &format!("::error file=pyproject.toml::pyproject.toml cannot be read: {error}"),
    );
    lacks(&text, "TOML parse error");
    lacks(&text, "declares no build backend");
    lacks(&fixture.noted("calls"), "uv build --wheel");

    for failing in ["uv export", "uv build", "uv venv", "uv pip install"] {
        let fixture = Fixture::new();
        fixture.fail(&[failing]);
        let (code, text) = fixture.run();
        assert_eq!(code, 1, "{text}");
        has(&text, "The wheel installs and runs: failed");
        if failing == "uv build" {
            has(
                &text,
                "::error file=pyproject.toml::uv build cannot build the wheel; its message \
                 stands in the log",
            );
            assert_eq!(
                fs::read_to_string(fixture.root().join("pyproject.toml")).unwrap(),
                PYPROJECT
            );
        }
    }

    let fixture = Fixture::new();
    fixture.mark("read-only", "");
    let (code, text) = fixture.run();
    fs::set_permissions(
        fixture.root().join("pyproject.toml"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error file=pyproject.toml::cannot be put back as it is committed: ",
    );
    lacks(&text, "uv build cannot build");

    // The placeholder cannot be written: that is the finding, uv build does
    // not run, and the file, still as committed, is not written again.
    let fixture = Fixture::new();
    fixture.mark("read-only-early", "");
    let (code, text) = fixture.run();
    fs::set_permissions(
        fixture.root().join("pyproject.toml"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(code, 1, "{text}");
    starts(
        &text,
        "::error file=pyproject.toml::cannot be given the placeholder version 0.0.0: ",
    );
    has(&text, "The wheel installs and runs: failed");
    lacks(&text, "The wheel gets the placeholder version");
    lacks(&text, "uv build cannot build");
    lacks(&text, "cannot be put back");
    lacks(&fixture.noted("calls"), "uv build --wheel");
    assert_eq!(
        fs::read_to_string(fixture.root().join("pyproject.toml")).unwrap(),
        PYPROJECT
    );
}

#[test]
fn the_folder_of_the_check_has_to_be_made() {
    let _alone = alone();
    let mut fixture = Fixture::new();
    fixture.temp = fixture.here().join("missing");
    let (code, text) = fixture.run();
    assert_eq!(code, 1, "{text}");
    starts(&text, "::error::no folder can be made in");
}

#[test]
fn the_helpers() {
    assert_eq!(python::at(".", "a"), "a");
    assert_eq!(python::at("x/y", "a"), "x/y/a");
    assert_eq!(python::above("."), None);
    assert_eq!(python::above("x"), Some(".".to_owned()));
    assert_eq!(python::above("x/y"), Some("x".to_owned()));
    assert_eq!(python::upwards("x/y"), ["x/y", "x", "."]);
    assert_eq!(python::major_minor("Python 3.14.0"), Some((3, 14)));
    assert_eq!(python::major_minor("3.11.0"), Some((3, 11)));
    assert_eq!(python::major_minor("Python 3"), None);
    assert_eq!(python::major_minor("Python"), None);
    assert_eq!(
        python::in_environment(Path::new("/e"), "python", true),
        PathBuf::from("/e/Scripts/python.exe")
    );
    assert_eq!(
        python::in_environment(Path::new("/e"), "python", false),
        PathBuf::from("/e/bin/python")
    );
    assert_eq!(
        python::excluded(
            &["a/x.py".to_owned()],
            &[
                path("a/x.py"),
                path("a/y.pyi"),
                path("a/z.txt"),
                path("b/w.py")
            ],
            &path("a"),
        ),
        ["a/y.pyi"]
    );
    assert!(python::REMOVED.contains(&"PYTHONPATH"));
    assert!(python::VARIABLES.iter().any(
        |(name, setting)| *name == "UV_NO_PROGRESS" && *setting == python::Setting::Value("1")
    ));
}

#[test]
fn ruff_helpers() {
    let var = |name: &str| match name {
        "HOME" => Some("/home/me".to_owned()),
        "BASE" => Some("/base".to_owned()),
        _ => None,
    };
    assert_eq!(ruff::expanded("~/r.toml", &var), "/home/me/r.toml");
    assert_eq!(ruff::expanded("~", &var), "/home/me");
    assert_eq!(ruff::expanded("~other/r.toml", &var), "~other/r.toml");
    assert_eq!(ruff::expanded("~/r.toml", &|_| None), "~/r.toml");
    assert_eq!(
        ruff::expanded("$BASE/a/${BASE}/$NONE", &var),
        "/base/a//base/$NONE"
    );
    assert_eq!(ruff::target_version(">=3.12,<4"), Some("py312".to_owned()));
    assert_eq!(ruff::target_version("~=3.12"), None);
    assert_eq!(ruff::inside(".", "a/b.py"), "a/b.py");
    assert_eq!(ruff::inside("a", "a/b.py"), "b.py");
    assert_eq!(ruff::inside("c", "a/b.py"), "a/b.py");
    let root = Path::new("/r");
    assert_eq!(ruff::from_root(root, ".", "a.py"), "a.py");
    assert_eq!(ruff::from_root(root, "x", "a.py"), "x/a.py");
    assert_eq!(ruff::from_root(root, "x", "/r/x/a.py"), "x/a.py");
    assert_eq!(
        ruff::from_root(root, "x", "/elsewhere/a.py"),
        "/elsewhere/a.py"
    );
    assert_eq!(ruff::from_root(root, "x", "/rx/a.py"), "/rx/a.py");
    let none = ruff::settings(Path::new("pyproject.toml"), "[tool]\nother = 1\n").unwrap();
    assert!(none.is_none());
    let none = ruff::settings(Path::new("pyproject.toml"), "tool = 1\n").unwrap();
    assert!(none.is_none());
    let some = ruff::settings(Path::new("ruff.toml"), "line-length = 90\n").unwrap();
    assert!(some.is_some());
    assert!(ruff::settings(Path::new("ruff.toml"), "=").is_err());
    assert_eq!(
        ruff::layer(Path::new("/below.toml"), &toml::Table::new()),
        "extend = \"/below.toml\"\n"
    );
}

#[test]
fn pyright_helpers() {
    let lowered = pyright::lowered(
        "\"\"\"Doc.\"\"\"\n# type: ignore\n# pyright: strict, standard\n",
        true,
    );
    assert_eq!(lowered.len(), 1, "{lowered:?}");
    assert_eq!(lowered[0].0, 3);
    assert!(pyright::lowered("#type:ignore\n", false).is_empty());
    let lowered = pyright::lowered("  \n# x # type: ignore[]\nimport os\n", true);
    assert_eq!(lowered.len(), 1, "{lowered:?}");
    assert_eq!(lowered[0].0, 2);
    assert!(pyright::settings(Path::new("pyproject.toml"), "[tool]\n").is_err());
    assert!(pyright::settings(Path::new("pyproject.toml"), "[").is_err());
    let settings = pyright::run_settings(
        serde_json::Map::new(),
        Path::new("/r"),
        &[PathBuf::from("/r/a.py")],
        Path::new("/"),
        (3, 12),
    );
    assert_eq!(settings["venvPath"], "");
    assert_eq!(settings["venv"], "");
    assert_eq!(settings["include"], json!(["a.py"]));
    assert_eq!(settings["exclude"], json!([pyright::EXCLUDES_NOTHING]));
    let (found, summary) = pyright::diagnostics(&json!({}), Path::new("/r"), "u");
    assert!(found.is_empty());
    assert_eq!(summary, "Pyright: 0 file(s), 0 error(s), 0 warning(s)");
}

#[test]
fn pytest_helpers() {
    assert_eq!(
        pytest::rootdir("a\nrootdir: /x/y\n"),
        Some("/x/y".to_owned())
    );
    assert_eq!(
        pytest::rootdir("rootdir: /x, configfile: pyproject.toml, testpaths: tests"),
        Some("/x".to_owned())
    );
    assert_eq!(pytest::rootdir("nothing"), None);
    assert_eq!(pytest::normalized("a/./b//c/../d"), "a/b/d");
    assert_eq!(pytest::normalized("/a/../b"), "/b");
    assert_eq!(pytest::normalized("../../a"), "../../a");
    assert_eq!(pytest::normalized("."), ".");
    assert_eq!(
        pytest::candidates(Some("/x".to_owned()), Path::new("/r/u"), Path::new("/r")),
        [PathBuf::from("/x")]
    );
    assert_eq!(
        pytest::candidates(None, Path::new("/r/u/v"), Path::new("/r")),
        [
            PathBuf::from("/r/u/v"),
            PathBuf::from("/r/u"),
            PathBuf::from("/r")
        ]
    );
    assert_eq!(
        pytest::candidates(None, Path::new("/u"), Path::new("/r")),
        [PathBuf::from("/u"), PathBuf::from("/")]
    );
    for (status, reason) in [
        (
            Some(2),
            "the run was interrupted, as by a test module that cannot be collected",
        ),
        (Some(3), "pytest itself failed"),
        (Some(4), "its command line or configuration is not usable"),
        (Some(5), "it collected no test"),
        (Some(1), "a test failed"),
        (None, "a test failed"),
    ] {
        assert_eq!(pytest::reason(status), reason);
    }
    // Without a folder to look in, a report path stays as it is written.
    let found = pytest::failed(
        "<testcase name=\"t\" file=\"/abs/t.py\" line=\"x\"><failure message=\"m\"/></testcase>",
        &[],
        Path::new("/"),
    )
    .unwrap();
    assert_eq!(
        found,
        [pytest::Failed {
            place: Some(("abs/t.py".to_owned(), 1)),
            message: "pytest: t failed: m".to_owned()
        }]
    );
    // A report path is found in a later folder where the first lacks it.
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("u")).unwrap();
    fs::write(root.join("t.py"), "").unwrap();
    let found = pytest::failed(
        "<testcase name=\"t\" file=\"t.py\" line=\"0\"><failure>t.py:x: no number</failure></testcase>",
        &[root.join("u"), root.clone()],
        &root,
    )
    .unwrap();
    assert_eq!(found[0].place, Some(("t.py".to_owned(), 1)));
}

#[test]
fn coverage_helpers() {
    assert_eq!(coverage::ranges(&[7, 1, 2, 3]), "1-3, 7");
    assert_eq!(coverage::ranges(&[]), "");
    assert!(coverage::uncovered(&coverage::Measured::default()).is_none());
    let entry = coverage::Measured {
        missing_branches: vec![(5, 0)],
        ..coverage::Measured::default()
    };
    assert_eq!(
        coverage::uncovered(&entry),
        Some((5, "coverage.py: not covered: branch(es) 5->exit".to_owned()))
    );
    let report: coverage::Report = serde_json::from_value(json!({"files": {"a.py": {}}})).unwrap();
    let by_path = coverage::by_path(report, Path::new("/r"), Path::new("/r/u"));
    assert!(by_path.contains_key("u/a.py"), "{by_path:?}");
}

#[test]
fn wheel_helpers() {
    let manifest: toml::Table =
        toml::from_str("[build-system]\n[tool.uv]\npackage = false\n").unwrap();
    assert!(wheel::no_backend(&manifest));
    let manifest: toml::Table = toml::from_str("[build-system]\n").unwrap();
    assert!(!wheel::no_backend(&manifest));

    let source = "# head\n[project] # the project\nname = \"x\"\nversion = \"1\"\ndynamic = [\n  \"version\",\n  \"readme\",\n]\n\n[[tool.list]]\nversion = 2\n[tool.other]\ndynamic = 3\n";
    let manifest: toml::Table = toml::from_str(source).unwrap();
    assert_eq!(
        wheel::placeholder(source, &manifest).unwrap(),
        "# head\n[project] # the project\nversion = \"0.0.0\"\ndynamic = [\"readme\"]\nname = \"x\"\n\n[[tool.list]]\nversion = 2\n[tool.other]\ndynamic = 3\n"
    );
    let source = "[project]\nname = \"x\"\ndynamic = [\"version\"]\n";
    let manifest: toml::Table = toml::from_str(source).unwrap();
    assert!(
        wheel::placeholder(source, &manifest)
            .unwrap()
            .contains("dynamic = []")
    );
    // A second [project] table makes the file no TOML.
    let source = "[project]\nname = \"x\"\n[project]\n";
    assert!(wheel::placeholder(source, &toml::Table::new()).is_none());

    assert!(wheel::identifier("_a1"));
    assert!(wheel::identifier("é"));
    assert!(!wheel::identifier(""));
    assert!(!wheel::identifier("1a"));
    assert!(!wheel::identifier("a-b"));

    assert_eq!(
        wheel::console_scripts(
            "# c\n[gui_scripts]\ng = a:b\n[console_scripts]\n\n; c\nfirst = a:b\n  continued\nsecond: a:c\nnothing\n"
        ),
        ["first", "second"]
    );
    let names: Vec<String> = [
        "pkg/__init__.py",
        "pkg/",
        "mod.py",
        "_speedups.cpython-312-x86_64-linux-gnu.so",
        "native.pyd",
        "x-1.0.dist-info/METADATA",
        "x-1.0.data/scripts/s",
        "not-a-name.py",
        "README.txt",
        "pkg/sub.py",
    ]
    .map(str::to_owned)
    .to_vec();
    assert_eq!(wheel::tops(&names), ["_speedups", "mod", "native", "pkg"]);
    assert!(wheel::offered(Path::new("/nonexistent.whl")).is_err());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.whl");
    zipped(
        &file,
        &[
            ("x-1.dist-info/entry_points.txt", b"\xff\xfe"),
            ("x/entry_points.txt", b""),
        ],
    );
    assert!(wheel::offered(&file).is_err());

    let status = |code: i32| {
        Command::new("sh")
            .args(["-c", &format!("exit {code}")])
            .status()
            .unwrap()
    };
    assert_eq!(wheel::unanswered("c", Some(status(0))), None);
    assert!(
        wheel::unanswered("c", Some(status(4)))
            .unwrap()
            .contains("failed with exit status 4")
    );
    assert!(
        wheel::unanswered("c", None)
            .unwrap()
            .contains("did not answer within 60 seconds")
    );
}

#[test]
fn what_a_command_that_does_not_answer_started_is_stopped_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let marker = dir.path().join("marker");
    // A launcher: it starts a server in the background and waits for it.
    let mut launcher = Command::new("sh");
    launcher.args([
        "-c",
        &format!("(sleep 1; echo alive > '{}') & wait", marker.display()),
    ]);
    let ended = wheel::answer(launcher, &log, Duration::from_millis(300)).unwrap();
    assert_eq!(ended, None);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "the server outlived the command");
}

#[test]
fn a_command_that_does_not_answer_in_time_is_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let mut slow = Command::new("sh");
    slow.args(["-c", "echo started; exec sleep 5"]);
    let ended = wheel::answer(slow, &log, Duration::from_millis(300)).unwrap();
    assert_eq!(ended, None);
    assert_eq!(fs::read_to_string(&log).unwrap(), "started\n");
    let mut quick = Command::new("sh");
    quick.args(["-c", "exit 0"]);
    assert!(
        wheel::answer(quick, &log, Duration::from_secs(5))
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(wheel::answer(Command::new("/nonexistent"), &log, Duration::from_secs(1)).is_err());
    assert!(
        wheel::answer(
            Command::new("sh"),
            Path::new("/nonexistent/log"),
            Duration::from_secs(1)
        )
        .is_err()
    );
}
