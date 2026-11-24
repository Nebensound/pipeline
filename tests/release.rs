//! `nebensound-repo release`: the release stage on `main`.
//!
//! Nothing here reaches the network. The GitHub API is a double: a small HTTP
//! server on the loopback address, in a thread of the test, that keeps the
//! tags and releases it was given and answers the calls the stage makes. The
//! stage talks to it through the real curl, over plain HTTP, which the tests
//! allow and a run does not. The repositories are real Git repositories, built
//! commit by commit, and the artifacts of a run's builds are folders the tests
//! write. Hence Unix only: the gate runs on Linux.

#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use nebensound_repo::build::cargo_package::sha256;
use nebensound_repo::console::Console;
use nebensound_repo::history::Tag;
use nebensound_repo::release::{self, GitHub, Planned, Settings, What};
use nebensound_repo::repository::{Records, Repository};
use nebensound_repo::setup::find;
use nebensound_repo::{TagVersion, UnitName, UnitVersion};
use serde_json::{Value, json};
use support::{Git, QUIET, central, repository};
use tempfile::TempDir;

/// The token the tests hand the stage.
const TOKEN: &str = "token-that-must-stay-secret";

/// The repository the double serves.
const REPOSITORY: &str = "acme/pipeline";

/// The repository and a component `notes`.
const UNITS: &str = r#"[repo]
audience = "public"
pipeline = "none"

[component.notes]
path     = "notes"
pipeline = "none"
"#;

/// One call the double received.
#[derive(Debug, Clone)]
struct Request {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Value,
    /// The body as it came, for one that is no JSON.
    raw: Vec<u8>,
    content_type: Option<String>,
}

/// What the double holds and has seen.
#[derive(Debug, Default)]
struct State {
    /// Where it answers, for the address of the uploads.
    address: String,
    releases: Vec<Value>,
    /// Every asset, with the number of its release under `release`.
    assets: Vec<Value>,
    tags: BTreeMap<String, String>,
    requests: Vec<Request>,
    /// Whether a release GitHub publishes is immutable.
    immutable: bool,
    /// Answers given instead of the double's own, by method and path:
    /// status and body.
    canned: BTreeMap<(String, String), (u16, String)>,
    next: u64,
}

/// The GitHub API, as far as the stage calls it.
struct Double {
    address: String,
    state: Arc<Mutex<State>>,
}

impl Double {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port on the loopback address");
        let address = format!("http://{}", listener.local_addr().expect("its address"));
        let state = Arc::new(Mutex::new(State {
            address: address.clone(),
            immutable: true,
            next: 1,
            ..State::default()
        }));
        let shared = Arc::clone(&state);
        thread::spawn(move || {
            for stream in listener.incoming() {
                serve(stream.expect("a connection"), &shared);
            }
        });
        Self { address, state }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A release the repository holds already.
    fn holds(&self, tag: &str, draft: bool) -> u64 {
        let mut state = self.state();
        let id = state.next + 100;
        state.next += 1;
        let upload = upload_url(&state.address, id);
        state.releases.push(json!({
            "id": id,
            "tag_name": tag,
            "draft": draft,
            "prerelease": draft,
            "name": tag,
            "body": "left by an earlier attempt",
            "html_url": format!("https://github.example/{REPOSITORY}/releases/{id}"),
            "upload_url": upload,
            "immutable": !draft,
        }));
        id
    }

    /// An asset a release holds already.
    fn holds_asset(&self, release: u64, name: &str, bytes: &[u8], condition: &str) -> u64 {
        let mut state = self.state();
        let id = state.next + 1000;
        state.next += 1;
        state.assets.push(json!({
            "id": id,
            "release": release,
            "name": name,
            "state": condition,
            "digest": format!("sha256:{}", sha256(bytes)),
            "size": bytes.len(),
        }));
        id
    }

