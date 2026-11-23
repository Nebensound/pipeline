//! The calls of the GitHub REST API the release stage makes, through curl.
//!
//! The tool links no HTTP or TLS library: like the downloads of `setup`, every
//! request goes through curl, which the runners of GitHub bring. The token
//! never stands on curl's command line, which a process list shows, and never
//! in what the tool prints: curl reads the whole request, the token's header
//! among it, as its configuration from standard input. Its command line is
//! `curl --disable --config -` and nothing else, so no `.curlrc` of the
//! computer changes the request either.
//!
//! Every call is made once. A request that wrote something and whose answer
//! was lost would write twice if it were repeated, so a failed call fails the
//! stage, and the re-run takes up what it left.

use std::ffi::OsString;
use std::io::{self, Write as _};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::setup::find;

/// What the release stage needs of the API: the repository and how to reach
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHub {
    /// The address of the API, without a slash at the end, such as
    /// `https://api.github.com`.
    pub api: String,
    /// The repository as `owner/name`.
    pub repository: String,
    /// The token that may write to the repository.
    pub token: String,
    /// Where curl is looked for: the `PATH`.
    pub search_path: OsString,
    /// The protocols a request may use, in the form curl's `--proto` takes:
    /// `=https` in a run.
    pub protocols: String,
}

/// A release as the list of the repository's releases names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// Its number.
    pub id: u64,
    /// The tag it records.
    pub tag: String,
    /// Whether it is still a draft.
    pub draft: bool,
}

/// A published release, as GitHub shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// Its page.
    pub url: String,
    /// Whether GitHub holds it immutable.
    pub immutable: bool,
}

/// How many releases one page of the list holds: the most GitHub gives.
const PAGE: usize = 100;

impl GitHub {
    /// Every release of the repository, drafts included, page by page.
    ///
    /// # Errors
    ///
    /// Returns a sentence when a call fails or its answer is not a list of
    /// releases.
    pub fn releases(&self) -> Result<Vec<Listed>, String> {
        let mut all = Vec::new();
        for page in 1.. {
            let path = format!("releases?per_page={PAGE}&page={page}");
            let answer = self.call("GET", &path, None)?;
            let entries = answer
                .as_array()
                .ok_or_else(|| unexpected("GET", &path, "a list"))?;
            for entry in entries {
                all.push(Listed {
                    id: number(entry, "GET", &path)?,
                    tag: text(entry, "tag_name", "GET", &path)?,
                    draft: flag(entry, "draft", "GET", &path)?,
                });
            }
            if entries.len() < PAGE {
                break;
            }
        }
        Ok(all)
    }

    /// Creates a lightweight tag on a commit.
    ///
    /// # Errors
    ///
    /// Returns a sentence when the call fails.
    pub fn create_tag(&self, tag: &str, commit: &str) -> Result<(), String> {
        let body = json!({ "ref": format!("refs/tags/{tag}"), "sha": commit });
        self.call("POST", "git/refs", Some(&body)).map(drop)
    }

    /// The notes GitHub generates for a tag, against the tag before it.
    ///
    /// # Errors
    ///
    /// Returns a sentence when the call fails or its answer holds no notes.
    pub fn generate_notes(&self, tag: &str, previous: &str) -> Result<String, String> {
        let path = "releases/generate-notes";
        let body = json!({ "tag_name": tag, "previous_tag_name": previous });
        let answer = self.call("POST", path, Some(&body))?;
        text(&answer, "body", "POST", path)
    }

    /// Creates a release of a tag as a draft, a full release, and returns its
    /// number.
    ///
    /// # Errors
    ///
    /// Returns a sentence when the call fails or its answer holds no number.
    pub fn create_draft(
        &self,
        tag: &str,
        commit: &str,
        title: &str,
        notes: &str,
    ) -> Result<u64, String> {
        let path = "releases";
        let body = json!({
            "tag_name": tag,
            "target_commitish": commit,
            "name": title,
            "body": notes,
            "draft": true,
            "prerelease": false,
        });
        let answer = self.call("POST", path, Some(&body))?;
        number(&answer, "POST", path)
    }

    /// Publishes a draft as a full release, marked as the latest release or
    /// not: GitHub honours that mark only in the call that publishes.
    ///
    /// # Errors
    ///
    /// Returns a sentence when the call fails.
    pub fn publish(&self, id: u64, latest: bool) -> Result<(), String> {
        let body = json!({
            "draft": false,
            "prerelease": false,
            "make_latest": if latest { "true" } else { "false" },
        });
        self.call("PATCH", &format!("releases/{id}"), Some(&body))
            .map(drop)
    }

