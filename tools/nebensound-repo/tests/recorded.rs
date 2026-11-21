//! Recorded histories, with the versions they resolve to.
//!
//! Within an edition the versions of an existing history never change with the
//! version of the tool (see `docs/declaration.md`, Editions). These tests hold
//! that promise for edition 1. Each builds a history commit by commit, the way
//! the runs of a repository meet it, and writes down what every run shows: the
//! version of each unit at its commit, the current version of each unit the
//! commit does not touch, and the code of every finding. The whole account is
//! compared with the one recorded beside the test, and read again at the end,
//! every commit of the line still shows what it showed. A change of the tool
//! that moves one version fails here; such a change comes as a new edition.
//!
//! No tag is set anywhere: none is read for a version.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use nebensound_repo::UnitName;
use nebensound_repo::repository::Repository;
use support::{Git, central};

/// What a run at the commit at hand shows, as one line of the account: for
/// every unit that carries a version its name (`repo` for the repository) and
/// the version of the commit, `(x)` when the commit does not touch it and its
/// current version is `x`, `-` when it has none yet; then the code of every
/// finding in brackets. `does not read` when phase 0 stops before any finding.
fn shown(git: &Git) -> String {
    let Ok(analysis) = Repository::at(git.path()).analyse(&central()) else {
        return "does not read".to_owned();
    };
    let mut cells: Vec<String> = analysis
        .versions
        .iter()
        .map(|unit| {
            let name = match &unit.name {
                UnitName::Repository => "repo".to_owned(),
                UnitName::Component(name) => name.to_string(),
            };
            let value = match (&unit.version, &unit.current) {
                (Some(version), current) => {
                    // The current version is the commit's own where it has
                    // one.
                    assert_eq!(current.as_ref(), Some(version), "{name}");
                    version.to_string()
                }
                (None, Some(current)) => format!("({current})"),
                (None, None) => "-".to_owned(),
            };
            format!("{name} {value}")
        })
        .collect();
    let mut codes: Vec<&str> = analysis
        .findings()
        .iter()
        .map(|finding| finding.rule.code())
        .collect();
    codes.sort_unstable();
    codes.dedup();
    cells.extend(codes.into_iter().map(|code| format!("[{code}]")));
    cells.join(", ")
}

/// A history as it is built, with the account of every run on it.
struct Recorder {
    git: Git,
    /// What happened, and what the run after it showed.
    account: Vec<(String, String)>,
    /// What the last run at each commit of `main` showed, by commit.
    last: BTreeMap<String, String>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            git: Git::new(),
            account: Vec::new(),
            last: BTreeMap::new(),
        }
    }

    /// Writes files into the working tree.
    fn write(&self, files: &[(&str, &str)]) -> &Self {
        self.git.write(files);
        self
    }

    /// One call of git.
    fn git(&self, args: &[&str]) -> &Self {
        self.git.run(args);
        self
    }

    /// Commits everything and notes what the run of the commit shows.
    fn commit(&mut self, message: &str) {
        self.git.commit(message);
        self.note(message);
    }

    /// Promotes the tip of `main` and notes what the run of the promoted
    /// commit shows once the merge into `release` exists.
    fn promote(&mut self) {
        self.git.promote();
        self.note("(promoted)");
    }

    /// Merges a branch into `main` with a merge commit and notes what the run
    /// of the merge shows.
    fn merge(&mut self, branch: &str, message: &str) {
        self.git
            .run(&["merge", "--quiet", "--no-ff", "--message", message, branch]);
        self.note(message);
    }

    /// Notes what the run at the commit at hand shows.
    fn note(&mut self, what: &str) {
        let shown = shown(&self.git);
        self.last.insert(self.git.head(), shown.clone());
        self.account.push((what.to_owned(), shown));
    }

    /// The account, once every commit of `main` read again has shown what its
    /// last run showed.
    fn finish(self) -> String {
        let line = self.git.run(&["rev-list", "--first-parent", "main"]);
        for commit in line.lines() {
            self.git.run(&["checkout", "--quiet", "--detach", commit]);
            assert_eq!(
                Some(&shown(&self.git)),
                self.last.get(commit),
                "{commit} read again"
            );
        }
        self.git.run(&["checkout", "--quiet", "main"]);
        let width = self
            .account
            .iter()
            .map(|(what, _)| what.chars().count())
            .max()
            .unwrap_or_default();
        let mut text = String::new();
        for (what, shown) in &self.account {
            writeln!(text, "{what:width$} | {shown}").expect("a String takes any text");
        }
        text
    }
}

