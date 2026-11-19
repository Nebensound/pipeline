//! Unpacking a downloaded build into its folder.
//!
//! A build comes as a single file, a single file compressed with gzip, a tar
//! archive compressed with gzip, or a zip archive; which one, its address says
//! by its ending. Archives are unpacked whole, and the program is the path the
//! register names inside them.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;

use super::cannot;

/// How a download is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    /// The program itself.
    Plain,
    /// The program, compressed with gzip.
    Gzip,
    /// A tar archive compressed with gzip.
    TarGz,
    /// A zip archive.
    Zip,
}

impl Packing {
    /// How the download at this address is packed, by its ending.
    #[must_use]
    pub fn of(url: &str) -> Self {
        let url = url.to_ascii_lowercase();
        let ends = |ending: &str| url.ends_with(ending);
        if ends(".tar.gz") || ends(".tgz") {
            Self::TarGz
        } else if ends(".gz") {
            Self::Gzip
        } else if ends(".zip") {
            Self::Zip
        } else {
            Self::Plain
        }
    }
}

/// Unpacks a download into `dir` and returns the program in it, made
/// executable.
///
/// A single file is saved as `executable`; an archive is unpacked whole, and
/// `executable` is the program's path inside it. Neither the tar nor the zip
/// reader writes outside `dir`, whatever the archive's paths say.
///
/// # Errors
///
/// Returns a sentence when the download cannot be read or unpacked, or holds
/// no such program.
pub fn unpack(
    file: &Path,
    packing: Packing,
    dir: &Path,
    executable: &str,
) -> Result<PathBuf, String> {
    let source = File::open(file).map_err(cannot("cannot be read", file))?;
    let target = dir.join(executable);
    match packing {
        Packing::Plain => write(source, &target)?,
        Packing::Gzip => write(GzDecoder::new(source), &target)?,
        Packing::TarGz => tar::Archive::new(GzDecoder::new(source))
            .unpack(dir)
            .map_err(cannot("cannot be unpacked", file))?,
        Packing::Zip => zip::ZipArchive::new(source)
            .and_then(|mut archive| archive.extract(dir))
            .map_err(|error| cannot("cannot be unpacked", file)(io::Error::other(error)))?,
    }
    if !target.is_file() {
        return Err(format!(
            "{} holds no {executable}, which the register names as its program",
            file.display()
        ));
    }
    executable_bit(&target).map(|()| target)
}

/// Saves a single file as the program.
fn write(mut source: impl Read, target: &Path) -> Result<(), String> {
    let mut file = File::create(target).map_err(cannot("cannot be written", target))?;
    io::copy(&mut source, &mut file)
        .map(drop)
        .map_err(cannot("cannot be unpacked into", target))
}

/// Lets everyone run the program, as a release archive would have it.
#[cfg(unix)]
fn executable_bit(target: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(target, fs::Permissions::from_mode(0o755))
        .map_err(cannot("cannot be made executable", target))
}

/// Windows decides by the ending whether a file runs.
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn executable_bit(_: &Path) -> Result<(), String> {
    Ok(())
}
