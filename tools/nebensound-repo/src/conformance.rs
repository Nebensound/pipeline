//! Where a repository departs from the organization's conventions, as far as
//! that can be told for certain: its name, its default branch and the files
//! every repository holds.
//!
//! Each departure is a warning. It stops nothing: a repository is brought in
//! line when it is touched, and the warning says what to bring in line without
//! anyone having to remember the list. What needs judgement (the language of
//! comments and messages, naming, documentation) is not told here.
//!
//! Free of input and output like the rules of the declaration: the name and
//! the default branch are handed in, by the run or read from Git by
//! [`crate::repository`], and the files are the paths Git tracks.

use crate::finding::{Finding, Rule};
use crate::path::RelativePath;

/// The one name that does not have to conform: GitHub requires it for the
/// repository that holds an organization's community health files.
const EXEMPT_NAME: &str = ".github";

/// The default branch of every repository: where every version is given out.
const DEFAULT_BRANCH: &str = "main";

/// What is known about a fact of the repository that its declaration does not
/// hold, because GitHub holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Told {
    /// Known: handed in by the run, or read from Git.
    Known(String),
    /// Not known, with the sentence that says why. It is not guessed.
    Unknown(String),
}

/// The facts about a repository that GitHub holds and the declaration does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The repository's name.
    pub name: Told,
    /// Its default branch.
    pub default_branch: Told,
}

/// Whether a repository's name conforms: lowercase letters and digits, in
/// words joined by single hyphens, `^[a-z0-9]+(-[a-z0-9]+)*$`. `.github` is
/// exempt.
#[must_use]
pub fn conforming_name(name: &str) -> bool {
    name == EXEMPT_NAME
        || name.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

/// The name a repository's name would conform as: lowercase, `_` and `.`
/// become `-`, hyphens that follow each other become one, and a hyphen at
/// either end goes. Words are never split at a change of case: `MyTool`
/// becomes `mytool`, not `my-tool`, since a split would have to guess.
#[must_use]
pub fn suggested_name(name: &str) -> String {
    let mut suggested = String::with_capacity(name.len());
    for character in name.chars().flat_map(char::to_lowercase) {
        let character = match character {
            '_' | '.' => '-',
            other => other,
        };
        if character == '-' && suggested.ends_with('-') {
            continue;
        }
        suggested.push(character);
    }
    suggested.trim_matches('-').to_owned()
}

/// A repository's name as an address of its remote names it: the last
/// segment, without `.git`. [`None`] when nothing is left.
///
/// Every form Git takes for an address ends in the name:
/// `https://github.com/<owner>/<name>.git`, `git@github.com:<owner>/<name>`,
/// a path on disk.
#[must_use]
pub fn name_from_address(address: &str) -> Option<String> {
    let trimmed = address.trim().trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', ':', '\\']).next().unwrap_or(trimmed);
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then(|| name.to_owned())
}

/// The branch a symbolic `refs/remotes/origin/HEAD` names, from what
/// `git symbolic-ref` prints for it. [`None`] when it names no branch of
/// `origin`.
#[must_use]
pub fn branch_from_head(target: &str) -> Option<String> {
    target
        .trim()
        .strip_prefix("refs/remotes/origin/")
        .filter(|branch| !branch.is_empty())
        .map(ToOwned::to_owned)
}

/// How a minimum file may be named.
#[derive(Debug, Clone, Copy)]
enum Name {
    /// Exactly one of these, letter for letter.
    Exact(&'static [&'static str]),
    /// This word, in any case, alone or followed by an extension:
    /// `README`, `readme.md`, `README.rst`.
    Stem(&'static str),
    /// Anything that starts with one of these, in any case: `LICENSE`,
    /// `LICENSE-MIT`, `COPYING.txt`.
    Prefix(&'static [&'static str]),
}

impl Name {
    fn accepts(self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        match self {
            Self::Exact(names) => names.contains(&name),
            Self::Stem(stem) => lower == stem || lower.starts_with(&format!("{stem}.")),
            Self::Prefix(prefixes) => prefixes.iter().any(|prefix| lower.starts_with(prefix)),
        }
    }
}

