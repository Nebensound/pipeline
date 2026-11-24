//! A preset on a repository the way a real project of its kind holds it:
//! phase 0 passes without a gap, an error or a warning, and every check, build
//! and version writer the plan starts has its building block, so that no step
//! `Not written yet` fails for it.
//!
//! The repository is not trimmed until nothing is reported: it holds what a
//! project of the preset holds, with the files every repository holds and the
//! caller of the workflow, and phase 0 runs with the name, the default branch
//! and the visibility a run hands it.
//!
//! The blocks are read from `.github/actions/` beside the tool, where the
//! workflow calls them; `cargo xtask check` holds that folder, the register and
//! the lists of written blocks in `nebensound.yml` to each other. It reads
//! files outside the package: in the crate `cargo package` builds, they are not
//! there to be read, so the package leaves this file out (see its
//! `Cargo.toml`).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::path::Path;

use serde_json::Value;
use support::{Git, call};

/// Where the building blocks lie, from the repository root.
const ACTIONS: &str = ".github/actions";

/// The files every repository holds, as a project writes them.
const MINIMUM: &[(&str, &str)] = &[
    ("LICENSE", "All rights reserved.\n"),
    ("AGENTS.md", "# Rules\n\nEverything is English.\n"),
    (".github/CODEOWNERS", "* @example\n"),
    (
        ".editorconfig",
        "root = true\n\n[*]\ninsert_final_newline = true\n",
    ),
];

/// The caller of the workflow, pinned to a commit as the README shows it.
const CALLER: &str = r"name: gate

on:
  pull_request:
  push:
    branches:
      - main

permissions:
  contents: read

jobs:
  nebensound:
    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@5ca52fe5ec8089bbda9ad056cdb83ebe0c09e07c # 2026-11-21
    permissions:
      contents: read
";