    /// The assets of a release: name and digest.
    fn assets(&self, release: u64) -> Vec<(String, String)> {
        self.state()
            .assets
            .iter()
            .filter(|asset| asset["release"] == release)
            .map(|asset| {
                (
                    asset["name"].as_str().unwrap().to_owned(),
                    asset["digest"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }

    /// Answers a call with this status and body instead of its own answer.
    fn answer(&self, method: &str, path: &str, status: u16, body: &str) {
        self.state().canned.insert(
            (method.to_owned(), path.to_owned()),
            (status, body.to_owned()),
        );
    }

    fn requests(&self) -> Vec<Request> {
        self.state().requests.clone()
    }

    /// The calls, as `METHOD path`.
    fn calls(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|request| format!("{} {}", request.method, request.path))
            .collect()
    }

    fn release(&self, tag: &str) -> Value {
        self.state()
            .releases
            .iter()
            .find(|release| release["tag_name"] == tag)
            .cloned()
            .unwrap_or_else(|| panic!("no release of {tag}"))
    }

    fn settings(&self, summary: Option<PathBuf>) -> Settings {
        Settings {
            github: GitHub {
                api: self.address.clone(),
                repository: REPOSITORY.to_owned(),
                token: TOKEN.to_owned(),
                search_path: std::env::var_os("PATH").unwrap_or_default(),
                protocols: "=http".to_owned(),
            },
            summary,
            assets: None,
        }
    }
}

/// The path of an upload of `name` to the release `id`, as the double sees
/// it.
fn upload(id: u64, name: &str) -> String {
    format!("/uploads/repos/{REPOSITORY}/releases/{id}/assets?name={name}")
}

/// Where the assets of the release `id` are uploaded to, with the template of
/// the query GitHub writes behind it.
fn upload_url(address: &str, id: u64) -> String {
    format!("{address}/uploads/repos/{REPOSITORY}/releases/{id}/assets{{?name,label}}")
}

/// Answers one connection.
fn serve(mut stream: TcpStream, state: &Mutex<State>) {
    let mut reader = BufReader::new(stream.try_clone().expect("the stream twice"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("a request line");
    let mut words = line.split_whitespace();
    let method = words.next().unwrap_or_default().to_owned();
    let target = words.next().unwrap_or_default().to_owned();
    let mut length = 0;
    let mut authorization = None;
    let mut content_type = None;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).expect("a header");
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(": ").unwrap_or((header, ""));
        match name.to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().expect("a length"),
            "authorization" => authorization = Some(value.to_owned()),
            "content-type" => content_type = Some(value.to_owned()),
            _ => {}
        }
    }
    let mut raw = vec![0; length];
    reader.read_exact(&mut raw).expect("the body");
    let body: Value = if content_type.as_deref() == Some("application/json") {
        serde_json::from_slice(&raw).expect("a body in JSON")
    } else {
        Value::Null
    };
    let prefix = format!("/repos/{REPOSITORY}/");
    let path = target.strip_prefix(&prefix).unwrap_or(&target).to_owned();
    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
    state.requests.push(Request {
        method: method.clone(),
        path: path.clone(),
        authorization,
        body: body.clone(),
        raw: raw.clone(),
        content_type,
    });
    let (status, answer) = match state.canned.get(&(method.clone(), path.clone())) {
        Some((status, answer)) => (*status, answer.clone()),
        None => route(&mut state, &method, &path, &body, &raw),
    };
    drop(state);
    let response = format!(
        "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{answer}",
        answer.len()
    );
    stream.write_all(response.as_bytes()).expect("the answer");
}

/// The number of the page a query asks for.
fn page(query: &str) -> usize {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("page="))
        .expect("a page")
        .parse()
        .expect("a number")
}

/// The double's own answer to a call about assets, if it is one.
fn route_assets(
    state: &mut State,
    method: &str,
    path: &str,
    query: &str,
    raw: &[u8],
) -> Option<(u16, String)> {
    let uploads = format!("/uploads/repos/{REPOSITORY}/releases/");
    if let Some(release) = path.strip_prefix(&uploads) {
        let release: u64 = release
            .strip_suffix("/assets")
            .expect("the assets of a release")
            .parse()
            .expect("a number");
        let name = query.strip_prefix("name=").expect("a name").to_owned();
        let id = state.next + 1000;
        state.next += 1;
        let asset = json!({
            "id": id,
            "release": release,
            "name": name,
            "state": "uploaded",
            "digest": format!("sha256:{}", sha256(raw)),
            "size": raw.len(),
        });
        state.assets.push(asset.clone());
        return Some((201, asset.to_string()));
    }
    if method == "GET"
        && let Some(release) = path
            .strip_prefix("releases/")
            .and_then(|rest| rest.strip_suffix("/assets"))
    {
        let release: u64 = release.parse().expect("a number");
        let listed: Vec<Value> = state
            .assets
            .iter()
            .filter(|asset| asset["release"] == release)
            .skip((page(query) - 1) * 100)
            .take(100)
            .cloned()
            .collect();
        return Some((200, Value::Array(listed).to_string()));
    }
    if method == "DELETE"
        && let Some(asset) = path.strip_prefix("releases/assets/")
    {
        let asset: u64 = asset.parse().expect("a number");
        state.assets.retain(|each| each["id"] != asset);
        return Some((204, String::new()));
    }
    None
}

/// The double's own answer to a call.
fn route(state: &mut State, method: &str, path: &str, body: &Value, raw: &[u8]) -> (u16, String) {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    if let Some(answer) = route_assets(state, method, path, query, raw) {
        return answer;
    }
    match (method, path) {
        ("GET", "releases") => {
            let listed: Vec<Value> = state
                .releases
                .iter()
                .skip((page(query) - 1) * 100)
                .take(100)
                .cloned()
                .collect();
            (200, Value::Array(listed).to_string())
        }
        ("POST", "git/refs") => {
            let name = body["ref"].as_str().expect("a ref").to_owned();
            if state.tags.contains_key(&name) {
                return (
                    422,
                    json!({ "message": "Reference already exists" }).to_string(),
                );
            }
            let sha = body["sha"].as_str().expect("a commit").to_owned();
            state.tags.insert(name.clone(), sha.clone());
            (
                201,
                json!({ "ref": name, "object": { "sha": sha } }).to_string(),
            )
        }
        ("POST", "releases/generate-notes") => {
            let notes = format!(
                "## What's Changed\n* \"everything\" since {}\\",
                body["previous_tag_name"].as_str().expect("a previous tag")
            );
            (200, json!({ "name": "notes", "body": notes }).to_string())
        }
        ("POST", "releases") => {
            let id = state.next;
            state.next += 1;
            let mut release = body.clone();
            release["id"] = json!(id);
            release["html_url"] =
                json!(format!("https://github.example/{REPOSITORY}/releases/{id}"));
            release["upload_url"] = json!(upload_url(&state.address, id));
            release["immutable"] = json!(false);
            state.releases.push(release.clone());
            (201, release.to_string())
        }
        ("PATCH", release) => {
            let id: u64 = release
                .strip_prefix("releases/")
                .expect("a release")
                .parse()
                .expect("a number");
            let immutable = state.immutable;
            let found = state
                .releases
                .iter_mut()
                .find(|release| release["id"] == id)
                .expect("a release the double holds");
            for (key, value) in body.as_object().expect("an object") {
                found[key] = value.clone();
            }
            found["immutable"] = json!(immutable);
            (200, found.to_string())
        }
        ("GET", release) => {
            let id: u64 = release
                .strip_prefix("releases/")
                .expect("a release")
                .parse()
                .expect("a number");
            let found = state
                .releases
                .iter()
                .find(|release| release["id"] == id)
                .expect("a release the double holds");
            (200, found.to_string())
        }
        _ => (404, json!({ "message": "Not Found" }).to_string()),
    }
}

/// The repository alone, one commit on `main`.
fn single() -> (Git, String) {
    let git = Git::new();
    git.write(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    let head = git.commit("fix: start");
    (git, head)
}

/// The repository, released once with `v0.0.1`, and a second commit.
fn second() -> (Git, String, String) {
    let (git, first) = single();
    git.tag("v0.0.1", &first);
    git.write(&[("README.md", "# x\n\nMore.\n")]);
    let head = git.commit("fix: more");
    (git, first, head)
}

/// The stage on the fixture as it stands, with these settings, written for a
/// workflow run.
fn stage(git: &Git, settings: &Settings) -> (String, u8) {
    stage_in(git, settings, true)
}

fn stage_in(git: &Git, settings: &Settings, workflow: bool) -> (String, u8) {
    let repository = Repository::at(git.path());
    let central = central();
    let analysis = repository.analyse(&central).expect("the fixture reads");
    assert!(!analysis.failed(), "{:?}", analysis.findings());
    let records = repository.records().expect("the records read");
    let declared = release::assets::declared(&analysis, &central);
    let mut out = Vec::new();
    let mut console = Console::new(&mut out, workflow);
    let code = release::run(
        &analysis.versions,
        &declared,
        &records,
        settings,
        &mut console,
    );
    let code = console.end(code);
    (
        support::printed(&String::from_utf8(out).expect("the log is UTF-8")),
        code,
    )
}

/// A folder for the summary of the run, and the file in it.
fn summary_file() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("a directory");
    let file = dir.path().join("summary.md");
    (dir, file)
}

#[test]
fn a_first_release_is_tagged_drafted_and_published_as_the_latest() {
    let (git, head) = single();
    let double = Double::new();
    let (_dir, summary) = summary_file();

    let (text, code) = stage(&git, &double.settings(Some(summary.clone())));

    assert_eq!(code, 0, "{text}");
    assert_eq!(
        double.calls(),
        [
            "GET releases?per_page=100&page=1",
            "POST git/refs",
            "POST releases",
            "PATCH releases/1",
            "GET releases/1",
        ]
    );
    assert_eq!(
        double.state().tags,
        BTreeMap::from([("refs/tags/v0.0.1".to_owned(), head.clone())])
    );
    let requests = double.requests();
    assert_eq!(
        requests[2].body,
        json!({
            "tag_name": "v0.0.1",
            "target_commitish": head,
            "name": "pipeline 0.0.1",
            "body": "The first release of pipeline, so there is no earlier release to list the \
                     changes against.",
            "draft": true,
            "prerelease": false,
        })
    );
    assert_eq!(
        requests[3].body,
        json!({ "draft": false, "prerelease": false, "make_latest": "true" })
    );
    let release = double.release("v0.0.1");
    assert_eq!(release["draft"], false);
    assert_eq!(release["prerelease"], false);
    assert_eq!(release["make_latest"], "true");

    let url = format!("https://github.example/{REPOSITORY}/releases/1");
    assert_eq!(
        text,
        format!(
            "the repository: v0.0.1 on {head}, a full release, the latest, its first release.\n\
             the repository: tagged {head} as v0.0.1.\n\
             the repository: drafted the release v0.0.1.\n\
             the repository: published v0.0.1, full release, latest: {url}\n"
        )
    );
    assert_eq!(
        fs::read_to_string(&summary).expect("the summary"),
        format!(
            "### The release stage on main\n\n\
             - the repository: v0.0.1 on {head}, a full release, the latest, its first release.\n\
             \n### Published\n\n\
             | Unit | Release | Kind |\n\
             | ---- | ------- | ---- |\n\
             | the repository | [v0.0.1]({url}) | full release, latest |\n"
        )
    );
}

#[test]
fn the_summary_is_appended_to_what_the_run_wrote_before() {
    let (git, _) = single();
    let double = Double::new();
    let (_dir, summary) = summary_file();
    fs::write(&summary, "Before.\n").expect("an earlier step's summary");

    let (text, code) = stage(&git, &double.settings(Some(summary.clone())));

    assert_eq!(code, 0, "{text}");
    let written = fs::read_to_string(&summary).expect("the summary");
    assert!(
        written.starts_with("Before.\n### The release stage on main\n"),
        "{written}"
    );
}

#[test]
fn every_call_carries_the_token_and_no_command_line_shows_it() {
    let (git, _) = single();
    let double = Double::new();
    // A curl that writes down how it was called and then is the real one.
    let bin = tempfile::tempdir().expect("a directory");
    let calls = bin.path().join("calls");
    let curl = find::search("curl", &std::env::var_os("PATH").unwrap_or_default(), false)
        .expect("curl is installed");
    executable(
        &bin.path().join("curl"),
        &format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nexec '{}' \"$@\"\n",
            calls.display(),
            curl.display()
        ),
    );
    let mut settings = double.settings(None);
    bin.path()
        .as_os_str()
        .clone_into(&mut settings.github.search_path);

    let (text, code) = stage(&git, &settings);

    assert_eq!(code, 0, "{text}");
    let requests = double.requests();
    assert_eq!(requests.len(), 5);
    for request in &requests {
        assert_eq!(
            request.authorization.as_deref(),
            Some(format!("Bearer {TOKEN}").as_str()),
            "{request:?}"
        );
    }
    let command_lines = fs::read_to_string(&calls).expect("the calls of curl");
    assert_eq!(command_lines, "--disable --config -\n".repeat(5));
    assert!(!text.contains(TOKEN), "{text}");
}

#[test]
fn the_notes_are_generated_against_the_previous_record() {
    let (git, first, head) = second();
    // Beside the previous record: a pre-release of it, tags that read as no
    // record of the repository, and a higher one on a commit `HEAD` does not
    // contain.
    git.tag("v0.0.1-pre.1", &first);
    git.tag("v0.0.1-rc.1", &first);
    git.tag("vintage", &first);
    git.tag("notes-v3.0.0", &first);
    git.run(&["checkout", "--quiet", "-b", "side", &first]);
    git.write(&[("side.md", "# Side\n")]);
    let side = git.commit("fix: side");
    git.tag("v9.9.9", &side);
    git.run(&["checkout", "--quiet", "main"]);
    let double = Double::new();

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 0, "{text}");
    assert!(
        text.starts_with(&format!(
            "the repository: v0.0.2 on {head}, a full release, the latest, with notes since \
             v0.0.1.\n"
        )),
        "{text}"
    );
    let requests = double.requests();
    let notes = requests
        .iter()
        .find(|request| request.path == "releases/generate-notes")
        .expect("notes were generated");
    assert_eq!(
        notes.body,
        json!({ "tag_name": "v0.0.2", "previous_tag_name": "v0.0.1" })
    );
    assert_eq!(
        double.release("v0.0.2")["body"],
        "## What's Changed\n* \"everything\" since v0.0.1\\"
    );
    assert_eq!(double.release("v0.0.2")["name"], "pipeline 0.0.2");
}

#[test]
fn a_component_gets_a_full_release_that_is_not_the_latest() {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", UNITS),
        ("README.md", "# x\n"),
        ("notes/a.md", "# Notes\n"),
    ]);
    let head = git.commit("fix: start");
    let double = Double::new();
    let (_dir, summary) = summary_file();

    let (text, code) = stage(&git, &double.settings(Some(summary.clone())));

    assert_eq!(code, 0, "{text}");
    let notes = double.release("notes-v0.0.1");
    assert_eq!(notes["name"], "notes 0.0.1");
    assert_eq!(
        notes["body"],
        "The first release of notes, so there is no earlier release to list the changes against."
    );
    assert_eq!(notes["make_latest"], "false");
    assert_eq!(notes["prerelease"], false);
    assert_eq!(double.release("v0.0.1")["make_latest"], "true");
    assert!(
        text.contains(&format!(
            "component notes: notes-v0.0.1 on {head}, a full release, not the latest, its first \
             release.\n"
        )),
        "{text}"
    );
    let written = fs::read_to_string(&summary).expect("the summary");
    assert!(
        written.contains("| component notes | [notes-v0.0.1](https://github.example/"),
        "{written}"
    );
    assert!(written.contains(") | full release |\n"), "{written}");
}

