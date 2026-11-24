//! Where the output of a run goes.
//!
//! It stands apart from the interface because every build carries it: the
//! entry point takes it as an argument whether or not the interface was
//! compiled in.

use std::env;
use std::io::{self, IsTerminal as _};

/// Where the output of a run goes.
///
/// It is an argument rather than a question this module asks itself. A test
/// that let it ask would get a truthful answer — and take over the terminal it
/// runs in, or write annotations into the log of the run it is part of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// A terminal. The interface can run.
    Terminal,
    /// Somewhere else: a pipe, a file, a log.
    Elsewhere,
    /// The log of a workflow run on GitHub Actions: no terminal, and `check`
    /// reports every finding as an annotation of the run, so that it shows on
    /// the run and on the pull request.
    ///
    /// Only where no terminal is attached: a run's log never is one, and a
    /// terminal that is attached in a run (a pseudo terminal a test opens, a
    /// local runner) is still a terminal the interface can use.
    Workflow,
}

impl Output {
    /// Where this process writes. A workflow run says so with
    /// `GITHUB_ACTIONS=true`, which the runner sets for every step.
    #[must_use]
    pub fn current() -> Self {
        Self::detect(
            env::var("GITHUB_ACTIONS").as_deref() == Ok("true"),
            io::stdout().is_terminal(),
        )
    }

    /// From the answers to "is this a workflow run?" and "is a terminal
    /// attached?". A terminal wins: a run's log is never one, so a terminal
    /// that is attached is one even in a run.
    #[must_use]
    pub fn detect(workflow: bool, terminal: bool) -> Self {
        if !terminal && workflow {
            Self::Workflow
        } else {
            Self::of(terminal)
        }
    }

    /// From the answer to "is a terminal attached?".
    #[must_use]
    pub fn of(terminal: bool) -> Self {
        if terminal {
            Self::Terminal
        } else {
            Self::Elsewhere
        }
    }
}