/// A small HTTP service in Python: a uv project with its lockfile, the Python
/// version uv reads, a typed package, its tests and the Dockerfile the build
/// docker reads.
const PYTHON_SERVICE: &[(&str, &str)] = &[
    (
        ".nebensound/repo.toml",
        "[repo]\naudience = \"public\"\npipeline = \"python-service\"\n",
    ),
    (
        "README.md",
        "# greeter\n\nA service that greets whoever asks, over HTTP on port 8000.\n",
    ),
    (".gitignore", ".venv/\n__pycache__/\n"),
    (".dockerignore", ".git/\n.venv/\n**/__pycache__/\n"),
    (
        ".github/dependabot.yml",
        r"version: 2
updates:
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
  - package-ecosystem: uv
    directory: /
    schedule:
      interval: weekly
  - package-ecosystem: docker
    directory: /deploy
    schedule:
      interval: weekly
",
    ),
    (".github/workflows/gate.yml", CALLER),
    (".python-version", "3.14\n"),
    (
        "pyproject.toml",
        r#"[project]
name = "greeter"
version = "0.0.0"
description = "A service that greets whoever asks."
readme = "README.md"
requires-python = ">=3.14"
dependencies = []

[project.scripts]
greeter = "greeter.server:main"

[dependency-groups]
dev = ["pytest>=9.2"]

[build-system]
requires = ["uv_build>=0.13.9,<0.14"]
build-backend = "uv_build"
"#,
    ),
    ("uv.lock", UV_LOCK),
    (
        "src/greeter/__init__.py",
        "\"\"\"A service that greets whoever asks.\"\"\"\n",
    ),
    ("src/greeter/py.typed", ""),
    (
        "src/greeter/server.py",
        r#""""The HTTP server of the greeter."""

import argparse
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def greet(name: str) -> str:
    """Return the greeting for a name."""
    return f"Hello, {name}!"


def answer(path: str) -> tuple[HTTPStatus, str]:
    """Return the status and the body for the path of a request."""
    if path == "/health":
        return HTTPStatus.OK, "ok"
    name = path.strip("/")
    if "/" in name:
        return HTTPStatus.NOT_FOUND, "not found"
    return HTTPStatus.OK, greet(name or "world")


class Handler(BaseHTTPRequestHandler):
    """Answers every GET with what its path asks for."""

    def do_GET(self) -> None:  # noqa: N802 - the name http.server calls
        """Send the answer for the path."""
        status, body = answer(self.path)
        data = body.encode()
        self.send_response(status)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def serve(port: int) -> ThreadingHTTPServer:
    """Return a server on every address at the port, not yet serving."""
    return ThreadingHTTPServer(("", port), Handler)


def main(argv: list[str] | None = None) -> int:
    """Serve greetings until the process is stopped."""
    parser = argparse.ArgumentParser(prog="greeter", description="Serves greetings.")
    parser.add_argument("--port", type=int, default=8000)
    arguments = parser.parse_args(argv)
    with serve(int(arguments.port)) as server:
        server.serve_forever()
    return 0
"#,
    ),
    (
        "tests/__init__.py",
        "\"\"\"The tests of the greeter.\"\"\"\n",
    ),
    (
        "tests/test_server.py",
        r#""""The server answers each path with its greeting."""

import threading
from http import HTTPStatus
from http.client import HTTPConnection

import pytest

from greeter import server


def test_greet() -> None:
    """Greet a name."""
    assert server.greet("Ada") == "Hello, Ada!"


def test_answer() -> None:
    """Answer the health check, a name, the root and a deeper path."""
    assert server.answer("/health") == (HTTPStatus.OK, "ok")
    assert server.answer("/Ada") == (HTTPStatus.OK, "Hello, Ada!")
    assert server.answer("/") == (HTTPStatus.OK, "Hello, world!")
    assert server.answer("/a/b") == (HTTPStatus.NOT_FOUND, "not found")


def test_the_server_answers_over_http() -> None:
    """Serve a request on a free port."""
    with server.serve(0) as running:
        thread = threading.Thread(target=running.serve_forever)
        thread.start()
        connection = HTTPConnection("127.0.0.1", running.server_address[1])
        try:
            connection.request("GET", "/Ada")
            response = connection.getresponse()
            assert response.status == HTTPStatus.OK
            assert response.read() == b"Hello, Ada!"
        finally:
            connection.close()
            running.shutdown()
            thread.join()


def test_main_serves_on_the_port_it_is_given(monkeypatch: pytest.MonkeyPatch) -> None:
    """Start serving on the port from the command line."""
    ports: list[int] = []

    def serve_forever(self: server.ThreadingHTTPServer) -> None:
        ports.append(self.server_address[1])

    monkeypatch.setattr(server.ThreadingHTTPServer, "serve_forever", serve_forever)
    assert server.main(["--port", "0"]) == 0
    assert len(ports) == 1
"#,
    ),
    (
        "deploy/Dockerfile",
        r#"# The greeter in its own virtual environment, served as the user nobody.
FROM python:3.14-slim
COPY --from=ghcr.io/astral-sh/uv:0.13 /uv /bin/uv
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy UV_PYTHON_DOWNLOADS=never
WORKDIR /app
COPY pyproject.toml uv.lock .python-version README.md ./
COPY src ./src
RUN uv sync --locked --no-dev
USER 65534:65534
EXPOSE 8000
HEALTHCHECK --interval=5s --timeout=3s --retries=3 CMD ["/app/.venv/bin/python", "-c", "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8000/health')"]
CMD ["/app/.venv/bin/greeter"]
"#,
    ),
];

/// The lockfile uv writes for the project: pytest and what it needs, for the
/// tests.
const UV_LOCK: &str = r#"version = 1
revision = 5
requires-python = ">=3.14"

[[package]]
name = "colorama"
version = "0.4.6"
source = { registry = "https://pypi.org/simple" }
sdist = { url = "https://files.pythonhosted.org/packages/d8/53/6f443c9a4a8358a93a6792e2acffb9d9d5cb0a5cfd8802644b7b1c9a02e4/colorama-0.4.6.tar.gz", hash = "sha256:08695f5cb7ed6e0531a20572697297273c47b8cae5a63ffc6d6ed5c201be6e44", size = 27697, upload-time = "2022-10-25T02:36:22.414Z" }
wheels = [
    { url = "https://files.pythonhosted.org/packages/d1/d6/3965ed04c63042e047cb6a3e6ed1a63a35087b6a609aa3a15ed8ac56c221/colorama-0.4.6-py2.py3-none-any.whl", hash = "sha256:4f1d9991f5acc0ca119f9d443620b77f9d6b33703e51011c16baf57afb285fc6", size = 25335, upload-time = "2022-10-25T02:36:20.889Z" },
]

