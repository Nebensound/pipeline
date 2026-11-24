//! The one place the output of a call goes through, and the rule that keeps
//! text from outside the pipeline from starting a workflow command.
//!
//! In a workflow run the runner of GitHub Actions reads every line of a step's
//! log as a possible workflow command: `::name::` at the start of a line after
//! any white space, `##[name]` anywhere in a line, and a bare carriage return
//! ends a line. A program the tool starts and the repository it checks control
//! much of what the tool prints: the names of files, what a program says about
//! them, the text of a finding. Rewriting that text so that the runner reads
//! nothing in it would mirror the runner's parser, and every case it missed
//! would start a command.
//!
//! So the rule is this: only the tool writes workflow commands, and everything
//! else is printed while the runner's command processing is stopped. The
//! console opens a stretch with `::stop-commands::<token>` before the first
//! byte of text and closes it with `::<token>::` right before the tool writes
//! a command of its own ([`Console::command`]). Between the two nothing takes
//! effect: neither form of a command, nor a mask, a group or an annotation, and
//! the lines stay in the log as they were printed. Every stretch has a token of
//! its own, 16 bytes from the operating system's randomness ([`getrandom`]),
//! written as 32 hexadecimal digits. The token never reaches a program the tool
//! starts, neither as an argument nor in a variable, so nothing a program
//! prints can name it; the runner masks it in the log. A stretch still open
//! when the call ends is closed then ([`Console::end`]), so that a later call
//! in the same step starts with commands processed; the runner ends a stopped
//! state with the step in any case.
//!
//! The runner prints both lines in the log, the token masked, so every
//! stretch costs two lines. To keep the log short, an annotation does not
//! close the stretch at once: it waits until the tool writes its next other
//! command, such as the end of a group, or until the call ends
//! ([`Console::annotate`]). So what a program printed stands as one stretch,
//! and the findings the tool read off it follow it together.
//!
//! The runner reads a step's standard error before its standard output within
//! a batch, and both share the one stopped state, so the tool writes
//! everything to one output, and every program it starts writes into a pipe
//! the tool reads, never into the step's outputs directly.
//!
//! Outside a workflow run nothing is a command: text goes out as it is, and a
//! command of the tool is written the way a reader takes it in ([`plain`]).

use std::fmt::Write as _;
use std::io::{self, Write};

use crate::report::shown;

/// How the console draws the 16 bytes of a token. The operating system's
/// randomness ([`randomness`]) in every call; a test hands in its own.
pub type Draw = fn() -> Result<[u8; 16], getrandom::Error>;

/// 16 bytes from the operating system's randomness.
///
/// # Errors
///
/// When the operating system gives none.
pub fn randomness() -> Result<[u8; 16], getrandom::Error> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map(|()| bytes)
}

/// The exit code of a call whose console could draw no token: the code of a
/// repository with a problem, since what it printed was left out.
const UNSTOPPED: u8 = 1;

/// Where the output of a call goes: see [the module](self).
pub struct Console<'o> {
    out: &'o mut dyn Write,
    workflow: bool,
    draw: Draw,
    /// The token of the stretch that is open, if one is.
    stopped: Option<String>,
    /// Whether the last byte written ended a line, so that a line of the
    /// console's own starts at the start of a line.
    at_start: bool,
    /// Whether a token could not be drawn: from then on, text is left out.
    unstopped: bool,
    /// The annotations that wait for the next other command.
    waiting: Vec<String>,
}

impl<'o> Console<'o> {
    /// The console on `out`, of a workflow run or not, with tokens from the
    /// operating system's randomness.
    pub fn new(out: &'o mut dyn Write, workflow: bool) -> Self {
        Self::drawing(out, workflow, randomness)
    }

    /// The same with tokens drawn by `draw`.
    pub fn drawing(out: &'o mut dyn Write, workflow: bool, draw: Draw) -> Self {
        Self {
            out,
            workflow,
            draw,
            stopped: None,
            at_start: true,
            unstopped: false,
            waiting: Vec::new(),
        }
    }

    /// Whether this is the log of a workflow run, where a finding is an
    /// annotation.
    #[must_use]
    pub fn workflow(&self) -> bool {
        self.workflow
    }