/// Fails with every line that differs when an account is not the recorded one.
fn holds(account: &str, recorded: &str) {
    if account == recorded {
        return;
    }
    let mut differences = String::new();
    let (mut now, mut then) = (account.lines(), recorded.lines());
    loop {
        match (now.next(), then.next()) {
            (None, None) => break,
            (now, then) if now == then => {}
            (now, then) => {
                writeln!(
                    differences,
                    "recorded: {}\nnow:      {}",
                    then.unwrap_or("(nothing)"),
                    now.unwrap_or("(nothing)")
                )
                .expect("a String takes any text");
            }
        }
    }
    panic!(
        "a recorded history resolves to other versions than it did: within an edition that is \
         a broken promise\n\n{differences}\nthe whole account now:\n{account}"
    );
}

/// The repository and a component `app`, both released in one stage.
const ONE_STAGE: &str = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.app]\npath     = \"app\"\npipeline = \"none\"\n";

#[test]
fn a_history_in_one_stage() {
    let floored = |repo: &str, app: &str| {
        ONE_STAGE
            .replace(
                "[repo]\naudience = \"public\"\n",
                &format!("[repo]\naudience = \"public\"\n{repo}"),
            )
            .replace(
                "path     = \"app\"\n",
                &format!("path     = \"app\"\n{app}"),
            )
    };
    let lifted = floored("minimum-version = \"0.1.0\"\n", "");
    let mut history = Recorder::new();
    history.write(&[("README.md", "# One\n")]);
    history.commit("chore: before the declaration");
    history.write(&[
        (".nebensound/repo.toml", ONE_STAGE),
        ("app/a.md", "# App\n"),
    ]);
    history.commit("feat: declare the repository and the app");
    history.write(&[("app/a.md", "# App\n\nFixed.\n")]);
    history.commit("fix(app): a correction");
    history.write(&[("README.md", "# One\n\nWords.\n")]);
    history.commit("docs: the readme only");
    history.write(&[("README.md", "# One\n\nMore words.\n")]);
    history.commit("Update the readme");

    // The steps to 0.1.0 and to 1.0.0 are taken with minimum-version.
    history.write(&[(".nebensound/repo.toml", &lifted)]);
    history.commit("chore: take the step to 0.1.0");
    history.write(&[("app/a.md", "# App\n\nBroken.\n")]);
    history.commit("feat(app)!: a breaking change below 0.1.0");
    history.write(&[("README.md", "# One\n\nA feature.\n")]);
    history.commit("feat: a feature below 1.0.0");
    history.write(&[(
        ".nebensound/repo.toml",
        &floored(
            "minimum-version = \"0.1.0\"\n",
            "minimum-version = \"1.0.0\"\n",
        ),
    )]);
    history.commit("chore: take the app to 1.0.0");
    history.write(&[("app/a.md", "# App\n\nA feature.\n")]);
    history.commit("feat(app): a feature");
    history.write(&[("app/a.md", "# App\n\nBroken again.\n")]);
    history.commit("fix(app)!: a breaking correction");
    history.write(&[(".nebensound/repo.toml", &lifted)]);
    history.commit("chore: drop the app's floor");

    // A merge commit touches nothing; only the first-parent line counts.
    history.git(&["checkout", "--quiet", "-b", "side"]);
    history.write(&[("app/a.md", "# App\n\nOn the side.\n")]);
    history.git(&["add", "--all"]).git(&[
        "commit",
        "--quiet",
        "--message",
        "fix(app): on a side branch",
    ]);
    history.git(&["checkout", "--quiet", "main"]);
    history.merge("side", "Merge the side branch");

    // A unit that moves keeps counting.
    history.git(&["mv", "app", "apps"]);
    let moved = lifted.replace("path     = \"app\"\n", "path     = \"apps\"\n");
    history.write(&[(".nebensound/repo.toml", &moved)]);
    history.commit("refactor(app): move the app to apps");
    history.write(&[("apps/a.md", "# App\n\nMoved.\n")]);
    history.commit("fix(app): after the move");

    // Only the commits that give a unit a version count for it.
    let tool = "\n[component.tool]\npath     = \"tool\"\npipeline = \"none\"\n";
    let with_tool = format!("{moved}{tool}");
    history.write(&[
        (
            ".nebensound/repo.toml",
            &format!("{with_tool}versioning = \"none\"\n"),
        ),
        ("tool/t.md", "# Tool\n"),
    ]);
    history.commit("feat: declare the tool without a version");
    history.write(&[(".nebensound/repo.toml", &with_tool)]);
    history.commit("feat: give the tool a version");
    history.write(&[("tool/t.md", "# Tool\n\nFixed.\n")]);
    history.commit("fix(tool): a correction");
    history.write(&[(".nebensound/repo.toml", &moved)]);
    history.commit("refactor: the tool is no unit any more");
    history.write(&[("tool/t.md", "# Tool\n\nPlain files.\n")]);
    history.commit("fix: the tool as plain files");
    history.write(&[(".nebensound/repo.toml", &with_tool)]);
    history.commit("feat: the tool is a unit again");

    // A declaration that does not read counts with what the next readable
    // one says, for every unit that one gives a version.
    history.write(&[(".nebensound/repo.toml", "[repo\n")]);
    history.commit("chore: a declaration that does not read");
    history.write(&[(".nebensound/repo.toml", &with_tool)]);
    history.commit("fix: the declaration reads again");

    // One stage gives out no pre-release: a pre-release floor the versions
    // have not reached is an error, and the commit counts all the same.
    let aimed = |floor: &str| {
        with_tool.replace(
            "path     = \"apps\"\n",
            &format!("path     = \"apps\"\nminimum-version = \"{floor}\"\n"),
        )
    };
    history.write(&[(".nebensound/repo.toml", &aimed("3.0.0-pre.1"))]);
    history.commit("chore: aim the app at a pre-release");
    history.write(&[(".nebensound/repo.toml", &aimed("3.0.0"))]);
    history.commit("chore: take the app to 3.0.0");

    holds(&history.finish(), RECORDED_ONE_STAGE);
}