#[test]
fn a_unit_the_commit_does_not_touch_gets_nothing() {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", UNITS),
        ("README.md", "# x\n"),
        ("notes/a.md", "# Notes\n"),
    ]);
    let first = git.commit("fix: start");
    git.tag("v0.0.1", &first);
    git.tag("notes-v0.0.1", &first);
    git.write(&[("README.md", "# x\n\nMore.\n")]);
    git.commit("fix: more");
    let double = Double::new();

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("component notes"), "{text}");
    let tags: Vec<String> = double.state().tags.keys().cloned().collect();
    assert_eq!(tags, ["refs/tags/v0.0.2"]);
}

#[test]
fn a_tag_of_the_version_on_another_commit_stops_everything_before_anything_is_written() {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", UNITS),
        ("README.md", "# x\n"),
        ("notes/a.md", "# Notes\n"),
    ]);
    let first = git.commit("fix: start");
    git.tag("v0.0.1", &first);
    git.tag("notes-v0.0.1", &first);
    git.write(&[("README.md", "# x\n\nMore.\n"), ("notes/a.md", "# More\n")]);
    let head = git.commit("fix: more");
    // A commit `HEAD` does not contain carries the version the history gives
    // the repository at `HEAD`; phase 0 passes it over, the stage may not.
    git.run(&["checkout", "--quiet", "-b", "side", &first]);
    git.write(&[("side.md", "# Side\n")]);
    let side = git.commit("fix: side");
    git.tag("v0.0.2", &side);
    git.run(&["checkout", "--quiet", "main"]);
    let double = Double::new();
    let (_dir, summary) = summary_file();

    let (text, code) = stage(&git, &double.settings(Some(summary.clone())));

    assert_eq!(code, 1, "{text}");
    assert!(double.calls().is_empty(), "{:?}", double.calls());
    // The error follows what the stage says, as every annotation of a call
    // follows the text before it.
    assert!(
        text.ends_with(&format!(
            "::error::The tag v0.0.2 stands on the commit {side}, and the history gives 0.0.2 of \
             the repository to {head}, the commit pushed. A version belongs to exactly one \
             commit, so nothing is released in this run. Delete the tag if nobody published it, \
             or raise minimum-version past it, as for the finding conflicting-tag.\n"
        )),
        "{text}"
    );
    // The component has nothing in the way, and is still not released.
    assert!(text.contains("component notes: notes-v0.0.2 on "), "{text}");
    let written = fs::read_to_string(&summary).expect("the summary");
    assert!(!written.contains("Published"), "{written}");
}

#[test]
fn a_tag_of_the_version_on_no_commit_stops_the_stage_as_well() {
    let (git, head) = single();
    let tree = git.run(&["rev-parse", "HEAD^{tree}"]).trim().to_owned();
    git.tag("v0.0.1", &tree);
    let double = Double::new();

    let (text, code) = stage_in(&git, &double.settings(None), false);

    assert_eq!(code, 1, "{text}");
    assert!(double.calls().is_empty(), "{:?}", double.calls());
    assert!(
        text.starts_with(&format!(
            "error: The tag v0.0.1 stands on no commit, and the history gives 0.0.1 of the \
             repository to {head}, the commit pushed."
        )),
        "{text}"
    );
}

#[test]
fn a_newer_record_that_holds_the_commit_leaves_the_unit_out() {
    let (git, first, second) = second();
    git.tag("v0.0.2", &second);
    git.run(&["checkout", "--quiet", "--detach", &first]);
    let double = Double::new();
    let (_dir, summary) = summary_file();

    let (text, code) = stage(&git, &double.settings(Some(summary.clone())));

    assert_eq!(code, 0, "{text}");
    assert!(double.calls().is_empty(), "{:?}", double.calls());
    let line = format!(
        "the repository: the newer release v0.0.2 already holds {first}, so v0.0.1 is not \
         released."
    );
    assert_eq!(text, format!("{line}\nNothing is released in this run.\n"));
    assert_eq!(
        fs::read_to_string(&summary).expect("the summary"),
        format!("### The release stage on main\n\n- {line}\n- Nothing is released in this run.\n")
    );
}

#[test]
fn a_re_run_keeps_the_tag_on_the_commit() {
    let (git, head) = single();
    git.tag("v0.0.1", &head);
    let double = Double::new();

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 0, "{text}");
    assert!(
        !double.calls().contains(&"POST git/refs".to_owned()),
        "{:?}",
        double.calls()
    );
    assert!(
        text.contains(&format!(
            "the repository: the tag v0.0.1 stands on {head} already.\n"
        )),
        "{text}"
    );
    assert_eq!(double.release("v0.0.1")["draft"], false);
}

#[test]
fn a_re_run_takes_up_the_draft_an_earlier_attempt_left() {
    let (git, _, head) = second();
    git.tag("v0.0.2", &head);
    let double = Double::new();
    // Left as a pre-release by hand: it is published as a full release all
    // the same.
    let id = double.holds("v0.0.2", true);

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 0, "{text}");
    assert_eq!(
        double.calls(),
        [
            "GET releases?per_page=100&page=1".to_owned(),
            format!("PATCH releases/{id}"),
            format!("GET releases/{id}"),
        ]
    );
    assert!(
        text.contains(
            "the repository: the draft of v0.0.2 that an earlier attempt left is taken up.\n"
        ),
        "{text}"
    );
    let release = double.release("v0.0.2");
    assert_eq!(release["draft"], false);
    assert_eq!(release["prerelease"], false);
    assert_eq!(release["make_latest"], "true");
    assert_eq!(release["body"], "left by an earlier attempt");
}

#[test]
fn a_re_run_leaves_a_published_release_as_it_is() {
    let (git, head) = single();
    git.tag("v0.0.1", &head);
    let double = Double::new();
    double.holds("v0.0.1", false);
    let (_dir, summary) = summary_file();

    let (text, code) = stage(&git, &double.settings(Some(summary.clone())));

    assert_eq!(code, 0, "{text}");
    assert_eq!(double.calls(), ["GET releases?per_page=100&page=1"]);
    assert!(
        text.ends_with(
            "the repository: the release v0.0.1 is published already; nothing is changed.\n\
             No release is left to publish.\n"
        ),
        "{text}"
    );
    let written = fs::read_to_string(&summary).expect("the summary");
    assert!(!written.contains("Published"), "{written}");
}

#[test]
fn a_re_run_of_a_run_that_went_through_changes_nothing() {
    let (git, head) = single();
    let double = Double::new();
    let (text, code) = stage(&git, &double.settings(None));
    assert_eq!(code, 0, "{text}");
    let before = double.state().releases.clone();
    // The checkout of the re-run holds the tag the first attempt wrote.
    git.tag("v0.0.1", &head);
    let written = double.requests().len();

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 0, "{text}");
    let calls: Vec<String> = double.calls().split_off(written);
    assert_eq!(calls, ["GET releases?per_page=100&page=1"]);
    assert_eq!(double.state().releases, before);
}

#[test]
fn a_draft_on_a_later_page_of_the_list_is_found() {
    let (git, head) = single();
    git.tag("v0.0.1", &head);
    let double = Double::new();
    for n in 0..120 {
        double.holds(&format!("other-v0.0.{n}"), false);
    }
    let id = double.holds("v0.0.1", true);

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 0, "{text}");
    assert_eq!(
        double.calls(),
        [
            "GET releases?per_page=100&page=1".to_owned(),
            "GET releases?per_page=100&page=2".to_owned(),
            format!("PATCH releases/{id}"),
            format!("GET releases/{id}"),
        ]
    );
}

#[test]
fn a_release_that_is_not_immutable_is_published_with_a_warning() {
    let (git, _) = single();
    let double = Double::new();
    double.state().immutable = false;
    let (_dir, summary) = summary_file();

    let (text, code) = stage(&git, &double.settings(Some(summary.clone())));

    assert_eq!(code, 0, "{text}");
    let warning = format!(
        "The release v0.0.1 is published and is not immutable: immutable releases are switched \
         off in {REPOSITORY}, so its tag can still be moved or deleted and its assets changed. \
         Switch them on in the settings of the repository."
    );
    assert!(text.ends_with(&format!("::warning::{warning}\n")), "{text}");
    assert!(
        fs::read_to_string(&summary)
            .expect("the summary")
            .ends_with(&format!(
                " | full release, latest |\n\n**Warning:** {warning}\n"
            )),
        "{text}"
    );
}

#[test]
fn an_immutable_release_gets_no_warning() {
    let (git, _) = single();
    let double = Double::new();

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("warning"), "{text}");
}

#[test]
fn a_refused_call_fails_the_stage_and_writes_nothing_after_it() {
    let (git, _) = single();
    let double = Double::new();
    double.answer(
        "POST",
        "git/refs",
        422,
        &json!({ "message": "Reference already exists" }).to_string(),
    );

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 1, "{text}");
    assert!(
        text.ends_with(
            "::error::GitHub refused POST git/refs with 422: Reference already exists\n"
        ),
        "{text}"
    );
    assert_eq!(
        double.calls(),
        ["GET releases?per_page=100&page=1", "POST git/refs"]
    );
}

#[test]
fn every_refused_call_after_the_first_write_fails_the_stage() {
    for (method, path, earlier) in [
        ("POST", "releases/generate-notes", true),
        ("POST", "releases", false),
        ("PATCH", "releases/1", false),
        ("GET", "releases/1", false),
    ] {
        let git = if earlier { second().0 } else { single().0 };
        let double = Double::new();
        double.answer(
            method,
            path,
            500,
            &json!({ "message": "Broken" }).to_string(),
        );

        let (text, code) = stage(&git, &double.settings(None));

        assert_eq!(code, 1, "{text}");
        assert!(
            text.ends_with(&format!(
                "::error::GitHub refused {method} {path} with 500: Broken\n"
            )),
            "{text}"
        );
        let calls = double.calls();
        assert_eq!(calls.last(), Some(&format!("{method} {path}")), "{calls:?}");
    }
}