    /// Writes text: in a workflow run inside a stopped stretch, which opens
    /// here when none is open.
    ///
    /// When the operating system gives no randomness for a token, the text
    /// is left out rather than written where it could start a command, and
    /// the console says so once, as an annotation; the call then fails
    /// ([`Console::end`]).
    pub fn bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.workflow && self.stopped.is_none() {
            match (self.draw)() {
                Ok(drawn) => {
                    let token = drawn.iter().fold(String::new(), |mut token, byte| {
                        let _ = write!(token, "{byte:02x}");
                        token
                    });
                    self.own(&format!("::stop-commands::{token}"));
                    self.stopped = Some(token);
                }
                Err(error) => {
                    if !self.unstopped {
                        self.unstopped = true;
                        self.own(&format!(
                            "::error::The operating system gives no randomness for the token \
                             that stops workflow commands ({error}), so what programs and the \
                             repository print is left out of this log."
                        ));
                    }
                    return;
                }
            }
        }
        self.write_out(bytes);
    }

    /// Writes text, as [`Console::bytes`] does.
    pub fn text(&mut self, text: &str) {
        self.bytes(text.as_bytes());
    }

    /// Writes a line of text, as [`Console::bytes`] does.
    pub fn line(&mut self, text: &str) {
        self.text(text);
        self.text("\n");
    }

    /// Writes a workflow command of the tool's own, escaped by whoever built
    /// it: in a workflow run after the stretch that is open is closed,
    /// elsewhere the way a reader takes it in ([`plain`]), and the end of a
    /// group not at all.
    ///
    /// The annotations that wait are written first.
    pub fn command(&mut self, command: &str) {
        self.release();
        self.write_command(command);
    }

    /// Writes an annotation of the tool's own, escaped by whoever built it,
    /// as [`Console::command`] does, once the tool writes its next other
    /// command or the call ends: see [the module](self).
    pub fn annotate(&mut self, annotation: &str) {
        self.waiting.push(annotation.to_owned());
    }

    /// Writes the annotations that wait.
    fn release(&mut self) {
        for annotation in std::mem::take(&mut self.waiting) {
            self.write_command(&annotation);
        }
    }

    fn write_command(&mut self, command: &str) {
        if self.workflow {
            self.resume();
            self.own(command);
        } else if let Some(line) = plain(command) {
            self.write_out(line.as_bytes());
            self.write_out(b"\n");
        }
    }

    /// Ends the call: closes the stretch that is open, and returns its exit
    /// code, which fails when text was left out for want of a token.
    #[must_use]
    pub fn end(mut self, code: u8) -> u8 {
        self.release();
        self.resume();
        let _ = self.out.flush();
        if self.unstopped {
            code.max(UNSTOPPED)
        } else {
            code
        }
    }

    /// Closes the stretch that is open, if one is.
    fn resume(&mut self) {
        if let Some(token) = self.stopped.take() {
            self.own(&format!("::{token}::"));
        }
    }

    /// Writes a line of the console's own at the start of a line.
    fn own(&mut self, line: &str) {
        if !self.at_start {
            self.write_out(b"\n");
        }
        self.write_out(line.as_bytes());
        self.write_out(b"\n");
    }

    /// Writes, and stays quiet when the output is gone: a message about that
    /// would take the same road. Text that is empty never comes here except
    /// before a line break ([`Console::command`] outside a run), which sets
    /// the start of a line again.
    fn write_out(&mut self, bytes: &[u8]) {
        let _ = self.out.write_all(bytes);
        self.at_start = bytes.ends_with(b"\n");
    }
}

/// A console that goes without [`Console::end`] still writes the annotations
/// that wait and closes the stretch that is open.
impl Drop for Console<'_> {
    fn drop(&mut self) {
        self.release();
        self.resume();
    }
}

/// Text written through [`Write`] is text: see [`Console::bytes`].
impl Write for Console<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// A command of the tool as it is written outside a workflow run.
///
/// An annotation (`::error`, `::warning` or `::notice`) becomes
/// `file:line:col: error: message`, with the parts it does not give left
/// out, its title in brackets behind the level and every escape undone. A
/// group (`::group::name`) becomes its name, and its end nothing, [`None`].
/// Every other line stays as it is.
#[must_use]
pub fn plain(line: &str) -> Option<String> {
    if line == "::endgroup::" {
        return None;
    }
    if let Some(name) = line.strip_prefix("::group::") {
        return Some(unescape(name));
    }
    let Some((level, properties, message)) = annotation(line) else {
        return Some(line.to_owned());
    };
    let mut place = Vec::new();
    let mut title = String::new();
    for pair in properties.split(',') {
        match pair.split_once('=') {
            // The name the property escapes for the runner, shown to a reader
            // the way the report shows it.
            Some(("file", value)) => place.insert(0, shown(&unescape(value))),
            Some(("line" | "col", value)) => place.push(unescape(value)),
            Some(("title", value)) => title = format!("[{}]", unescape(value)),
            _ => {}
        }
    }
    let message = unescape(message);
    Some(if place.is_empty() {
        format!("{level}{title}: {message}")
    } else {
        format!("{}: {level}{title}: {message}", place.join(":"))
    })
}

/// An annotation, taken apart: its level, its properties (empty when it has
/// none) and its message, all still escaped. Escaped, a property holds no
/// `:`. [`None`] for any other line.
#[must_use]
pub fn annotation(line: &str) -> Option<(&str, &str, &str)> {
    let rest = line.strip_prefix("::")?;
    let level = ["error", "warning", "notice"]
        .into_iter()
        .find(|level| rest.starts_with(level))?;
    let rest = &rest[level.len()..];
    if let Some(message) = rest.strip_prefix("::") {
        return Some((level, "", message));
    }
    let (properties, message) = rest.strip_prefix(' ')?.split_once("::")?;
    (!properties.contains(':')).then_some((level, properties, message))
}

/// A property of a workflow command, such as the `file=` of an annotation,
/// in the runner's own escaping: `%`, a carriage return and a line break,
/// which would end the command's line, as `%25`, `%0D` and `%0A`, and `:` and
/// `,`, which end a value, as `%3A` and `%2C`. The runner undoes these, so a
/// file is named as it stands and the annotation sits on it.
///
/// The runner has no escape for any other control character, such as a tab
/// or an escape character. Such a character cannot end the line, so no
/// command starts from it, but the tool writes no control character from the
/// repository into a line of its own: it is written as [`shown`] writes it
/// (`\t`, `\u{1b}`). An annotation on such a file then names no file, and its
/// message still names it; phase 0 refuses a tracked name that holds one
/// (`control-character`).
#[must_use]
pub fn property(text: &str) -> String {
    let mut written = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '%' => written.push_str("%25"),
            '\r' => written.push_str("%0D"),
            '\n' => written.push_str("%0A"),
            ':' => written.push_str("%3A"),
            ',' => written.push_str("%2C"),
            other if other.is_control() => written.extend(other.escape_debug()),
            other => written.push(other),
        }
    }
    written
}

/// Undoes the escapes of a workflow command.
#[must_use]
pub fn unescape(text: &str) -> String {
    text.replace("%0A", "\n")
        .replace("%0D", "\r")
        .replace("%3A", ":")
        .replace("%2C", ",")
        .replace("%25", "%")
}
