//! The programs the checks call, and the runtimes some of them run on.
//!
//! A check is carried out by external programs: formatters, linters, a link
//! checker. The register pins each of them to one version and says, per
//! operating system and architecture, where that version comes from and which
//! digest the download has. A program written in JavaScript or Python names
//! its runtime instead of a build per platform: its one package is installed
//! through the runtime's own installer.
//!
//! This is data. Finding a program on a computer, downloading and unpacking it
//! belong to whatever runs the checks.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, de};

use crate::names::RuntimeName;

/// A program a check calls, pinned to one version.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Program {
    /// The version the register pins: three numbers, as the program prints
    /// them when asked for its version.
    pub version: String,
    /// The name it is called by on a computer.
    pub command: String,
    /// The arguments that make it print its version.
    #[serde(default = "version_args")]
    pub version_args: Vec<String>,
    /// The runtime it runs on and is installed through, if it needs one.
    #[serde(default)]
    pub runtime: Option<RuntimeName>,
    /// The lowest version of its runtime it runs on, three numbers, as its
    /// package states it (`requires_python` in the Python Package Index,
    /// `engines.node` in the npm registry). A program on a runtime names one;
    /// any other names none. A runtime on the computer below it cannot run
    /// the program.
    #[serde(default)]
    pub runtime_minimum: Option<String>,
    /// Where the pinned version comes from. A program on a runtime has one
    /// package for every platform ([`Platform::Any`]); any other has a build
    /// per platform.
    pub download: BTreeMap<Platform, Download>,
    /// The packages installed beside a program on a runtime that are no
    /// command of their own, such as the configurations a linter extends,
    /// each pinned like the program. The runtime's installer puts them in
    /// place together with it.
    #[serde(default, rename = "companion")]
    pub companions: Vec<Companion>,
}

/// A package installed beside a program on a runtime, pinned by its digest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Companion {
    /// The name its installer knows it by.
    pub name: String,
    /// The version pinned, three numbers.
    pub version: String,
    /// Where it comes from.
    pub url: String,
    /// The digest the file has, as its publisher states it.
    pub digest: Digest,
}

impl Companion {
    /// The package as a download, which names no executable.
    #[must_use]
    pub fn download(&self) -> Download {
        Download {
            url: self.url.clone(),
            digest: self.digest.clone(),
            executable: None,
        }
    }
}

/// A runtime some programs run on, such as Node.js or Python.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Runtime {
    /// What it is called in words.
    pub title: String,
    /// The version the register pins, three numbers.
    pub version: String,
    /// The names it is called by on a computer, tried in this order.
    pub commands: Vec<String>,
    /// The arguments that make it print its version.
    #[serde(default = "version_args")]
    pub version_args: Vec<String>,
    /// How a program on it is installed.
    pub installer: Installer,
    /// Where the pinned version comes from, a build per platform.
    pub download: BTreeMap<Platform, Download>,
}

fn version_args() -> Vec<String> {
    vec!["--version".to_owned()]
}

/// How a program on a runtime is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Installer {
    /// npm, which comes with Node.js.
    Npm,
    /// pip, in a virtual environment of its own.
    Pip,
}

impl fmt::Display for Installer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Npm => "npm",
            Self::Pip => "pip",
        })
    }
}

/// One file to download, and what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Download {
    /// Where it comes from.
    pub url: String,
    /// The digest the file has, as its publisher states it.
    pub digest: Digest,
    /// The program inside it: its path in an archive, or the name a single
    /// file is saved as. A package for a runtime names none, since the
    /// runtime's installer decides where the program goes.
    #[serde(default)]
    pub executable: Option<String>,
}

/// An operating system and an architecture, the key of a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub enum Platform {
    /// Every platform: a package that is the same everywhere.
    #[serde(rename = "any")]
    Any,
    /// Linux on 64-bit x86.
    #[serde(rename = "linux-x86_64")]
    LinuxX86_64,
    /// Linux on 64-bit ARM.
    #[serde(rename = "linux-aarch64")]
    LinuxAarch64,
    /// macOS on 64-bit x86.
    #[serde(rename = "macos-x86_64")]
    MacosX86_64,
    /// macOS on Apple silicon.
    #[serde(rename = "macos-aarch64")]
    MacosAarch64,
    /// Windows on 64-bit x86.
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
    /// Windows on 64-bit ARM.
    #[serde(rename = "windows-aarch64")]
    WindowsAarch64,
}