    /// A release as GitHub shows it.
    ///
    /// # Errors
    ///
    /// Returns a sentence when the call fails or its answer holds no page.
    pub fn release(&self, id: u64) -> Result<Shown, String> {
        let path = format!("releases/{id}");
        let answer = self.call("GET", &path, None)?;
        Ok(Shown {
            url: text(&answer, "html_url", "GET", &path)?,
            immutable: answer["immutable"] == Value::Bool(true),
        })
    }

    /// One call of the API on the repository: `path` below
    /// `repos/<owner>/<name>/`, with a JSON body if one is given: the answer
    /// of a call that succeeded.
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let curl = find::search("curl", &self.search_path, cfg!(windows))
            .ok_or_else(|| "there is no curl on the PATH to call GitHub with".to_owned())?;
        let config = self.config(method, path, body);
        let mut child = Command::new(curl)
            .args(["--disable", "--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| unstartable(&error))?;
        // Dropping the pipe closes it, so that curl reads to its end.
        let written = child
            .stdin
            .take()
            .map_or(Ok(()), |mut stdin| stdin.write_all(config.as_bytes()));
        let output = written
            .and(child.wait_with_output())
            .map_err(|error| unstartable(&error))?;
        let printed = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            return Err(format!(
                "curl failed on {method} {path} ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        // curl writes the status behind the answer, on a line of its own.
        let (answer, status) = printed
            .rsplit_once('\n')
            .and_then(|(answer, status)| Some((answer, status.parse::<u16>().ok()?)))
            .ok_or_else(|| unexpected(method, path, "a status"))?;
        if !(200..300).contains(&status) {
            let message = serde_json::from_str::<Value>(answer)
                .ok()
                .and_then(|value| value["message"].as_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| answer.trim().to_owned());
            return Err(format!(
                "GitHub refused {method} {path} with {status}: {message}"
            ));
        }
        serde_json::from_str(answer).map_err(|_| unexpected(method, path, "JSON"))
    }

    /// The configuration curl reads from standard input for one call. The
    /// address comes last: a configuration that broke off before its end
    /// names none, and curl sends nothing.
    fn config(&self, method: &str, path: &str, body: Option<&Value>) -> String {
        let mut lines = vec![
            "silent".to_owned(),
            "show-error".to_owned(),
            format!("proto = {}", quoted(&self.protocols)),
            format!("request = {}", quoted(method)),
            format!("user-agent = {}", quoted("nebensound-repo")),
            format!("header = {}", quoted("Accept: application/vnd.github+json")),
            format!("header = {}", quoted("X-GitHub-Api-Version: 2022-11-28")),
            format!(
                "header = {}",
                quoted(&format!("Authorization: Bearer {}", self.token))
            ),
            // No round trip to ask whether a large body may follow.
            format!("header = {}", quoted("Expect:")),
            format!("write-out = {}", quoted("\n%{http_code}")),
        ];
        if let Some(body) = body {
            lines.push(format!(
                "header = {}",
                quoted("Content-Type: application/json")
            ));
            lines.push(format!("data-raw = {}", quoted(&body.to_string())));
        }
        lines.push(format!(
            "url = {}",
            quoted(&format!("{}/repos/{}/{path}", self.api, self.repository))
        ));
        let mut config = lines.join("\n");
        config.push('\n');
        config
    }
}

/// A value of curl's configuration, in double quotes. Inside them curl reads a
/// backslash as the start of an escape, so a backslash and a double quote are
/// escaped, and so is a line break, which would end the line. No other control
/// character reaches here: the body is JSON, which escapes them, and the
/// settings refuse them ([`super::Settings::read`]).
fn quoted(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// Why curl could not be run, or not be handed its configuration.
fn unstartable(error: &io::Error) -> String {
    format!("curl cannot be run: {error}")
}

/// The sentence for an answer that lacks what the call expects.
fn unexpected(method: &str, path: &str, what: &str) -> String {
    format!("GitHub answered {method} {path} without {what}")
}

/// A release's number in an answer.
fn number(value: &Value, method: &str, path: &str) -> Result<u64, String> {
    value["id"]
        .as_u64()
        .ok_or_else(|| unexpected(method, path, "the number of a release"))
}

/// A text field of an answer.
fn text(value: &Value, field: &str, method: &str, path: &str) -> Result<String, String> {
    value[field]
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| unexpected(method, path, &format!("`{field}`")))
}

/// A yes or no field of an answer.
fn flag(value: &Value, field: &str, method: &str, path: &str) -> Result<bool, String> {
    value[field]
        .as_bool()
        .ok_or_else(|| unexpected(method, path, &format!("`{field}`")))
}