[[package]]
name = "greeter"
version = "0.0.0"
source = { editable = "." }

[package.dev-dependencies]
dev = [
    { name = "pytest" },
]

[package.metadata]

[package.metadata.requires-dev]
dev = [{ name = "pytest", specifier = ">=9.2" }]

[[package]]
name = "iniconfig"
version = "2.3.1"
source = { registry = "https://pypi.org/simple" }
sdist = { url = "https://files.pythonhosted.org/packages/01/e1/2069291243c926a2ff1cd706c7f3eeb9b62144bf60f77c9fb9ff2fb26bd3/iniconfig-2.3.1.tar.gz", hash = "sha256:67f4b9c50da0dedf52af349e7749a80a9057a5031199791b906c3bb3ae878960", size = 21209, upload-time = "2026-10-06T22:48:38.076Z" }
wheels = [
    { url = "https://files.pythonhosted.org/packages/56/43/4ca9e49d27a1fcf6bece6f6aec0ea46bb9112489b93d4b688fb415457bdb/iniconfig-2.3.1-py3-none-any.whl", hash = "sha256:9121e2c1fdb355232495be3194c8dfe87ccc2d5dee45947b78e68f499790d7a7", size = 7552, upload-time = "2026-10-06T22:48:36.959Z" },
]

[[package]]
name = "packaging"
version = "26.3"
source = { registry = "https://pypi.org/simple" }
sdist = { url = "https://files.pythonhosted.org/packages/7d/fa/3944b40b07da9ce895c0e6303a5ab7d53da063554f534556b134a54d6093/packaging-26.3.tar.gz", hash = "sha256:94edc256424af38762eb31306eed28beb9f0efc50a8837492c9d6fd6004aed79", size = 313412, upload-time = "2026-08-04T18:15:28.737Z" }
wheels = [
    { url = "https://files.pythonhosted.org/packages/63/34/ba1c580383c9eada3711951fef0795c80b829a078d72188184bcab9dd527/packaging-26.3-py3-none-any.whl", hash = "sha256:d7193f7c8e4e93f444fde0262bf90af30e16fa0ad0ad44cb553c87339b23cd1c", size = 129956, upload-time = "2026-08-04T18:15:27.159Z" },
]

[[package]]
name = "pluggy"
version = "1.7.0"
source = { registry = "https://pypi.org/simple" }
sdist = { url = "https://files.pythonhosted.org/packages/bf/db/7fc19e6f2dc92a966727031389fc2e08b558f0f25eb7403c1119ad4713cd/pluggy-1.7.0.tar.gz", hash = "sha256:d1eaa46ebb595891b860ab086b4d09c8588af65ebd4361b8e8f4bb8920b90ba8", size = 123304, upload-time = "2026-10-15T09:50:58.343Z" }
wheels = [
    { url = "https://files.pythonhosted.org/packages/40/9e/2b38731e0fc536806f16490e1a12d7f0dc2a1235aa8cc07bcc75416a7daa/pluggy-1.7.0-py3-none-any.whl", hash = "sha256:7dd7b0d8832ba3cb632c306926ded123429211b83641b35dc5c41ad2d34f9bec", size = 27082, upload-time = "2026-10-15T09:50:56.808Z" },
]

[[package]]
name = "pygments"
version = "2.21.0"
source = { registry = "https://pypi.org/simple" }
sdist = { url = "https://files.pythonhosted.org/packages/49/2e/ced460408999b33da6b31b0021b0f37d329e202d4169aeb164493778f25b/pygments-2.21.0.tar.gz", hash = "sha256:610ca751c9bc2492b38eb9a38a7fbc93edbbb2d7182edaf34e66ae493dee5c8c", size = 5005329, upload-time = "2026-08-17T08:02:48.824Z" }
wheels = [
    { url = "https://files.pythonhosted.org/packages/71/46/17f022dd3e953bf20a04a028a21ec746d942f8d2af30fa0f124fa0e6a684/pygments-2.21.0-py3-none-any.whl", hash = "sha256:2363c69b61c4a97c838da3b130dcd6468f4848992b21a82f2a63ec34377137d9", size = 1250147, upload-time = "2026-08-17T08:02:44.912Z" },
]