/// What every run of [`a_history_in_one_stage`] showed.
const RECORDED_ONE_STAGE: &str = concat!(
    "chore: before the declaration             | does not read\n",
    "feat: declare the repository and the app  | repo 0.0.1, app 0.0.1\n",
    "fix(app): a correction                    | repo 0.0.2, app 0.0.2\n",
    "docs: the readme only                     | repo 0.0.3, app (0.0.2)\n",
    "Update the readme                         | repo 0.0.4, app (0.0.2), [unconventional-commit]\n",
    "chore: take the step to 0.1.0             | repo 0.1.0, app 0.0.3\n",
    "feat(app)!: a breaking change below 0.1.0 | repo 0.2.0, app 0.0.4\n",
    "feat: a feature below 1.0.0               | repo 0.2.1, app (0.0.4)\n",
    "chore: take the app to 1.0.0              | repo 0.2.2, app 1.0.0\n",
    "feat(app): a feature                      | repo 0.2.3, app 1.1.0\n",
    "fix(app)!: a breaking correction          | repo 0.3.0, app 2.0.0\n",
    "chore: drop the app's floor               | repo 0.3.1, app 2.0.1\n",
    "Merge the side branch                     | repo (0.3.1), app (2.0.1)\n",
    "refactor(app): move the app to apps       | repo 0.3.2, app 2.0.2\n",
    "fix(app): after the move                  | repo 0.3.3, app 2.0.3\n",
    "feat: declare the tool without a version  | repo 0.3.4, app 2.1.0\n",
    "feat: give the tool a version             | repo 0.3.5, app 2.2.0, tool 0.0.1\n",
    "fix(tool): a correction                   | repo 0.3.6, app (2.2.0), tool 0.0.2\n",
    "refactor: the tool is no unit any more    | repo 0.3.7, app 2.2.1\n",
    "fix: the tool as plain files              | repo 0.3.8, app (2.2.1)\n",
    "feat: the tool is a unit again            | repo 0.3.9, app 2.3.0, tool 0.0.3\n",
    "chore: a declaration that does not read   | does not read\n",
    "fix: the declaration reads again          | repo 0.3.11, app 2.3.2, tool 0.0.5\n",
    "chore: aim the app at a pre-release       | repo 0.3.12, app (2.3.2), tool 0.0.6, [pre-release-minimum]\n",
    "chore: take the app to 3.0.0              | repo 0.3.13, app 3.0.0, tool 0.0.7\n",
);