#[test]
fn a_refusal_without_a_message_is_shown_as_it_came() {
    let (git, _) = single();
    let double = Double::new();
    double.answer("GET", "releases?per_page=100&page=1", 502, "Bad gateway\n");

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 1, "{text}");
    assert!(
        text.ends_with(
            "::error::GitHub refused GET releases?per_page=100&page=1 with 502: Bad gateway\n"
        ),
        "{text}"
    );
}

#[test]
fn an_answer_without_what_the_call_needs_fails_the_stage() {
    let list = "GET releases?per_page=100&page=1";
    for (method, path, answer, expected) in [
        (
            "GET",
            "releases?per_page=100&page=1",
            "{}",
            format!("{list} without a list"),
        ),
        (
            "GET",
            "releases?per_page=100&page=1",
            "[{}]",
            format!("{list} without the number of a release"),
        ),
        (
            "GET",
            "releases?per_page=100&page=1",
            r#"[{"id": 1}]"#,
            format!("{list} without `tag_name`"),
        ),
        (
            "GET",
            "releases?per_page=100&page=1",
            r#"[{"id": 1, "tag_name": "v9.0.0"}]"#,
            format!("{list} without `draft`"),
        ),
        (
            "GET",
            "releases?per_page=100&page=1",
            r#"[{"id": 1, "tag_name": "v9.0.0", "draft": false}]"#,
            format!("{list} without `upload_url`"),
        ),
        (
            "GET",
            "releases?per_page=100&page=1",
            "no JSON",
            format!("{list} without JSON"),
        ),
        (
            "POST",
            "releases",
            "{}",
            "POST releases without the number of a release".to_owned(),
        ),
        (
            "POST",
            "releases",
            r#"{"id": 1}"#,
            "POST releases without `upload_url`".to_owned(),
        ),
        (
            "GET",
            "releases/1",
            "{}",
            "GET releases/1 without `html_url`".to_owned(),
        ),
    ] {
        let (git, _) = single();
        let double = Double::new();
        double.answer(method, path, 200, answer);

        let (text, code) = stage(&git, &double.settings(None));

        assert_eq!(code, 1, "{text}");
        assert!(
            text.ends_with(&format!("::error::GitHub answered {expected}\n")),
            "{text}"
        );
    }
}

#[test]
fn notes_that_github_does_not_give_fail_the_stage() {
    let (git, _, _) = second();
    let double = Double::new();
    double.answer("POST", "releases/generate-notes", 200, "{}");

    let (text, code) = stage(&git, &double.settings(None));

    assert_eq!(code, 1, "{text}");
    assert!(
        text.ends_with("::error::GitHub answered POST releases/generate-notes without `body`\n"),
        "{text}"
    );
    assert!(
        !double.calls().contains(&"POST releases".to_owned()),
        "{:?}",
        double.calls()
    );
}

#[test]
fn a_call_over_plain_http_is_refused_in_a_run() {
    let (git, _) = single();
    let double = Double::new();
    let mut settings = double.settings(None);
    settings.github.protocols = "=https".to_owned();

    let (text, code) = stage(&git, &settings);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("::error::curl failed on GET releases?per_page=100&page=1 ("),
        "{text}"
    );
    assert!(double.calls().is_empty(), "{:?}", double.calls());
}

#[test]
fn an_api_nobody_answers_fails_the_stage() {
    let (git, _) = single();
    let closed = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = format!("http://{}", closed.local_addr().expect("its address"));
    drop(closed);
    let double = Double::new();
    let mut settings = double.settings(None);
    settings.github.api = address;

    let (text, code) = stage(&git, &settings);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("::error::curl failed on GET "), "{text}");
}

#[test]
fn without_curl_nothing_is_called() {
    let (git, _) = single();
    let double = Double::new();
    let empty = tempfile::tempdir().expect("a directory");
    let mut settings = double.settings(None);
    empty
        .path()
        .as_os_str()
        .clone_into(&mut settings.github.search_path);

    let (text, code) = stage(&git, &settings);

    assert_eq!(code, 1, "{text}");
    assert!(
        text.ends_with("::error::there is no curl on the PATH to call GitHub with\n"),
        "{text}"
    );
}

/// The stage with a curl that is this script.
fn with_curl(script: &str, token: &str) -> (String, u8) {
    let (git, _) = single();
    let double = Double::new();
    let bin = tempfile::tempdir().expect("a directory");
    executable(&bin.path().join("curl"), script);
    let mut settings = double.settings(None);
    bin.path()
        .as_os_str()
        .clone_into(&mut settings.github.search_path);
    token.clone_into(&mut settings.github.token);
    stage(&git, &settings)
}

#[test]
fn a_curl_that_cannot_be_started_fails_the_stage() {
    let (text, code) = with_curl("#!/nonexistent/interpreter\n", TOKEN);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("::error::curl cannot be run: "), "{text}");
}

#[test]
fn a_curl_that_does_not_read_its_configuration_fails_the_stage() {
    // More than a pipe holds, so that writing waits for curl, which closes
    // the pipe instead of reading it.
    let token = "x".repeat(1 << 20);
    let (text, code) = with_curl("#!/bin/sh\nexec 0<&-\nsleep 1\n", &token);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("::error::curl cannot be run: "), "{text}");
    assert!(!text.contains(&token), "the token stays out of the log");
}

#[test]
fn an_answer_without_a_status_fails_the_stage() {
    // The second answer has a line for the status, and no number on it.
    for printed in ["no status", "[]\\nno number"] {
        let (text, code) = with_curl(
            &format!("#!/bin/sh\ncat > /dev/null\nprintf '{printed}'\n"),
            TOKEN,
        );

        assert_eq!(code, 1, "{text}");
        assert!(
            text.ends_with(
                "::error::GitHub answered GET releases?per_page=100&page=1 without a status\n"
            ),
            "{text}"
        );
    }
}

#[test]
fn a_summary_that_cannot_be_written_fails_the_stage() {
    let (git, head) = single();
    git.tag("v0.0.1", &head);
    let double = Double::new();
    double.holds("v0.0.1", false);
    let folder = tempfile::tempdir().expect("a directory");

    let (text, code) = stage(&git, &double.settings(Some(folder.path().to_owned())));

    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains(&format!(
            "::error::the summary of the run cannot be written to {}: ",
            folder.path().display()
        )),
        "{text}"
    );
}

#[test]
fn outside_a_run_the_log_has_no_workflow_commands() {
    let (git, _) = single();
    let double = Double::new();
    double.state().immutable = false;

    let (text, code) = stage_in(&git, &double.settings(None), false);

    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("::"), "{text}");
    assert!(
        text.contains("warning: The release v0.0.1 is published and is not immutable"),
        "{text}"
    );
}

// The archives of `release-assets`.

/// The repository builds its one binary with cargo-release and publishes the
/// archives to its release.
const BINARY: &str = r#"[repo]
audience = "public"
pipeline = { version = ["cargo"], build = "cargo-release", publish = ["release-assets"] }
"#;

/// The targets of cargo-release, in the order of the register.
const TARGETS: [&str; 3] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-musl",
];

/// The repository of [`BINARY`], one commit on `main`.
fn binary() -> (Git, String) {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", BINARY),
        ("README.md", "# x\n"),
        ("LICENSE", "The licence.\n"),
        ("Cargo.toml", "[package]\nname = \"app\"\n"),
    ]);
    let head = git.commit("fix: start");
    (git, head)
}

/// The name of the archive for a target.
fn archive_name(target: &str) -> String {
    format!("app-{target}.tar.gz")
}

/// The bytes of the archive for a target.
fn bytes(target: &str) -> Vec<u8> {
    format!("the archive for {target}\n").into_bytes()
}

/// The digest of the archive for a target, as GitHub gives it.
fn digest(target: &str) -> String {
    format!("sha256:{}", sha256(&bytes(target)))
}

/// The name of the repository's artifact, as cargo-release names it.
fn artifact_name(version: &str, commit: &str, target: &str) -> String {
    format!("executable-.@{version}@{commit}@{target}")
}

/// The artifacts of a run's builds, as Publish fetches them: each in a folder
/// named after it.
struct Handed {
    dir: TempDir,
}

impl Handed {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a directory");
        fs::create_dir(dir.path().join("artifacts")).expect("the folder");
        Self { dir }
    }

    /// The three archives of the repository's build of `version` from
    /// `commit`.
    fn of(version: &str, commit: &str) -> Self {
        let handed = Self::new();
        handed.all(version, commit);
        handed
    }

    /// The three archives added.
    fn all(&self, version: &str, commit: &str) -> &Self {
        for target in TARGETS {
            self.archive(version, commit, target);
        }
        self
    }

    fn path(&self) -> PathBuf {
        self.dir.path().join("artifacts")
    }

    /// The artifact for a target, with the archive named for it.
    fn archive(&self, version: &str, commit: &str, target: &str) -> &Self {
        self.artifact(
            &artifact_name(version, commit, target),
            &[(&archive_name(target), &bytes(target))],
        )
    }

    /// An artifact with these files.
    fn artifact(&self, name: &str, files: &[(&str, &[u8])]) -> &Self {
        let folder = self.path().join(name);
        fs::create_dir_all(&folder).expect("the artifact");
        for (file, bytes) in files {
            fs::write(folder.join(file), bytes).expect("a file");
        }
        self
    }

    fn settings(&self, double: &Double, summary: Option<PathBuf>) -> Settings {
        let mut settings = double.settings(summary);
        settings.assets = Some(self.path());
        settings
    }
}

