//! `cargo xtask`: hands the arguments to [`xtask::run`].
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::process::ExitCode;

// Only reads the arguments and maps the result of `run`, which the library's
// tests cover.
#[cfg_attr(coverage_nightly, coverage(off))]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match xtask::run(&args) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
