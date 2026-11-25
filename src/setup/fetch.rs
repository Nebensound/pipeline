//! Downloading a file and holding it against the digest the register pins.
//!
//! The download goes through curl, which every supported operating system
//! brings (Windows since Windows 10), so the tool links no HTTP and TLS stack
//! of its own. The digest is computed here.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::programs::{Algorithm, Digest, Download};
use sha2::{Sha256, Sha512};

use super::{Environment, cannot, find};

/// Downloads a file into `into`, under the last segment of its address, and
/// refuses it unless it has the digest the register pins: then it is deleted
/// again.
///
/// # Errors
///
/// Returns a sentence saying what went wrong when there is no curl, the
/// download fails, or the file has another digest.
pub fn download(download: &Download, into: &Path, env: &Environment) -> Result<PathBuf, String> {
    fs::create_dir_all(into).map_err(cannot("cannot be made", into))?;
    let file = into.join(file_name(&download.url));
    fetch(&download.url, &file, env)?;
    verify(&file, &download.url, &download.digest).inspect_err(|_| drop(fs::remove_file(&file)))?;
    Ok(file)
}

/// Holds a downloaded file against the digest the register pins.
///
/// # Errors
///
/// Returns a sentence when the file cannot be read or has another digest.
pub fn verify(file: &Path, url: &str, digest: &Digest) -> Result<(), String> {
    let actual = digest_of(file, digest.algorithm()).map_err(cannot("cannot be read", file))?;
    if actual == digest.hex() {
        Ok(())
    } else {
        Err(format!(
            "the download from {url} has the digest {}:{actual}, and the register pins \
             {digest}; it was refused and deleted",
            digest.algorithm().as_str()
        ))
    }
}

/// The name a download is saved under: the last segment of its address,
/// reduced to characters that are safe in a file name everywhere.
#[must_use]
pub fn file_name(url: &str) -> String {
    let last = url.rsplit('/').next().unwrap_or(url);
    let name: String = last
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
        .collect();
    if name.chars().all(|c| c == '.') {
        "download".to_owned()
    } else {
        name
    }
}

/// Fetches one address into one file with curl, following redirects over the
/// protocols the environment allows.
///
/// # Errors
///
/// Returns a sentence when there is no curl on the search path or curl fails.
pub fn fetch(url: &str, to: &Path, env: &Environment) -> Result<(), String> {
    let curl = find::search("curl", &env.search_path, env.windows)
        .ok_or_else(|| "there is no curl on the PATH to download with".to_owned())?;
    let mut command = Command::new(curl);
    command
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--retry", "3"])
        .arg("--proto")
        .arg(&env.protocols)
        .arg("--proto-redir")
        .arg(&env.protocols)
        .arg("--output")
        .arg(to)
        .arg("--")
        .arg(url)
        .env("PATH", &env.search_path)
        .stdin(Stdio::null());
    super::execute(&mut command, "curl").map(drop)
}

/// The digest of a file, in lowercase hexadecimal.
///
/// # Errors
///
/// Returns the error when the file cannot be read.
pub fn digest_of(file: &Path, algorithm: Algorithm) -> io::Result<String> {
    let mut source = File::open(file)?;
    let bytes = match algorithm {
        Algorithm::Sha256 => {
            let mut hasher = Hashing(<Sha256 as sha2::Digest>::new());
            io::copy(&mut source, &mut hasher)?;
            sha2::Digest::finalize(hasher.0).to_vec()
        }
        Algorithm::Sha512 => {
            let mut hasher = Hashing(<Sha512 as sha2::Digest>::new());
            io::copy(&mut source, &mut hasher)?;
            sha2::Digest::finalize(hasher.0).to_vec()
        }
    };
    Ok(bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    }))
}

/// A hash function that bytes can be written into, so that [`io::copy`]
/// feeds it a file piece by piece instead of reading the whole file into
/// memory. Every byte written goes into the hash function, and all of them are
/// taken: the hash functions of `sha2` up to 0.10 were writers themselves in
/// just this way, and from 0.11 on they are not.
pub struct Hashing<D>(pub D);

impl<D: sha2::Digest> io::Write for Hashing<D> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
