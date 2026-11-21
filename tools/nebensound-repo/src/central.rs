//! What is kept centrally: the vocabulary and the defaults.
//!
//! Both are compiled in so that no repository holds a version that can fall
//! behind. The defaults are themselves a declaration — the same shape, the same
//! parser, the same rules — with one freedom more: centrally, an exclusion may
//! be a pattern. The Dependabot policy stands in the defaults as well, as a
//! table of its own that only they hold.

use thiserror::Error;

use crate::ReadError;
use crate::dependabot::Policy;
use crate::model::{Declaration, Pipeline};
use crate::names::{CheckName, EcosystemName, Reason};
use crate::path::RelativePath;
use crate::pattern::Pattern;
use crate::register::{BUILTIN_DEFAULTS, BUILTIN_REGISTER, Register, RegisterError};
use crate::tree::Tree;

/// What holds for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ruling<'a> {
    /// It belongs in the check.
    Checked,
    /// It is excused, by this entry and with this reason.
    Excluded {
        /// The entry that takes it out.
        pattern: &'a Pattern,
        /// Why.
        reason: &'a Reason,
    },
}

/// An entry that matches the file.
struct Candidate<'a> {
    pattern: &'a Pattern,
    /// The reason, **if** the entry takes the file out. For an inclusion
    /// nothing stands here, even when someone wrote a reason down: what decides
    /// is the direction, not the text.
    excuse: Option<&'a Reason>,
}

impl Candidate<'_> {
    /// What is compared: first the specificity, then the direction. At equal
    /// specificity the inclusion wins.
    fn rank(&self) -> ((u8, usize), bool) {
        (self.pattern.specificity(), self.excuse.is_none())
    }
}

/// Why the central data could not be read.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CentralError {
    /// The register is not readable.
    #[error(transparent)]
    Register(#[from] RegisterError),
    /// The defaults are not readable.
    #[error("the defaults are not readable: {0}")]
    Defaults(#[from] ReadError),
    /// The defaults do not write their base set out.
    #[error(
        "the defaults have to write their pipeline out: the base set stands there, and looking a \
         preset up would mean fetching it out of itself"
    )]
    BaseSetNotWrittenOut,
    /// The defaults hold no Dependabot policy, or one Dependabot would not
    /// take.
    #[error("the Dependabot policy in the defaults {problem}")]
    Policy {
        /// What is wrong with it.
        problem: String,
    },
    /// The defaults name a check the register does not know.
    #[error("the base set names the check {check}, which the register does not know")]
    UnknownBaseCheck {
        /// The unknown check.
        check: String,
    },
}

/// The vocabulary and the defaults together.
#[derive(Debug, Clone)]
pub struct Central {
    register: Register,
    defaults: Declaration,
    base_checks: Vec<CheckName>,
    base_dependabot: Vec<EcosystemName>,
    dependabot: Policy,
}

impl Central {
    /// What is compiled into this crate.
    ///
    /// # Errors
    ///
    /// Returns [`CentralError`] only when this crate was built with broken
    /// data, which its own tests rule out.
    pub fn builtin() -> Result<Self, CentralError> {
        Self::parse(BUILTIN_REGISTER, BUILTIN_DEFAULTS)
    }

    /// Reads the vocabulary and the defaults.
    ///
    /// # Errors
    ///
    /// Returns [`CentralError`] when either of them does not read, the
    /// defaults do not write their base set out, or their Dependabot policy
    /// is missing or one Dependabot would not take.
    pub fn parse(register: &str, defaults: &str) -> Result<Self, CentralError> {
        let register = Register::parse(register)?;
        // The policy is a table only the defaults hold: it is taken out before
        // the rest is read as a declaration, whose shape does not know it, so
        // that a repository's declaration refuses it as an unknown key.
        Declaration::read_edition(defaults)?;
        let mut table: toml::Table = toml::from_str(defaults).map_err(ReadError::from)?;
        let policy = table
            .remove("dependabot")
            .ok_or_else(|| CentralError::Policy {
                problem: "is missing: the defaults hold it under [dependabot]".to_owned(),
            })?;
        let dependabot: Policy = policy.try_into().map_err(ReadError::from)?;
        dependabot
            .check(&register)
            .map_err(|problem| CentralError::Policy { problem })?;
        let defaults = Declaration::from_central_table(table)?;

        let Some(Pipeline::Explicit(written)) = &defaults.repo.pipeline else {
            return Err(CentralError::BaseSetNotWrittenOut);
        };
        for check in &written.check {
            if !register.check.contains_key(check) {
                return Err(CentralError::UnknownBaseCheck {
                    check: check.to_string(),
                });
            }
        }
        let base_checks = written.check.clone();
        for ecosystem in &written.dependabot {
            if !register.ecosystem.contains_key(ecosystem) {
                return Err(CentralError::UnknownBaseCheck {
                    check: ecosystem.to_string(),
                });
            }
        }
        let base_dependabot = written.dependabot.clone();

        Ok(Self {
            register,
            defaults,
            base_checks,
            base_dependabot,
            dependabot,
        })
    }

    /// The vocabulary.
    #[must_use]
    pub fn register(&self) -> &Register {
        &self.register
    }

    /// The defaults, as a declaration.
    #[must_use]
    pub fn defaults(&self) -> &Declaration {
        &self.defaults
    }

    /// The checks that run everywhere without being declared.
    #[must_use]
    pub fn base_checks(&self) -> &[CheckName] {
        &self.base_checks
    }

    /// The ecosystems watched everywhere without being declared.
    #[must_use]
    pub fn base_dependabot(&self) -> &[EcosystemName] {
        &self.base_dependabot
    }

    /// The policy every entry of a `dependabot.yml` follows.
    #[must_use]
    pub fn dependabot(&self) -> &Policy {
        &self.dependabot
    }

    /// What holds for this file, from the defaults and the declaration
    /// together.
    ///
    /// The **more specific** entry wins: a concrete path beats every pattern, a
    /// longer path a shorter one, a file name a type. At equal specificity the
    /// inclusion wins — adding work is always allowed, excusing it is not.
    ///
    /// The tree says whether the file is empty: an entry that holds only for
    /// an empty file is no candidate for one with content, or for one the
    /// tree knows nothing about.
    #[must_use]
    pub fn ruling<'a>(
        &'a self,
        declaration: &'a Declaration,
        file: &RelativePath,
        tree: &Tree,
    ) -> Ruling<'a> {
        let empty = tree.is_empty_file(file);
        let mut best: Option<Candidate<'a>> = None;

        let mut consider = |candidate: Candidate<'a>| {
            if !candidate.pattern.matches(file) {
                return;
            }
            if best
                .as_ref()
                .is_none_or(|held| candidate.rank() > held.rank())
            {
                best = Some(candidate);
            }
        };

        for entry in self.defaults.exclude.iter().chain(&declaration.exclude) {
            if entry.only_if_empty && !empty {
                continue;
            }
            consider(Candidate {
                pattern: &entry.path,
                excuse: Some(&entry.reason),
            });
        }
        for entry in self.defaults.include.iter().chain(&declaration.include) {
            consider(Candidate {
                pattern: &entry.path,
                excuse: None,
            });
        }

        match best {
            Some(Candidate {
                pattern,
                excuse: Some(reason),
            }) => Ruling::Excluded { pattern, reason },
            _ => Ruling::Checked,
        }
    }
}