impl Platform {
    /// The platform of a computer, from the names Rust gives its operating
    /// system and architecture (`std::env::consts::OS` and `ARCH`), if the
    /// register has a key for it.
    #[must_use]
    pub fn of(os: &str, arch: &str) -> Option<Self> {
        Some(match (os, arch) {
            ("linux", "x86_64") => Self::LinuxX86_64,
            ("linux", "aarch64") => Self::LinuxAarch64,
            ("macos", "x86_64") => Self::MacosX86_64,
            ("macos", "aarch64") => Self::MacosAarch64,
            ("windows", "x86_64") => Self::WindowsX86_64,
            ("windows", "aarch64") => Self::WindowsAarch64,
            _ => return None,
        })
    }

    /// The key as the register writes it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::LinuxX86_64 => "linux-x86_64",
            Self::LinuxAarch64 => "linux-aarch64",
            Self::MacosX86_64 => "macos-x86_64",
            Self::MacosAarch64 => "macos-aarch64",
            Self::WindowsX86_64 => "windows-x86_64",
            Self::WindowsAarch64 => "windows-aarch64",
        }
    }

    /// Whether this is a Windows platform, where a program's file ends in
    /// `.exe` and an installer puts it in other folders.
    #[must_use]
    pub fn is_windows(self) -> bool {
        matches!(self, Self::WindowsX86_64 | Self::WindowsAarch64)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The hash function a digest was made with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// SHA-256: 64 hexadecimal digits.
    Sha256,
    /// SHA-512: 128 hexadecimal digits.
    Sha512,
}

impl Algorithm {
    /// The name as a digest writes it in front.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }

    fn digits(self) -> usize {
        match self {
            Self::Sha256 => 64,
            Self::Sha512 => 128,
        }
    }
}

/// A digest written as `<algorithm>:<hexadecimal digits>`, such as
/// `sha256:8fe1…`, the form GitHub gives a release asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    algorithm: Algorithm,
    hex: String,
}

impl Digest {
    /// Reads a digest.
    ///
    /// # Errors
    ///
    /// Returns a sentence saying what is wrong when the algorithm is neither
    /// `sha256` nor `sha512`, or the digits are not as many lowercase
    /// hexadecimal digits as the algorithm makes.
    pub fn new(value: &str) -> Result<Self, String> {
        let (name, hex) = value
            .split_once(':')
            .ok_or_else(|| format!("{value:?} is no digest: it reads <algorithm>:<digits>"))?;
        let algorithm = match name {
            "sha256" => Algorithm::Sha256,
            "sha512" => Algorithm::Sha512,
            _ => {
                return Err(format!(
                    "{value:?} is no digest: the algorithm is sha256 or sha512"
                ));
            }
        };
        if hex.len() != algorithm.digits()
            || !hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
        {
            return Err(format!(
                "{value:?} is no digest: {name} has {} lowercase hexadecimal digits",
                algorithm.digits()
            ));
        }
        Ok(Self {
            algorithm,
            hex: hex.to_owned(),
        })
    }

    /// The hash function.
    #[must_use]
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// The digits, lowercase hexadecimal.
    #[must_use]
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm.as_str(), self.hex)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(&raw).map_err(de::Error::custom)
    }
}

/// Whether a version is three numbers, the form a program's version is
/// compared in.
#[must_use]
pub fn is_version(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// Whether `version` is `floor` or later, each three numbers, compared number
/// by number. A value that is not three numbers that fit in 64 bits is later
/// than nothing and has nothing later than it.
#[must_use]
pub fn at_least(version: &str, floor: &str) -> bool {
    match (numbers(version), numbers(floor)) {
        (Some(version), Some(floor)) => version >= floor,
        _ => false,
    }
}

fn numbers(value: &str) -> Option<[u64; 3]> {
    if !is_version(value) {
        return None;
    }
    let parts: Vec<u64> = value
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    <[u64; 3]>::try_from(parts).ok()
}

/// What is wrong with the downloads of a program or runtime, if anything.
///
/// `packaged` says whether the entry is a package for a runtime: then it has
/// one download for every platform and names no executable. Otherwise every
/// download is the build for one platform and names the executable in it.
pub(crate) fn download_problem(
    downloads: &BTreeMap<Platform, Download>,
    packaged: bool,
) -> Option<&'static str> {
    if packaged {
        let single = downloads.len() == 1
            && downloads
                .get(&Platform::Any)
                .is_some_and(|download| download.executable.is_none());
        return (!single).then_some(
            "runs on a runtime, so it has one download, for `any`, and names no executable",
        );
    }
    if downloads.is_empty() {
        return Some("has no download");
    }
    if downloads.contains_key(&Platform::Any) {
        return Some("has a download for `any`, which only a program on a runtime has");
    }
    if downloads
        .values()
        .any(|download| download.executable.is_none())
    {
        return Some("has a build that names no executable");
    }
    None
}
