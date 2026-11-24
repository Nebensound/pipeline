//! A program for a test case of build-cargo-release.

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("case 9.9.9");
    }
}