/// The calls of a release of the repository's three archives, as the first
/// release `1`.
fn attaching() -> Vec<String> {
    let mut calls = vec![
        "GET releases?per_page=100&page=1".to_owned(),
        "POST git/refs".to_owned(),
        "POST releases".to_owned(),
        "GET releases/1/assets?per_page=100&page=1".to_owned(),
    ];
    calls.extend(
        TARGETS
            .iter()
            .map(|target| format!("POST {}", upload(1, &archive_name(target)))),
    );
    calls.extend(["PATCH releases/1".to_owned(), "GET releases/1".to_owned()]);
    calls
}

#[test]
fn the_archives_are_attached_to_the_draft_before_the_release_is_published() {
    let (git, head) = binary();
    let double = Double::new();
    let handed = Handed::of("0.0.1", &head);
    let (_dir, summary) = summary_file();

    let (text, code) = stage(&git, &handed.settings(&double, Some(summary.clone())));

    assert_eq!(code, 0, "{text}");
    // The order is what an immutable release needs: the draft first, then
    // every archive, then the release published.
    assert_eq!(double.calls(), attaching());
    let expected: Vec<(String, String)> = TARGETS
        .iter()
        .map(|target| (archive_name(target), digest(target)))
        .collect();
    assert_eq!(double.assets(1), expected);
    for (request, target) in double.requests()[4..7].iter().zip(TARGETS) {
        assert_eq!(request.raw, bytes(target));
        assert_eq!(
            request.content_type.as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(
            request.authorization.as_deref(),
            Some(format!("Bearer {TOKEN}").as_str())
        );
    }
    let release = double.release("v0.0.1");
    assert_eq!(release["draft"], false);
    let names: Vec<String> = TARGETS.iter().map(|target| archive_name(target)).collect();
    assert!(
        text.contains(&format!(
            "the repository: the archives for v0.0.1 are {}.\n\
             the repository: tagged {head} as v0.0.1.\n\
             the repository: drafted the release v0.0.1.\n\
             the repository: attached {} on the draft of v0.0.1, {}.\n",
            names.join(", "),
            names[0],
            digest(TARGETS[0])
        )),
        "{text}"
    );
    assert!(!text.contains("::error::"), "{text}");
    assert!(
        fs::read_to_string(&summary)
            .expect("the summary")
            .contains("| the repository | [v0.0.1]("),
        "the summary"
    );
}

#[test]
fn a_re_run_of_a_run_that_attached_the_archives_changes_nothing() {
    let (git, head) = binary();
    let double = Double::new();
    let handed = Handed::of("0.0.1", &head);
    let settings = handed.settings(&double, None);
    let (text, code) = stage(&git, &settings);
    assert_eq!(code, 0, "{text}");
    let releases = double.state().releases.clone();
    let assets = double.state().assets.clone();
    git.tag("v0.0.1", &head);
    let written = double.requests().len();

    let (text, code) = stage(&git, &settings);

    assert_eq!(code, 0, "{text}");
    let calls: Vec<String> = double.calls().split_off(written);
    assert_eq!(calls, ["GET releases?per_page=100&page=1"]);
    assert_eq!(double.state().releases, releases);
    assert_eq!(double.state().assets, assets);
}

#[test]
fn a_re_run_keeps_an_archive_of_the_same_digest_and_replaces_any_other() {
    let (git, head) = binary();
    git.tag("v0.0.1", &head);
    let double = Double::new();
    let id = double.holds("v0.0.1", true);
    // The first archive is there as built, the second with other bytes, the
    // third from an upload that broke off; and one asset no build hands on.
    double.holds_asset(
        id,
        &archive_name(TARGETS[0]),
        &bytes(TARGETS[0]),
        "uploaded",
    );
    let other = double.holds_asset(id, &archive_name(TARGETS[1]), b"other", "uploaded");
    let broken = double.holds_asset(id, &archive_name(TARGETS[2]), &bytes(TARGETS[2]), "starter");
    double.holds_asset(id, "notes.txt", b"notes", "uploaded");
    let handed = Handed::of("0.0.1", &head);

    let (text, code) = stage(&git, &handed.settings(&double, None));

    assert_eq!(code, 0, "{text}");
    assert_eq!(
        double.calls(),
        [
            "GET releases?per_page=100&page=1".to_owned(),
            format!("GET releases/{id}/assets?per_page=100&page=1"),
            format!("DELETE releases/assets/{other}"),
            format!("POST {}", upload(id, &archive_name(TARGETS[1]))),
            format!("DELETE releases/assets/{broken}"),
            format!("POST {}", upload(id, &archive_name(TARGETS[2]))),
            format!("PATCH releases/{id}"),
            format!("GET releases/{id}"),
        ]
    );
    let mut expected: Vec<(String, String)> = vec![
        (archive_name(TARGETS[0]), digest(TARGETS[0])),
        (
            "notes.txt".to_owned(),
            format!("sha256:{}", sha256(b"notes")),
        ),
        (archive_name(TARGETS[1]), digest(TARGETS[1])),
        (archive_name(TARGETS[2]), digest(TARGETS[2])),
    ];
    expected.sort();
    let mut assets = double.assets(id);
    assets.sort();
    assert_eq!(assets, expected);
    assert!(
        text.contains(&format!(
            "the repository: the draft of v0.0.1 holds {} with the same digest already.\n\
             the repository: replaced {} on the draft of v0.0.1, {}.\n",
            archive_name(TARGETS[0]),
            archive_name(TARGETS[1]),
            digest(TARGETS[1])
        )),
        "{text}"
    );
}

#[test]
fn the_assets_of_a_draft_on_a_later_page_are_found() {
    let (git, head) = binary();
    git.tag("v0.0.1", &head);
    let double = Double::new();
    let id = double.holds("v0.0.1", true);
    for n in 0..120 {
        double.holds_asset(id, &format!("other-{n}"), b"x", "uploaded");
    }
    for target in TARGETS {
        double.holds_asset(id, &archive_name(target), &bytes(target), "uploaded");
    }
    let handed = Handed::of("0.0.1", &head);

    let (text, code) = stage(&git, &handed.settings(&double, None));

    assert_eq!(code, 0, "{text}");
    assert_eq!(
        double.calls(),
        [
            "GET releases?per_page=100&page=1".to_owned(),
            format!("GET releases/{id}/assets?per_page=100&page=1"),
            format!("GET releases/{id}/assets?per_page=100&page=2"),
            format!("PATCH releases/{id}"),
            format!("GET releases/{id}"),
        ]
    );
}

#[test]
fn an_upload_that_fails_half_way_publishes_nothing() {
    let (git, head) = binary();
    let double = Double::new();
    let second = upload(1, &archive_name(TARGETS[1]));
    double.answer(
        "POST",
        &second,
        500,
        &json!({ "message": "Broken" }).to_string(),
    );
    let handed = Handed::of("0.0.1", &head);

    let (text, code) = stage(&git, &handed.settings(&double, None));

    assert_eq!(code, 1, "{text}");
    let address = &double.address;
    assert!(
        text.ends_with(&format!(
            "::error::GitHub refused POST {address}{second} with 500: Broken\n"
        )),
        "{text}"
    );
    assert_eq!(double.calls().last(), Some(&format!("POST {second}")));
    assert!(
        !double.calls().iter().any(|call| call.starts_with("PATCH")),
        "{:?}",
        double.calls()
    );
    assert_eq!(double.release("v0.0.1")["draft"], true);
}

#[test]
fn an_upload_github_does_not_hold_as_it_was_sent_fails_the_stage() {
    let name = archive_name(TARGETS[0]);
    let ours = digest(TARGETS[0]);
    for (answer, expected) in [
        (
            json!({ "id": 9, "name": name, "state": "uploaded", "digest": "sha256:00" }),
            format!("as {name} with the digest sha256:00, uploaded,"),
        ),
        (
            json!({ "id": 9, "name": "renamed", "state": "uploaded" }),
            format!("as renamed with the digest {ours}, uploaded,"),
        ),
        (
            json!({ "id": 9, "name": name, "state": "starter", "digest": ours }),
            format!("as {name} with the digest {ours}, not uploaded,"),
        ),
    ] {
        let (git, head) = binary();
        let double = Double::new();
        double.answer("POST", &upload(1, &name), 201, &answer.to_string());
        let handed = Handed::of("0.0.1", &head);

        let (text, code) = stage(&git, &handed.settings(&double, None));

        assert_eq!(code, 1, "{text}");
        assert!(
            text.ends_with(&format!(
                "::error::GitHub holds the upload of {name} to the draft of v0.0.1 {expected} and \
                 the archive is {name} with {ours}\n"
            )),
            "{text}"
        );
        assert!(
            !double.calls().iter().any(|call| call.starts_with("PATCH")),
            "{:?}",
            double.calls()
        );
    }
}

#[test]
fn an_answer_about_assets_without_what_the_call_needs_fails_the_stage() {
    let list = "releases/1/assets?per_page=100&page=1";
    for (path, answer, expected) in [
        (list, "{}", format!("GET {list} without a list")),
        (
            list,
            "[{}]",
            format!("GET {list} without the number of an asset"),
        ),
        (list, r#"[{"id": 1}]"#, format!("GET {list} without `name`")),
        (
            list,
            r#"[{"id": 1, "name": "x"}]"#,
            format!("GET {list} without `state`"),
        ),
    ] {
        let (git, head) = binary();
        let double = Double::new();
        double.answer("GET", path, 200, answer);
        let handed = Handed::of("0.0.1", &head);

        let (text, code) = stage(&git, &handed.settings(&double, None));

        assert_eq!(code, 1, "{text}");
        assert!(
            text.ends_with(&format!("::error::GitHub answered {expected}\n")),
            "{text}"
        );
    }

    let (git, head) = binary();
    let double = Double::new();
    let upload = upload(1, &archive_name(TARGETS[0]));
    double.answer("POST", &upload, 201, "{}");
    let handed = Handed::of("0.0.1", &head);

    let (text, code) = stage(&git, &handed.settings(&double, None));

    assert_eq!(code, 1, "{text}");
    let address = &double.address;
    assert!(
        text.ends_with(&format!(
            "::error::GitHub answered POST {address}{upload} without the number of an asset\n"
        )),
        "{text}"
    );
}

#[test]
fn a_refused_deletion_fails_the_stage() {
    let (git, head) = binary();
    git.tag("v0.0.1", &head);
    let double = Double::new();
    let id = double.holds("v0.0.1", true);
    let other = double.holds_asset(id, &archive_name(TARGETS[0]), b"other", "uploaded");
    let path = format!("releases/assets/{other}");
    double.answer(
        "DELETE",
        &path,
        403,
        &json!({ "message": "No" }).to_string(),
    );
    let handed = Handed::of("0.0.1", &head);

    let (text, code) = stage(&git, &handed.settings(&double, None));

    assert_eq!(code, 1, "{text}");
    assert!(
        text.ends_with(&format!(
            "::error::GitHub refused DELETE {path} with 403: No\n"
        )),
        "{text}"
    );
}

/// The stage on [`binary`] with these artifacts: its log, its exit code, and
/// the calls it made.
fn checked(handed: impl FnOnce(&str, &Handed)) -> (String, u8, Vec<String>) {
    let (git, head) = binary();
    let double = Double::new();
    let artifacts = Handed::new();
    handed(&head, &artifacts);
    let (_dir, summary) = summary_file();
    let (text, code) = stage(&git, &artifacts.settings(&double, Some(summary.clone())));
    let written = fs::read_to_string(&summary).expect("the summary");
    assert!(!written.contains("Published"), "{written}");
    assert!(
        written.ends_with(
            "- Nothing is released in this run: the archives the builds handed on are not the \
             ones the releases take.\n"
        ) || code == 0,
        "{written}"
    );
    (text, code, double.calls())
}

/// The errors of a log, each without the unit in front.
fn errors(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("::error::the repository: "))
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn a_missing_target_fails_by_name_and_nothing_is_written() {
    let (text, code, calls) = checked(|head, handed| {
        handed.archive("0.0.1", head, TARGETS[0]);
        handed.archive("0.0.1", head, TARGETS[2]);
    });

    assert_eq!(code, 1, "{text}");
    assert!(calls.is_empty(), "{calls:?}");
    let head = text
        .split("executable-.@0.0.1@")
        .nth(1)
        .and_then(|rest| rest.split('@').next())
        .expect("the commit in the message")
        .to_owned();
    assert_eq!(
        errors(&text),
        [format!(
            "the archive for {} is missing: the run's builds handed on no artifact {}.",
            TARGETS[1],
            artifact_name("0.0.1", &head, TARGETS[1])
        )]
    );
}

#[test]
fn without_artifacts_every_target_is_missing() {
    let (git, head) = binary();
    let double = Double::new();
    for settings in [
        double.settings(None),
        Handed::new().settings(&double, None),
        {
            let mut settings = double.settings(None);
            settings.assets = Some(PathBuf::from("/nonexistent/artifacts"));
            settings
        },
    ] {
        let (text, code) = stage(&git, &settings);

        assert_eq!(code, 1, "{text}");
        let missing: Vec<String> = TARGETS
            .iter()
            .map(|target| {
                format!(
                    "the archive for {target} is missing: the run's builds handed on no artifact \
                     {}.",
                    artifact_name("0.0.1", &head, target)
                )
            })
            .collect();
        assert_eq!(errors(&text), missing);
    }
    assert!(double.calls().is_empty(), "{:?}", double.calls());
}

#[test]
fn a_folder_of_artifacts_that_cannot_be_read_fails_by_name() {
    let (git, _) = binary();
    let double = Double::new();
    let file = tempfile::NamedTempFile::new().expect("a file");
    let mut settings = double.settings(None);
    settings.assets = Some(file.path().to_owned());

    let (text, code) = stage(&git, &settings);

    assert_eq!(code, 1, "{text}");
    assert!(
        errors(&text)[0].starts_with(&format!(
            "the artifacts of the run's builds cannot be read from {}: ",
            file.path().display()
        )),
        "{text}"
    );
    assert!(double.calls().is_empty(), "{:?}", double.calls());
}

#[test]
fn an_archive_of_another_version_or_commit_fails_by_name() {
    let mut commit = String::new();
    let (text, code, calls) = checked(|head, handed| {
        commit = head.to_owned();
        handed.all("0.0.1", head);
        handed.archive("0.0.2", head, TARGETS[0]);
        handed.archive("0.0.1", &"0".repeat(40), TARGETS[1]);
    });

    assert_eq!(code, 1, "{text}");
    assert!(calls.is_empty(), "{calls:?}");
    let zero = "0".repeat(40);
    assert_eq!(
        errors(&text),
        [
            format!(
                "the artifact {} was built from {zero}, and this run releases {commit}.",
                artifact_name("0.0.1", &zero, TARGETS[1])
            ),
            format!(
                "the artifact {} holds an archive of 0.0.2, and this run releases 0.0.1.",
                artifact_name("0.0.2", &commit, TARGETS[0])
            ),
        ]
    );
}

#[test]
fn an_artifact_for_a_target_the_build_does_not_list_fails_by_name() {
    let mut commit = String::new();
    let (text, code, _) = checked(|head, handed| {
        commit = head.to_owned();
        handed.all("0.0.1", head);
        handed.archive("0.0.1", head, "riscv64gc-unknown-linux-gnu");
    });

    assert_eq!(code, 1, "{text}");
    assert_eq!(
        errors(&text),
        [format!(
            "the artifact {} is for riscv64gc-unknown-linux-gnu, which the build cargo-release \
             does not list.",
            artifact_name("0.0.1", &commit, "riscv64gc-unknown-linux-gnu")
        )]
    );
}

#[test]
fn an_artifact_whose_name_does_not_read_fails_by_name() {
    let (text, code, _) = checked(|head, handed| {
        handed.all("0.0.1", head);
        handed.artifact("executable-.@0.0.1", &[("a.tar.gz", b"x")]);
    });

    assert_eq!(code, 1, "{text}");
    assert_eq!(
        errors(&text),
        [
            "the artifact executable-.@0.0.1 does not read as one of the build cargo-release, which \
          names its artifacts executable-.@<version>@<commit>@<target>."
        ]
    );
}

#[test]
fn a_duplicate_archive_fails_by_name() {
    let mut commit = String::new();
    let (text, code, calls) = checked(|head, handed| {
        commit = head.to_owned();
        handed.all("0.0.1", head);
        // A second archive beside the first in the artifact of one target.
        handed.artifact(
            &artifact_name("0.0.1", head, TARGETS[2]),
            &[("app-copy-x86_64-unknown-linux-musl.tar.gz", b"again")],
        );
    });

    assert_eq!(code, 1, "{text}");
    assert!(calls.is_empty(), "{calls:?}");
    assert_eq!(
        errors(&text),
        [format!(
            "the artifact {} holds 2 entries, and an artifact of a build is a folder that holds \
             exactly one file, the archive.",
            artifact_name("0.0.1", &commit, TARGETS[2])
        )]
    );

    // Two artifacts whose archives would carry the same name on the release.
    let both = "app-aarch64-apple-darwin.-x86_64-apple-darwin.tar.gz";
    let (text, code, _) = checked(|head, handed| {
        handed.archive("0.0.1", head, TARGETS[2]);
        for target in &TARGETS[..2] {
            handed.artifact(&artifact_name("0.0.1", head, target), &[(both, b"x")]);
        }
    });

    assert_eq!(code, 1, "{text}");
    assert_eq!(
        errors(&text),
        [format!(
            "two artifacts hold an archive named {both}, and a release takes one file of a name."
        )]
    );
}

#[test]
fn an_artifact_that_is_no_folder_of_one_archive_fails_by_name() {
    let mut commit = String::new();
    let (text, code, _) = checked(|head, handed| {
        commit = head.to_owned();
        handed.archive("0.0.1", head, TARGETS[0]);
        // A file where the artifact belongs.
        fs::write(
            handed.path().join(artifact_name("0.0.1", head, TARGETS[1])),
            b"x",
        )
        .expect("a file");
        // A folder in the artifact instead of the archive.
        let artifact = handed.path().join(artifact_name("0.0.1", head, TARGETS[2]));
        fs::create_dir_all(artifact.join("inside")).expect("a folder");
    });

    assert_eq!(code, 1, "{text}");
    let artifact = |target| artifact_name("0.0.1", &commit, target);
    let errors = errors(&text);
    assert_eq!(
        errors[0],
        format!(
            "the artifact {} is no folder, and an artifact of a build is a folder that holds \
             exactly one file, the archive.",
            artifact(TARGETS[1])
        )
    );
    assert!(
        errors[1].starts_with(&format!("the artifact {} holds ", artifact(TARGETS[2])))
            && errors[1].ends_with(
                "inside, which is no plain file, and an artifact of a build is a folder that \
                 holds exactly one file, the archive."
            ),
        "{errors:?}"
    );
    assert_eq!(errors.len(), 2, "{errors:?}");
}

#[test]
fn an_archive_that_is_a_link_fails_by_name() {
    let (text, code, _) = checked(|head, handed| {
        handed.archive("0.0.1", head, TARGETS[0]);
        handed.archive("0.0.1", head, TARGETS[1]);
        let artifact = handed.path().join(artifact_name("0.0.1", head, TARGETS[2]));
        fs::create_dir_all(&artifact).expect("the artifact");
        std::os::unix::fs::symlink("/etc/hosts", artifact.join(archive_name(TARGETS[2])))
            .expect("a link");
    });

    assert_eq!(code, 1, "{text}");
    assert!(
        errors(&text)[0].contains(", which is no plain file, "),
        "{text}"
    );
}

#[test]
fn an_archive_whose_name_does_not_name_its_target_fails_by_name() {
    let mut commit = String::new();
    let (text, code, _) = checked(|head, handed| {
        commit = head.to_owned();
        handed.archive("0.0.1", head, TARGETS[0]);
        handed.archive("0.0.1", head, TARGETS[1]);
        // The Linux artifact holds the archive of a macOS build.
        handed.artifact(
            &artifact_name("0.0.1", head, TARGETS[2]),
            &[(&archive_name(TARGETS[1]), b"x")],
        );
    });

    assert_eq!(code, 1, "{text}");
    assert_eq!(
        errors(&text),
        [format!(
            "the artifact {} holds {}, whose name does not name {}.",
            artifact_name("0.0.1", &commit, TARGETS[2]),
            archive_name(TARGETS[1]),
            TARGETS[2]
        )]
    );
}

#[test]
fn an_archive_whose_name_holds_a_control_character_fails_by_name() {
    let (text, code, _) = checked(|head, handed| {
        handed.archive("0.0.1", head, TARGETS[0]);
        handed.archive("0.0.1", head, TARGETS[1]);
        handed.artifact(
            &artifact_name("0.0.1", head, TARGETS[2]),
            &[("app\u{7}-x86_64-unknown-linux-musl.tar.gz", b"x")],
        );
    });

    assert_eq!(code, 1, "{text}");
    assert!(
        errors(&text)[0].ends_with(".tar.gz, whose name is no text without control characters."),
        "{text}"
    );
}

#[test]
fn an_archive_that_cannot_be_read_fails_by_name() {
    let (text, code, _) = checked(|head, handed| {
        handed.all("0.0.1", head);
        let file = handed
            .path()
            .join(artifact_name("0.0.1", head, TARGETS[0]))
            .join(archive_name(TARGETS[0]));
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).expect("no access");
    });

    assert_eq!(code, 1, "{text}");
    let errors = errors(&text);
    assert!(
        errors[0].starts_with("the archive ") && errors[0].contains(" cannot be read: "),
        "{errors:?}"
    );
}

#[test]
fn artifacts_of_other_units_and_kinds_are_passed_over() {
    let (git, head) = binary();
    let double = Double::new();
    let handed = Handed::of("0.0.1", &head);
    handed.artifact(
        &format!("crate-.@0.0.1@{head}"),
        &[("app-0.0.1.crate", b"x")],
    );
    handed.artifact(
        &format!("executable-tools@0.0.1@{head}@{}", TARGETS[0]),
        &[("other", b"x")],
    );
    fs::write(handed.path().join("loose.txt"), b"x").expect("a file");

    let (text, code) = stage(&git, &handed.settings(&double, None));

    assert_eq!(code, 0, "{text}");
    assert_eq!(double.calls(), attaching());
}

#[test]
fn a_unit_that_is_not_released_takes_no_archives() {
    let (git, first) = binary();
    git.tag("v0.0.1", &first);
    git.write(&[("README.md", "# x\n\nMore.\n")]);
    let second = git.commit("fix: more");
    git.tag("v0.0.2", &second);
    git.run(&["checkout", "--quiet", "--detach", &first]);
    let double = Double::new();

    let (text, code) = stage(&git, &Handed::new().settings(&double, None));

    assert_eq!(code, 0, "{text}");
    assert!(
        text.ends_with("Nothing is released in this run.\n"),
        "{text}"
    );
}

#[test]
fn the_units_that_take_archives_are_those_that_publish_them_and_build() {
    let declared = |declaration: &str, files: &[(&str, &str)]| {
        let git = Git::new();
        git.write(&[
            (".nebensound/repo.toml", declaration),
            ("README.md", "# x\n"),
        ]);
        git.write(files);
        git.commit("fix: start");
        let central = central();
        let analysis = Repository::at(git.path())
            .analyse(&central)
            .expect("the fixture reads");
        release::assets::declared(&analysis, &central)
    };

    let found = declared(BINARY, &[("LICENSE", "x\n"), ("Cargo.toml", "[package]\n")]);
    assert_eq!(
        found,
        [release::Declared {
            name: UnitName::Repository,
            path: nebensound_repo::path::RelativePath::root(),
            build: "cargo-release".to_owned(),
            produces: "executable".to_owned(),
            targets: TARGETS.iter().map(|target| (*target).to_owned()).collect(),
        }]
    );
    assert!(declared(QUIET, &[]).is_empty());
    let unpublished = r#"[repo]
audience = "public"
pipeline = { version = ["cargo"], build = "cargo-release", publish = false }
"#;
    assert!(declared(unpublished, &[("Cargo.toml", "[package]\n")]).is_empty());
}

#[test]
fn a_build_without_targets_hands_on_one_archive() {
    let unit = release::Declared {
        name: UnitName::Repository,
        path: nebensound_repo::path::RelativePath::root(),
        build: "typst".to_owned(),
        produces: "document".to_owned(),
        targets: Vec::new(),
    };
    let handed = Handed::new();
    let folder = handed.path();

    assert_eq!(
        release::assets::found(Some(&folder), &unit, "1.0.0", "c"),
        Err(vec![
            "the archive is missing: the run's builds handed on no artifact document-.@1.0.0@c"
                .to_owned()
        ])
    );

    handed.artifact("document-.@1.0.0@c@extra", &[("x.pdf", b"x")]);
    assert_eq!(
        release::assets::found(Some(&folder), &unit, "1.0.0", "c"),
        Err(vec![
            "the artifact document-.@1.0.0@c@extra does not read as one of the build typst, \
             which names its artifacts document-.@<version>@<commit>"
                .to_owned(),
            "the archive is missing: the run's builds handed on no artifact document-.@1.0.0@c"
                .to_owned(),
        ])
    );

    fs::remove_dir_all(folder.join("document-.@1.0.0@c@extra")).expect("removed");
    handed.artifact("document-.@1.0.0@c", &[("manual.pdf", b"the manual")]);
    assert_eq!(
        release::assets::found(Some(&folder), &unit, "1.0.0", "c"),
        Ok(vec![release::Archive {
            name: "manual.pdf".to_owned(),
            file: folder.join("document-.@1.0.0@c").join("manual.pdf"),
            sha256: sha256(b"the manual"),
        }])
    );
}

#[test]
fn the_artifact_names_are_those_of_the_build() {
    let path = nebensound_repo::path::RelativePath::new("tools/a b").expect("a path");
    assert_eq!(
        release::assets::artifact("executable", &path, "1.0.0", "c", Some("t")),
        nebensound_repo::build::cargo_release::artifact("tools/a b", "1.0.0", "c", "t")
    );
    assert_eq!(
        release::assets::artifact("crate", &path, "1.0.0", "c", None),
        nebensound_repo::build::cargo_package::artifact("tools/a b", "1.0.0", "c")
    );
}

#[test]
fn the_call_hands_the_stage_the_folder_of_the_artifacts() {
    let (git, head) = binary();
    let handed = Handed::new();
    handed.archive("0.0.1", &head, TARGETS[0]);
    let (_dir, summary) = summary_file();
    let owned = run_variables(&summary);

    let (text, code) = tool_with(
        git.path(),
        &["--assets", handed.path().to_str().expect("UTF-8")],
        &borrowed(&owned),
    );

    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains(&format!(
            "the repository: the archive for {} is missing",
            TARGETS[1]
        )),
        "{text}"
    );
    assert!(
        !text.contains(&format!("the archive for {} is missing", TARGETS[0])),
        "{text}"
    );
}