/// The repository in one stage and a component `lib` in two.
const TWO_STAGES: &str = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.lib]\npath     = \"lib\"\npipeline = \"none\"\nrelease  = \"two-stage\"\n";

#[test]
fn a_history_in_two_stages() {
    let one_stage = TWO_STAGES.replace("release  = \"two-stage\"\n", "");
    let mut history = Recorder::new();
    history.write(&[
        (".nebensound/repo.toml", TWO_STAGES),
        ("README.md", "# Two\n"),
        ("lib/b.md", "# Lib\n"),
    ]);
    history.git.commit("chore: start");
    // The branch release exists from the first commit on.
    history.git(&["branch", "release"]).git(&[
        "update-ref",
        "refs/remotes/origin/release",
        "release",
    ]);
    history.note("chore: start");
    history.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    history.commit("fix(lib): one");
    history.write(&[("lib/b.md", "# Lib\n\nTwo.\n")]);
    history.commit("feat(lib): two");
    history.promote();

    // Promoted with nothing since the last promotion, it releases nothing.
    history.write(&[("README.md", "# Two\n\nWords.\n")]);
    history.commit("docs: the readme only");
    history.promote();

    // A pre-release floor sets the number of the next pre-release.
    let aimed = TWO_STAGES.replace(
        "release  = \"two-stage\"\n",
        "release  = \"two-stage\"\nminimum-version = \"1.0.0-pre.5\"\n",
    );
    history.write(&[(".nebensound/repo.toml", &aimed)]);
    history.commit("chore: aim the library at 1.0.0-pre.5");
    history.write(&[("lib/b.md", "# Lib\n\nThree.\n")]);
    history.commit("fix(lib): the next correction");
    history.promote();
    history.write(&[("lib/b.md", "# Lib\n\nFour.\n")]);
    history.commit("feat(lib): a feature");
    history.write(&[("lib/b.md", "# Lib\n\nFive.\n")]);
    history.commit("fix(lib)!: a breaking correction");
    history.promote();

    // A switch to one stage after a promotion goes on from it, and one back
    // to two stages from the releases of one stage.
    history.write(&[(".nebensound/repo.toml", &one_stage)]);
    history.commit("chore: release the library in one stage");
    history.write(&[("lib/b.md", "# Lib\n\nSix.\n")]);
    history.commit("fix(lib): in one stage");
    history.write(&[(".nebensound/repo.toml", TWO_STAGES)]);
    history.commit("chore: release the library in two stages again");

    // A switch to one stage before promoting is an error until the switch is
    // reverted and the tip promoted.
    history.write(&[(".nebensound/repo.toml", &one_stage)]);
    history.commit("chore: switch back before promoting");
    history.write(&[(".nebensound/repo.toml", TWO_STAGES)]);
    history.commit("revert: release the library in two stages");
    history.promote();
    history.write(&[(".nebensound/repo.toml", &one_stage)]);
    history.commit("chore: release the library in one stage after the promotion");

    holds(&history.finish(), RECORDED_TWO_STAGES);
}

