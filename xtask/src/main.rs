//! `cargo xtask`: the tasks of this repository. Everything lives in the
//! library.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(xtask::run(
        &args,
        &xtask::Setup::current(),
        &mut std::io::stdout(),
    ))
}