// What is decided, from the records alone.

/// A unit with a version of this run.
fn unit(name: UnitName, prefix: &str, version: &str) -> UnitVersion {
    UnitVersion {
        name,
        path: nebensound_repo::path::RelativePath::root(),
        prefix: prefix.to_owned(),
        current: TagVersion::parse(version),
        version: TagVersion::parse(version),
        recorded: false,
    }
}

fn records(
    head: &str,
    tags: &[(&str, Option<&str>)],
    holding: &[&str],
    inside: &[&str],
) -> Records {
    Records {
        head: head.to_owned(),
        tags: tags
            .iter()
            .map(|(name, commit)| Tag {
                name: (*name).to_owned(),
                commit: commit.map(ToOwned::to_owned),
            })
            .collect(),
        holding_head: holding.iter().map(|name| (*name).to_owned()).collect(),
        held_by_head: inside.iter().map(|name| (*name).to_owned()).collect(),
    }
}

#[test]
fn a_unit_without_a_version_of_this_run_is_not_decided_on() {
    let mut untouched = unit(UnitName::Repository, "v", "1.0.0");
    untouched.version = None;

    assert!(release::decide(&[untouched], &records("h", &[], &[], &[]), "pipeline").is_empty());
}

#[test]
fn the_records_decide_previous_newer_and_conflict() {
    let repository = unit(UnitName::Repository, "v", "1.2.0");
    let decided = |records: &Records| -> What {
        let planned: Vec<Planned> =
            release::decide(std::slice::from_ref(&repository), records, "pipeline");
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].tag, "v1.2.0");
        assert_eq!(planned[0].title(), "pipeline 1.2.0");
        assert!(planned[0].latest());
        planned[0].what.clone()
    };

    // The highest record below, on a commit `HEAD` contains; a pre-release
    // of the version itself is below it.
    let previous = records(
        "h",
        &[
            ("v1.1.0", Some("a")),
            ("v1.2.0-pre.3", Some("b")),
            ("v1.1.9", Some("side")),
            ("v1.0.0", Some("c")),
        ],
        &[],
        &["v1.1.0", "v1.2.0-pre.3", "v1.0.0"],
    );
    assert_eq!(
        decided(&previous),
        What::Release {
            previous: Some("v1.2.0-pre.3".to_owned())
        }
    );

    // The lowest record above that holds `HEAD` is not what counts: the
    // highest one is named.
    let newer = records(
        "h",
        &[
            ("v1.3.0", Some("n")),
            ("v2.0.0", Some("m")),
            ("v9.0.0", Some("x")),
        ],
        &["v1.3.0", "v2.0.0"],
        &[],
    );
    assert_eq!(
        decided(&newer),
        What::Held {
            newer: "v2.0.0".to_owned()
        }
    );

    // Its own tag on `HEAD` is the record of this release, not a newer one.
    let own = records("h", &[("v1.2.0", Some("h"))], &["v1.2.0"], &["v1.2.0"]);
    assert_eq!(decided(&own), What::Release { previous: None });

    let elsewhere = records("h", &[("v1.2.0", Some("o"))], &[], &[]);
    assert_eq!(
        decided(&elsewhere),
        What::Conflict {
            commit: Some("o".to_owned())
        }
    );
}