/// What every run of [`a_history_in_two_stages`] showed.
const RECORDED_TWO_STAGES: &str = concat!(
    "chore: start                                                | repo 0.0.1, lib 0.0.1-pre.1\n",
    "fix(lib): one                                               | repo 0.0.2, lib 0.0.1-pre.2\n",
    "feat(lib): two                                              | repo 0.0.3, lib 0.0.1-pre.3\n",
    "(promoted)                                                  | repo 0.0.3, lib 0.0.1\n",
    "docs: the readme only                                       | repo 0.0.4, lib (0.0.1)\n",
    "(promoted)                                                  | repo 0.0.4, lib (0.0.1)\n",
    "chore: aim the library at 1.0.0-pre.5                       | repo 0.0.5, lib 1.0.0-pre.5\n",
    "fix(lib): the next correction                               | repo 0.0.6, lib 1.0.0-pre.6\n",
    "(promoted)                                                  | repo 0.0.6, lib 1.0.0\n",
    "feat(lib): a feature                                        | repo 0.0.7, lib 1.1.0-pre.1\n",
    "fix(lib)!: a breaking correction                            | repo 0.0.8, lib 2.0.0-pre.2\n",
    "(promoted)                                                  | repo 0.0.8, lib 2.0.0\n",
    "chore: release the library in one stage                     | repo 0.0.9, lib 2.0.1\n",
    "fix(lib): in one stage                                      | repo 0.0.10, lib 2.0.2\n",
    "chore: release the library in two stages again              | repo 0.0.11, lib 2.0.3-pre.1\n",
    "chore: switch back before promoting                         | repo 0.0.12, lib (2.0.3-pre.1), [unreleased-before-switch]\n",
    "revert: release the library in two stages                   | repo 0.0.13, lib 2.0.3-pre.3\n",
    "(promoted)                                                  | repo 0.0.13, lib 2.0.3\n",
    "chore: release the library in one stage after the promotion | repo 0.0.14, lib 2.0.4\n",
);

/// The repository and a component `lib`, both released in one stage.
const LIB: &str = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.lib]\npath     = \"lib\"\npipeline = \"none\"\n";

/// A history whose first commits hold a declaration this reader cannot read,
/// as one written for another reader would be, until one that reads follows.
/// With `release`, the branch `release` exists from the first commit on.
fn adopted(history: &mut Recorder, release: bool) {
    let legacy = LIB.replace(
        "[repo]\naudience = \"public\"\n",
        "[repo]\naudience = \"public\"\nlegacy-field = true\n",
    );
    history.write(&[
        (".nebensound/repo.toml", &legacy),
        ("README.md", "# Adopted\n"),
        ("lib/b.md", "# Lib\n"),
    ]);
    let first = "chore: a declaration for another reader";
    history.git.commit(first);
    if release {
        history.git(&["branch", "release"]).git(&[
            "update-ref",
            "refs/remotes/origin/release",
            "release",
        ]);
    }
    history.note(first);
    history.write(&[("lib/b.md", "# Lib\n\nOne.\n")]);
    history.commit("fix(lib): one");
    history.write(&[("README.md", "# Adopted\n\nWords.\n")]);
    history.commit("docs: the readme only");
    history.write(&[("lib/b.md", "# Lib\n\nTwo.\n")]);
    history.commit("fix(lib): two");
    // From here on the commits before count with what this declaration
    // says: the library at `lib`, in one stage.
    history.write(&[(".nebensound/repo.toml", LIB)]);
    history.commit("chore: adopt the declaration");
    history.write(&[("lib/b.md", "# Lib\n\nThree.\n")]);
    history.commit("fix(lib): after the adoption");
}

#[test]
fn a_history_that_starts_with_declarations_that_do_not_read_then_moves() {
    let moved = LIB.replace("path     = \"lib\"\n", "path     = \"libs\"\n");
    let mut history = Recorder::new();
    adopted(&mut history, false);

    // The library moves: the commits whose declaration does not read are not
    // counted again at `libs`, where only the first of them would touch it.
    history.git(&["mv", "lib", "libs"]);
    history.write(&[(".nebensound/repo.toml", &moved)]);
    history.commit("refactor(lib): move the library to libs");

    // They do not count for a unit the readable declaration after them does
    // not name, although the first of them changed the declaration's file.
    history.write(&[
        (
            ".nebensound/repo.toml",
            &format!("{moved}\n[component.tool]\npath     = \"tool\"\npipeline = \"none\"\n"),
        ),
        ("tool/t.md", "# Tool\n"),
    ]);
    history.commit("feat: declare the tool");

    holds(&history.finish(), RECORDED_ADOPTED_THEN_MOVED);
}