[[package]]
name = "pytest"
version = "9.2.0"
source = { registry = "https://pypi.org/simple" }
dependencies = [
    { name = "colorama", marker = "sys_platform == 'win32'" },
    { name = "iniconfig" },
    { name = "packaging" },
    { name = "pluggy" },
    { name = "pygments" },
]
sdist = { url = "https://files.pythonhosted.org/packages/79/42/bb4641652e58dedeac2ec51abc1f8cae4fee4ec63899a0999016229a0f2a/pytest-9.2.0.tar.gz", hash = "sha256:97c4f3b1f3c7414aa36465e09b8220f1d03627993bd51b1282e193753c80557b", size = 1797555, upload-time = "2026-10-27T21:30:20.728Z" }
wheels = [
    { url = "https://files.pythonhosted.org/packages/51/39/1f5d8c8877a0fa327db959c19341d4be61660de1ab6b7391f9eb805bcd67/pytest-9.2.0-py3-none-any.whl", hash = "sha256:07acdcf88ec174f907dc01af70bf52d49b014bbab03faeeade31c96b18c2e588", size = 411776, upload-time = "2026-10-27T21:30:19.131Z" },
]
"#;

/// The repository, committed once on `main` with a Conventional Commit, as a
/// project's first push leaves it.
fn repository(files: &[(&str, &str)]) -> Git {
    let git = Git::new();
    git.write(MINIMUM).write(files);
    git.commit("feat: greet whoever asks over HTTP");
    git
}

/// Phase 0 the way `Declare` runs it: the whole result, and the plan.
fn phase0(git: &Git, name: &str) -> (String, Value) {
    let (text, code) = call(&[
        "nebensound-repo",
        "-C",
        git.root(),
        "check",
        "--name",
        name,
        "--default-branch",
        "main",
        "--visibility",
        "public",
    ]);
    assert_eq!(code, 0, "{text}");
    let (plan, code) = call(&["nebensound-repo", "-C", git.root(), "plan"]);
    assert_eq!(code, 0, "{plan}");
    (text, serde_json::from_str(&plan).expect("the plan is JSON"))
}

/// Whether the building block `name` exists, the folder the workflow calls.
fn written(name: &str) -> bool {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(ACTIONS)
        .join(name)
        .join("action.yml")
        .is_file()
}

fn names(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("a list")
        .iter()
        .map(|name| name.as_str().expect("a name"))
        .collect()
}

#[test]
fn a_python_service_passes_phase_0_without_a_gap() {
    let git = repository(PYTHON_SERVICE);
    let (text, plan) = phase0(&git, "greeter");

    assert!(text.contains("preset python-service"), "{text}");
    assert!(text.contains("Findings\n\n  none"), "{text}");
    assert!(
        text.contains("19 files, 0 gaps, 0 errors, 0 warnings"),
        "{text}"
    );

    // One unit, the repository: the checks of the preset and the base checks,
    // and the build docker, with a version of this run so that it starts.
    let units = plan["units"].as_array().expect("units");
    assert_eq!(units.len(), 1, "{plan}");
    let unit = &units[0];
    assert_eq!(unit["path"], ".", "{plan}");
    assert_eq!(
        names(&unit["checks"]),
        ["dockerfile", "json", "markdown", "python", "toml", "yaml"],
        "{plan}"
    );
    assert_eq!(unit["build"], "docker", "{plan}");
    let version = &plan["versions"][0];
    assert!(version["version"].is_string(), "{plan}");

    // Nothing of it fails in a step `Not written yet`.
    let mut blocks: Vec<String> = names(&unit["checks"])
        .into_iter()
        .map(|check| format!("check-{check}"))
        .collect();
    blocks.push(format!(
        "build-{}",
        unit["build"].as_str().expect("a build")
    ));
    blocks.extend(
        names(&version["writers"])
            .into_iter()
            .map(|writer| format!("version-{writer}")),
    );
    let unwritten: Vec<&String> = blocks.iter().filter(|block| !written(block)).collect();
    assert!(unwritten.is_empty(), "not written yet: {unwritten:?}");
}

#[test]
fn a_block_that_is_not_written_is_seen() {
    // The check above would pass for a plan of blocks that are all missing if
    // it looked in the wrong place.
    assert!(written("check-python"));
    assert!(!written("check-nothing-like-it"));
}