#[test]
fn a_component_is_named_by_its_name_and_is_never_the_latest() {
    let name = nebensound_repo::names::ComponentName::new("notes").expect("a name");
    let component = unit(UnitName::Component(name), "notes-v", "0.3.0");

    let planned = release::decide(&[component], &records("h", &[], &[], &[]), "pipeline");

    assert_eq!(planned[0].display, "notes");
    assert_eq!(planned[0].title(), "notes 0.3.0");
    assert!(!planned[0].latest());
}

// The records as Git gives them.

#[test]
fn the_records_name_what_holds_the_commit_and_what_it_holds() {
    let (git, first, second) = second();
    git.run(&[
        "tag",
        "--annotate",
        "--message",
        "annotated",
        "v0.0.2",
        &second,
    ]);
    git.run(&["checkout", "--quiet", "--detach", &first]);

    let records = Repository::at(git.path()).records().expect("the records");

    assert_eq!(records.head, first);
    assert_eq!(
        records.tags,
        [
            Tag {
                name: "v0.0.1".to_owned(),
                commit: Some(first.clone())
            },
            Tag {
                name: "v0.0.2".to_owned(),
                commit: Some(second.clone())
            },
        ]
    );
    assert_eq!(
        records.holding_head.iter().collect::<Vec<_>>(),
        ["v0.0.1", "v0.0.2"]
    );
    assert_eq!(records.held_by_head.iter().collect::<Vec<_>>(), ["v0.0.1"]);
}