/// What every run of
/// [`a_history_that_starts_with_declarations_that_do_not_read_then_moves`]
/// showed.
const RECORDED_ADOPTED_THEN_MOVED: &str = concat!(
    "chore: a declaration for another reader | does not read\n",
    "fix(lib): one                           | does not read\n",
    "docs: the readme only                   | does not read\n",
    "fix(lib): two                           | does not read\n",
    "chore: adopt the declaration            | repo 0.0.5, lib 0.0.4\n",
    "fix(lib): after the adoption            | repo 0.0.6, lib 0.0.5\n",
    "refactor(lib): move the library to libs | repo 0.0.7, lib 0.0.6\n",
    "feat: declare the tool                  | repo 0.0.8, lib 0.0.7, tool 0.0.1\n",
);

#[test]
fn a_history_that_starts_with_declarations_that_do_not_read_then_switches() {
    let mut history = Recorder::new();
    adopted(&mut history, true);

    // The library switches to two stages: the commits whose declaration does
    // not read stay releases of one stage, and its first target goes on from
    // the release before the switch.
    history.write(&[(
        ".nebensound/repo.toml",
        &format!("{LIB}release  = \"two-stage\"\n"),
    )]);
    history.commit("feat: release the library in two stages");
    history.promote();

    holds(&history.finish(), RECORDED_ADOPTED_THEN_SWITCHED);
}

/// What every run of
/// [`a_history_that_starts_with_declarations_that_do_not_read_then_switches`]
/// showed.
const RECORDED_ADOPTED_THEN_SWITCHED: &str = concat!(
    "chore: a declaration for another reader | does not read\n",
    "fix(lib): one                           | does not read\n",
    "docs: the readme only                   | does not read\n",
    "fix(lib): two                           | does not read\n",
    "chore: adopt the declaration            | repo 0.0.5, lib 0.0.4, [pointless-release-branch]\n",
    "fix(lib): after the adoption            | repo 0.0.6, lib 0.0.5, [pointless-release-branch]\n",
    "feat: release the library in two stages | repo 0.0.7, lib 0.0.6-pre.1\n",
    "(promoted)                              | repo 0.0.7, lib 0.0.6\n",
);

/// The repository and a component `docs` that generates a unit per folder.
const GENERATED: &str = "[repo]\naudience = \"public\"\npipeline = \"none\"\n\n[component.docs]\npath       = \"docs\"\npipeline   = \"none\"\nversioning = \"child-folders\"\n";

#[test]
fn a_history_of_generated_units() {
    let lifted = GENERATED.replace(
        "versioning = \"child-folders\"\n",
        "versioning = \"child-folders\"\nminimum-version = \"0.2.0\"\n",
    );
    let moved = lifted.replace("path       = \"docs\"\n", "path       = \"books\"\n");
    let mut history = Recorder::new();
    history.write(&[
        (".nebensound/repo.toml", GENERATED),
        ("docs/guide/a.md", "# Guide\n"),
    ]);
    history.commit("chore: start");
    history.write(&[("docs/handbook/a.md", "# Handbook\n")]);
    history.commit("feat: add a handbook");
    history.write(&[("docs/guide/a.md", "# Guide\n\nFixed.\n")]);
    history.commit("fix(guide): a correction");

    // The children take the floor of their parent.
    history.write(&[(".nebensound/repo.toml", &lifted)]);
    history.commit("chore: lift the docs to 0.2.0");

    // The parent moves, and its children keep counting.
    history.git(&["mv", "docs", "books"]);
    history.write(&[(".nebensound/repo.toml", &moved)]);
    history.commit("refactor: move the docs to books");
    history.write(&[("books/handbook/a.md", "# Handbook\n\nMoved.\n")]);
    history.commit("fix(handbook): after the move");

    // A child that goes and comes back goes on from where it was.
    history.git(&["rm", "--quiet", "-r", "books/guide"]);
    history.commit("refactor: drop the guide");
    history.write(&[("books/guide/a.md", "# Guide\n\nBack.\n")]);
    history.commit("feat: the guide comes back");

    holds(&history.finish(), RECORDED_GENERATED);
}

