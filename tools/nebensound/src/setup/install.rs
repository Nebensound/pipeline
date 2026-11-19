//! Installing a package through its runtime's own installer.
//!
//! A program written in JavaScript or Python comes as one package, which the
//! register pins by its digest. Its dependencies are the installer's business,
//! resolved as the building blocks resolve them: npm installs the verified
//! package file with `--ignore-scripts`, so nothing of it runs while it is put
//! in place; pip installs the verified wheel into a virtual environment of its
//! own, so that the computer's Python packages stay untouched, and takes only
//! wheels for its dependencies, so that no build code of one runs either.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use nebensound_declaration::programs::Installer;

use super::{Environment, execute, find};

/// Installs the package `file` into `dir` with the installer of the runtime
/// whose executable is `runtime`.
///
/// # Errors
///
/// Returns a sentence when the installer cannot be started or fails.
pub fn package(
    installer: Installer,
    runtime: &Path,
    file: &Path,
    dir: &Path,
    env: &Environment,
) -> Result<(), String> {
    let path = search_path_with(runtime, env);
    match installer {
        Installer::Npm => {
            // npm stands beside node, in every distribution of Node.js.
            let npm = runtime.with_file_name(if env.windows { "npm.cmd" } else { "npm" });
            let mut command = Command::new(npm);
            command
                .arg("install")
                .arg("--prefix")
                .arg(dir)
                .args(["--no-save", "--no-package-lock", "--ignore-scripts"])
                .args(["--no-fund", "--no-audit"])
                .arg(file);
            run(&mut command, &path, "npm")
        }
        Installer::Pip => {
            let mut venv = Command::new(runtime);
            venv.args(["-m", "venv"]).arg(dir);
            run(&mut venv, &path, "python -m venv")?;
            let mut pip = Command::new(find::environment_python(dir, env.windows));
            pip.args(["-m", "pip", "install"])
                .args(["--disable-pip-version-check", "--no-cache-dir", "--quiet"])
                .arg("--only-binary=:all:")
                .arg(file);
            run(&mut pip, &path, "pip")
        }
    }
}

fn run(command: &mut Command, path: &OsString, what: &str) -> Result<(), String> {
    command.env("PATH", path).stdin(Stdio::null());
    execute(command, what).map(drop)
}

/// The search path with the runtime's folder in front, so that what the
/// installer starts finds the same runtime.
fn search_path_with(runtime: &Path, env: &Environment) -> OsString {
    let mut path = OsString::from(runtime.parent().unwrap_or(runtime));
    path.push(if env.windows { ";" } else { ":" });
    path.push(&env.search_path);
    path
}
