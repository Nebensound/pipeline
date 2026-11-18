//! Where the output of a run goes.
//!
//! It stands apart from the interface because every build carries it: the
//! entry point takes it as an argument whether or not the interface was
//! compiled in.

use std::io::{self, IsTerminal as _};

/// Where the output of a run goes.
///
/// It is an argument rather than a question this module asks itself. A test
/// that let it ask would get a truthful answer — and take over the terminal it
/// runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// A terminal. The interface can run.
    Terminal,
    /// Somewhere else: a pipe, a file, a log.
    Elsewhere,
}

impl Output {
    /// Where this process writes.
    #[must_use]
    pub fn current() -> Self {
        Self::of(io::stdout().is_terminal())
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

/// Called by nothing, and left out of coverage on purpose: it proves that a
/// function marked `coverage(off)` does not lower the measured coverage.
#[cfg_attr(coverage_nightly, coverage(off))]
#[must_use]
pub fn uncovered() -> u8 {
    1
}
