//! A second program for a test case of build-cargo-release.

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("case {}", env!("CARGO_PKG_VERSION"));
    }
}
