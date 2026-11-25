//! The caller `init` writes is the caller the README documents.
//!
//! The tool holds the caller as one template; the README shows the whole file
//! a repository adds. Line for line they are the same file, but for the two
//! lines that may differ: the workflow's `name`, since `init` writes the file
//! as `.github/workflows/nebensound.yml` named `nebensound`, and the reference
//! of the workflow, where the template holds the place of the pin.
//!
//! It reads the README, a file outside the package: in the crate
//! `cargo package` builds, it is not there to be read, so the package leaves
//! this file out (see its `Cargo.toml`).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::Path;

use nebensound_repo::init::TEMPLATE;

/// The first YAML block below the heading that shows the caller.
fn readme_caller() -> String {
    let readme = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"))
        .expect("the README");
    let below = readme
        .split_once("\n## Calling the pipeline\n")
        .expect("the heading of the caller")
        .1;
    let block = below
        .split_once("\n```yaml\n")
        .expect("a YAML block below it")
        .1;
    let (block, _) = block.split_once("\n```\n").expect("the end of the block");
    format!("{block}\n")
}

/// Whether a line of the caller is one of the two that may differ.
fn may_differ(line: &str) -> bool {
    line.starts_with("name: ")
        || line.starts_with("    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@")
}

#[test]
fn the_template_is_the_readme_caller_but_for_its_name_and_its_pin() {
    let readme = readme_caller();
    let documented: Vec<&str> = readme.lines().collect();
    let template: Vec<&str> = TEMPLATE.lines().collect();

    assert_eq!(template.len(), documented.len(), "{TEMPLATE}\n{readme}");
    for (written, shown) in template.iter().zip(&documented) {
        if may_differ(shown) {
            assert!(may_differ(written), "{written:?} against {shown:?}");
        } else {
            assert_eq!(written, shown);
        }
    }
    let differing: Vec<&str> = template
        .into_iter()
        .filter(|line| may_differ(line))
        .collect();
    assert_eq!(
        differing,
        [
            "name: nebensound",
            "    uses: Nebensound/pipeline/.github/workflows/nebensound.yml@{pin}",
        ]
    );
    assert!(TEMPLATE.ends_with("deployments: write\n"));
}