#[test]
fn a_git_that_fails_on_the_records_is_named() {
    let (git, _) = single();
    let real = find::search("git", &std::env::var_os("PATH").unwrap_or_default(), false)
        .expect("git is installed");
    let bin = tempfile::tempdir().expect("a directory");
    // Each time one call of the records fails: the commit at hand, the tags,
    // the tags that hold it, the tags it holds.
    for word in ["rev-parse", "%(objecttype)", "--contains", "--merged"] {
        let fake = bin.path().join("git");
        executable(
            &fake,
            &format!(
                "#!/bin/sh\nfor argument in \"$@\"; do\n  case \"$argument\" in\n                     *'{word}'*) echo 'refused' >&2; exit 128 ;;\n  esac\ndone\nexec '{}' \"$@\"\n",
                real.display()
            ),
        );

        let refused = Repository::at(git.path())
            .with_git(&fake)
            .records()
            .expect_err("git refused");

        assert!(
            refused.to_string().ends_with(": refused"),
            "{word}: {refused}"
        );
    }
}

#[test]
fn a_repository_without_a_commit_has_no_records() {
    let empty = repository(&[(".nebensound/repo.toml", QUIET)]);

    let refused = Repository::at(empty.path())
        .records()
        .expect_err("no commit");

    assert!(
        refused.to_string().contains("HEAD names no commit"),
        "{refused}"
    );
}

// The settings, from the variables of a run.

fn variables(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    move |name| map.get(name).cloned()
}

const RUN: &[(&str, &str)] = &[
    ("GITHUB_REPOSITORY", "acme/pipeline"),
    ("GITHUB_API_URL", "https://api.github.com/"),
    ("GITHUB_TOKEN", TOKEN),
    ("GITHUB_STEP_SUMMARY", "/tmp/summary"),
];

#[test]
fn the_settings_come_from_the_variables_of_a_run() {
    let settings =
        Settings::read(&variables(RUN), OsString::from("/usr/bin")).expect("the settings");

    assert_eq!(
        settings,
        Settings {
            github: GitHub {
                api: "https://api.github.com".to_owned(),
                repository: "acme/pipeline".to_owned(),
                token: TOKEN.to_owned(),
                search_path: OsString::from("/usr/bin"),
                protocols: "=https".to_owned(),
            },
            summary: Some(PathBuf::from("/tmp/summary")),
            assets: None,
        }
    );

    let without: Vec<(&str, &str)> = RUN
        .iter()
        .copied()
        .filter(|(name, _)| *name != "GITHUB_STEP_SUMMARY")
        .collect();
    let settings = Settings::read(&variables(&without), OsString::new()).expect("the settings");
    assert_eq!(settings.summary, None);
}

#[test]
fn a_missing_or_broken_variable_is_named() {
    let with = |name: &str, value: Option<&str>| -> String {
        let pairs: Vec<(&str, &str)> = RUN
            .iter()
            .copied()
            .filter(|(each, _)| *each != name)
            .chain(value.map(|value| (name, value)))
            .collect();
        Settings::read(&variables(&pairs), OsString::new()).expect_err("refused")
    };

    for name in ["GITHUB_REPOSITORY", "GITHUB_API_URL", "GITHUB_TOKEN"] {
        let expected = format!("the release stage needs {name}, and it is not set");
        assert_eq!(with(name, None), expected);
        assert_eq!(with(name, Some("")), expected);
    }
    let broken = format!("{TOKEN}\nX-Injected: yes");
    let refused = with("GITHUB_TOKEN", Some(&broken));
    assert_eq!(refused, "GITHUB_TOKEN holds a control character");
    for repository in ["pipeline", "/pipeline", "acme/", "acme/pipe/line"] {
        assert_eq!(
            with("GITHUB_REPOSITORY", Some(repository)),
            format!(
                "GITHUB_REPOSITORY is {repository:?}, and the release stage needs the repository \
                 as owner/name"
            )
        );
    }
}

// The call, as a workflow makes it.

/// The built tool, with these variables of a run and none of the run it
/// may itself be part of.
fn tool(dir: &Path, variables: &[(&str, &OsString)]) -> (String, Option<i32>) {
    tool_with(dir, &[], variables)
}

/// The same, with these arguments of `release`.
fn tool_with(dir: &Path, args: &[&str], variables: &[(&str, &OsString)]) -> (String, Option<i32>) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nebensound-repo"));
    command.args(["-C", dir.to_str().expect("UTF-8"), "release"]);
    command.args(args);
    // Not cleared as a whole: what measures the coverage of the tool comes
    // through the environment as well.
    for name in [
        "GITHUB_ACTIONS",
        "GITHUB_REPOSITORY",
        "GITHUB_API_URL",
        "GITHUB_TOKEN",
        "GITHUB_STEP_SUMMARY",
    ] {
        command.env_remove(name);
    }
    for (name, value) in variables {
        command.env(name, value);
    }
    let output = command.output().expect("the built binary");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code(),
    )
}

fn run_variables(summary: &Path) -> Vec<(&'static str, OsString)> {
    vec![
        ("GITHUB_REPOSITORY", OsString::from(REPOSITORY)),
        (
            "GITHUB_API_URL",
            OsString::from("https://api.github.invalid"),
        ),
        ("GITHUB_TOKEN", OsString::from(TOKEN)),
        ("GITHUB_STEP_SUMMARY", summary.as_os_str().to_owned()),
    ]
}

fn borrowed<'v>(variables: &'v [(&'static str, OsString)]) -> Vec<(&'static str, &'v OsString)> {
    variables
        .iter()
        .map(|(name, value)| (*name, value))
        .collect()
}

#[test]
fn the_call_without_the_variables_of_a_run_is_wrong() {
    let (git, _) = single();

    let (text, code) = tool(git.path(), &[]);

    assert_eq!(code, Some(2), "{text}");
    assert_eq!(
        text,
        "the release stage needs GITHUB_REPOSITORY, and it is not set\n"
    );
}

#[test]
fn the_call_releases_nothing_when_phase_0_finds_errors() {
    let git = Git::new();
    git.write(&[
        (".nebensound/repo.toml", QUIET),
        ("src/main.rs", "fn main() {}\n"),
    ]);
    git.commit("fix: start");
    let (_dir, summary) = summary_file();
    let owned = run_variables(&summary);

    let (text, code) = tool(git.path(), &borrowed(&owned));

    assert_eq!(code, Some(1), "{text}");
    assert_eq!(
        text,
        "nothing is released: phase 0 found errors, and `check` says which\n"
    );
}

#[test]
fn the_call_on_a_repository_without_a_commit_fails() {
    let empty = repository(&[(".nebensound/repo.toml", QUIET), ("README.md", "# x\n")]);
    let (_dir, summary) = summary_file();
    let owned = run_variables(&summary);

    let (text, code) = tool(empty.path(), &borrowed(&owned));

    assert_eq!(code, Some(1), "{text}");
    assert!(text.contains("HEAD names no commit"), "{text}");
}

#[test]
fn the_call_writes_its_summary_where_the_run_says() {
    let (git, first, second) = second();
    git.tag("v0.0.2", &second);
    git.run(&["checkout", "--quiet", "--detach", &first]);
    let (_dir, summary) = summary_file();
    let owned = run_variables(&summary);

    let (text, code) = tool(git.path(), &borrowed(&owned));

    assert_eq!(code, Some(0), "{text}");
    assert!(
        text.ends_with("Nothing is released in this run.\n"),
        "{text}"
    );
    assert!(
        fs::read_to_string(&summary)
            .expect("the summary")
            .starts_with("### The release stage on main\n"),
        "the summary"
    );
}

fn executable(file: &Path, text: &str) {
    fs::write(file, text).expect("a file");
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).expect("executable");
}