/// A file every repository holds once, at the places and under the names
/// GitHub itself honours for it.
#[derive(Debug, Clone, Copy)]
struct Minimum {
    /// What it is, in the message.
    what: &'static str,
    /// Where it belongs when it is written anew: what the message names.
    written: &'static str,
    /// The folders it counts in; `""` is the root.
    folders: &'static [&'static str],
    /// The names it counts under.
    name: Name,
}

impl Minimum {
    fn held_by(self, path: &RelativePath) -> bool {
        let text = path.to_string();
        let (folder, name) = text.rsplit_once('/').unwrap_or(("", &text));
        self.folders.contains(&folder) && self.name.accepts(name)
    }
}

/// The files every repository holds once.
const MINIMUM_FILES: &[Minimum] = &[
    Minimum {
        what: "README",
        written: "README.md",
        folders: &["", ".github", "docs"],
        name: Name::Stem("readme"),
    },
    Minimum {
        what: "licence or proprietary notice",
        written: "LICENSE",
        folders: &[""],
        name: Name::Prefix(&["license", "licence", "copying", "unlicense"]),
    },
    Minimum {
        what: "AGENTS.md",
        written: "AGENTS.md",
        folders: &[""],
        name: Name::Exact(&["AGENTS.md"]),
    },
    Minimum {
        what: "CODEOWNERS",
        written: ".github/CODEOWNERS",
        folders: &["", "docs", ".github"],
        name: Name::Exact(&["CODEOWNERS"]),
    },
    Minimum {
        what: "Dependabot configuration",
        written: ".github/dependabot.yml",
        folders: &[".github"],
        name: Name::Exact(&["dependabot.yml", "dependabot.yaml"]),
    },
    Minimum {
        what: ".gitignore",
        written: ".gitignore",
        folders: &[""],
        name: Name::Exact(&[".gitignore"]),
    },
    Minimum {
        what: ".editorconfig",
        written: ".editorconfig",
        folders: &[""],
        name: Name::Exact(&[".editorconfig"]),
    },
];

/// Every departure from the conventions, as warnings: the name, the default
/// branch, then each minimum file the repository does not hold, at the path
/// where it belongs. A fact that is not known is not checked; [`notes`] says
/// so.
#[must_use]
pub fn findings<'a>(
    identity: &Identity,
    files: impl IntoIterator<Item = &'a RelativePath>,
) -> Vec<Finding> {
    let mut found = Vec::new();
    if let Told::Known(name) = &identity.name
        && !conforming_name(name)
    {
        found.push(Finding::warning(
            Rule::NonconformingName,
            format!(
                "the repository is named {name}, and a name is lowercase letters and digits \
                 joined by single hyphens; {} would conform",
                suggested_name(name)
            ),
        ));
    }
    if let Told::Known(branch) = &identity.default_branch
        && branch != DEFAULT_BRANCH
    {
        found.push(Finding::warning(
            Rule::NonconformingDefaultBranch,
            format!(
                "the default branch is {branch}, and every repository's is {DEFAULT_BRANCH}: \
                 the pipeline gives every version out on {DEFAULT_BRANCH}"
            ),
        ));
    }
    let files: Vec<&RelativePath> = files.into_iter().collect();
    for minimum in MINIMUM_FILES {
        if files.iter().any(|path| minimum.held_by(path)) {
            continue;
        }
        let path = minimum
            .written
            .split('/')
            .fold(RelativePath::root(), |path, segment| path.join(segment));
        found.push(
            Finding::warning(
                Rule::MissingMinimumFile,
                format!(
                    "the repository holds no {}, which every repository holds; write it at {}",
                    minimum.what, minimum.written
                ),
            )
            .at(path),
        );
    }
    found
}

/// One line for each fact that is not known and so not checked.
#[must_use]
pub fn notes(identity: &Identity) -> Vec<String> {
    let mut lines = Vec::new();
    if let Told::Unknown(why) = &identity.name {
        lines.push(format!("The repository's name is not checked: {why}."));
    }
    if let Told::Unknown(why) = &identity.default_branch {
        lines.push(format!("The default branch is not checked: {why}."));
    }
    lines
}