/// What every run of [`a_history_of_generated_units`] showed.
const RECORDED_GENERATED: &str = concat!(
    "chore: start                     | repo 0.0.1, guide 0.0.1\n",
    "feat: add a handbook             | repo 0.0.2, guide (0.0.1), handbook 0.0.1\n",
    "fix(guide): a correction         | repo 0.0.3, guide 0.0.2, handbook (0.0.1)\n",
    "chore: lift the docs to 0.2.0    | repo 0.0.4, guide 0.2.0, handbook 0.2.0\n",
    "refactor: move the docs to books | repo 0.0.5, guide 0.2.1, handbook 0.2.1\n",
    "fix(handbook): after the move    | repo 0.0.6, guide (0.2.1), handbook 0.2.2\n",
    "refactor: drop the guide         | repo 0.0.7, handbook (0.2.2)\n",
    "feat: the guide comes back       | repo 0.0.8, guide 0.2.2, handbook (0.2.2)\n",
);

/// The repository at `1.0.0` and a component `app` below it, both in one
/// stage, before `feat-types` names a type of their own.
const OWN_TYPES: &str = "[repo]\naudience = \"public\"\npipeline = \"none\"\nminimum-version = \"1.0.0\"\n\n[component.app]\npath     = \"app\"\npipeline = \"none\"\nminimum-version = \"0.1.0\"\n";

#[test]
fn a_history_with_types_of_its_own() {
    let rules = OWN_TYPES.replace("[repo]\n", "[repo]\nfeat-types = [\"rule\"]\n");
    let mut history = Recorder::new();
    history.write(&[
        (".nebensound/repo.toml", OWN_TYPES),
        ("README.md", "# Rules\n"),
        ("app/a.md", "# App\n"),
    ]);
    history.commit("chore: start");
    // Before the field, a type of its own is everything else.
    history.write(&[("README.md", "# Rules\n\nOne.\n")]);
    history.commit("rule: a rule before the field");

    // The commit that writes the field is read with it.
    history.write(&[(".nebensound/repo.toml", &rules)]);
    history.commit("rule: count rules like feat");
    history.write(&[("README.md", "# Rules\n\nTwo.\n")]);
    history.commit("rule: a new rule");
    history.write(&[("README.md", "# Rules\n\nThree.\n")]);
    history.commit("Rule(readme): the type ignores case");
    // Below 1.0.0 a listed type is a patch, as feat is, and the breaking
    // marker stays what it is.
    history.write(&[("app/a.md", "# App\n\nA rule.\n")]);
    history.commit("rule(app): a rule below 1.0.0");
    history.write(&[("app/a.md", "# App\n\nBroken.\n")]);
    history.commit("rule(app)!: a breaking rule");
    history.write(&[("README.md", "# Rules\n\nFour.\n")]);
    history.commit("announcement: a type that is not listed");

    // A declaration that does not read is read with the list of the next
    // readable one.
    history.write(&[(".nebensound/repo.toml", "[repo\n")]);
    history.commit("rule: a declaration that does not read");
    history.write(&[(".nebensound/repo.toml", &rules)]);
    history.commit("fix: the declaration reads again");

    // Without the field, each commit is read as before, the one that drops
    // it included.
    history.write(&[(".nebensound/repo.toml", OWN_TYPES)]);
    history.commit("rule: drop the field");
    history.write(&[("README.md", "# Rules\n\nFive.\n")]);
    history.commit("rule: after the field");

    holds(&history.finish(), RECORDED_OWN_TYPES);
}

/// What every run of [`a_history_with_types_of_its_own`] showed.
const RECORDED_OWN_TYPES: &str = concat!(
    "chore: start                            | repo 1.0.0, app 0.1.0\n",
    "rule: a rule before the field           | repo 1.0.1, app (0.1.0)\n",
    "rule: count rules like feat             | repo 1.1.0, app 0.1.1\n",
    "rule: a new rule                        | repo 1.2.0, app (0.1.1)\n",
    "Rule(readme): the type ignores case     | repo 1.3.0, app (0.1.1)\n",
    "rule(app): a rule below 1.0.0           | repo 1.4.0, app 0.1.2\n",
    "rule(app)!: a breaking rule             | repo 2.0.0, app 0.2.0\n",
    "announcement: a type that is not listed | repo 2.0.1, app (0.2.0)\n",
    "rule: a declaration that does not read  | does not read\n",
    "fix: the declaration reads again        | repo 2.1.1, app 0.2.2\n",
    "rule: drop the field                    | repo 2.1.2, app 0.2.3\n",
    "rule: after the field                   | repo 2.1.3, app (0.2.3)\n",
);
