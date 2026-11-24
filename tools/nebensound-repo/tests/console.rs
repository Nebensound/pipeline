//! The one place the output goes through, and the rule that keeps text from
//! outside the pipeline from starting a workflow command.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::io::Write as _;

use nebensound_repo::console::{self, Console};

/// Tokens that count up from 1, so that each stretch shows its own.
#[expect(
    clippy::unnecessary_wraps,
    reason = "a token is drawn with the signature `console::Draw` gives"
)]
fn counting() -> Result<[u8; 16], getrandom::Error> {
    use std::sync::atomic::{AtomicU8, Ordering};
    static NEXT: AtomicU8 = AtomicU8::new(1);
    let mut bytes = [0; 16];
    bytes[15] = NEXT.fetch_add(1, Ordering::SeqCst);
    Ok(bytes)
}

fn none() -> Result<[u8; 16], getrandom::Error> {
    Err(getrandom::Error::UNSUPPORTED)
}

/// What a console of a workflow run or not writes, and its exit code, for
/// what `write` does with it.
fn written(
    workflow: bool,
    draw: console::Draw,
    code: u8,
    write: impl FnOnce(&mut Console<'_>),
) -> (String, u8) {
    let mut out = Vec::new();
    let mut console = Console::drawing(&mut out, workflow, draw);
    write(&mut console);
    let code = console.end(code);
    (String::from_utf8(out).unwrap(), code)
}

/// The token of every stretch in `text`, in order.
fn tokens(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| line.strip_prefix("::stop-commands::"))
        .collect()
}

#[test]
fn text_stands_in_a_stopped_stretch_and_a_command_outside_it() {
    let (text, code) = written(true, counting, 0, |console| {
        console.command("::group::A step");
        console.line("a\n::warning::x and ##[warning]y");
        console.text("no line break at the end");
        console.command("::endgroup::");
        console.command("::notice::own");
        console.bytes(b"");
        console.line("again");
        assert!(console.workflow());
    });
    assert_eq!(code, 0);
    let tokens = tokens(&text);
    assert_eq!(tokens.len(), 2, "{text}");
    assert_ne!(tokens[0], tokens[1]);
    assert!(
        tokens
            .iter()
            .all(|token| token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())),
        "{tokens:?}"
    );
    assert_eq!(
        text,
        format!(
            "::group::A step\n::stop-commands::{one}\na\n::warning::x and ##[warning]y\nno line \
             break at the end\n::{one}::\n::endgroup::\n::notice::own\n::stop-commands::{two}\n\
             again\n::{two}::\n",
            one = tokens[0],
            two = tokens[1]
        )
    );
}

#[test]
fn an_annotation_waits_for_the_next_other_command_or_the_end() {
    let (text, _) = written(true, counting, 0, |console| {
        console.line("first");
        console.annotate("::error::one");
        console.line("second");
        console.annotate("::warning::two");
        console.command("::endgroup::");
        console.line("third");
        console.annotate("::error::three");
    });
    let tokens = tokens(&text);
    assert_eq!(
        text,
        format!(
            "::stop-commands::{one}\nfirst\nsecond\n::{one}::\n::error::one\n::warning::two\n\
             ::endgroup::\n::stop-commands::{two}\nthird\n::{two}::\n::error::three\n",
            one = tokens[0],
            two = tokens[1]
        )
    );
}

#[test]
fn text_through_write_is_text() {
    let (text, _) = written(true, counting, 0, |console| {
        write!(console, "written").unwrap();
        console.flush().unwrap();
    });
    let token = tokens(&text)[0];
    assert_eq!(
        text,
        format!("::stop-commands::{token}\nwritten\n::{token}::\n")
    );
}

#[test]
fn the_randomness_of_the_system_gives_a_new_token_each_time() {
    let one = console::randomness().unwrap();
    let two = console::randomness().unwrap();
    assert_ne!(one, two);
    let mut out = Vec::new();
    let mut console = Console::new(&mut out, true);
    console.line("x");
    console.command("::endgroup::");
    console.line("y");
    assert_eq!(console.end(0), 0);
    let text = String::from_utf8(out).unwrap();
    let tokens = tokens(&text);
    assert_eq!(tokens.len(), 2, "{text}");
    assert_ne!(tokens[0], tokens[1]);
}

#[test]
fn without_randomness_text_is_left_out_and_the_call_fails() {
    let (text, code) = written(true, none, 0, |console| {
        console.line("a ##[warning]name");
        console.line("more");
        console.command("::endgroup::");
    });
    assert_eq!(code, 1);
    assert_eq!(
        text,
        "::error::The operating system gives no randomness for the token that stops workflow \
         commands (getrandom: this target is not supported), so what programs and the \
         repository print is left out of this log.\n::endgroup::\n"
    );
    let (_, code) = written(true, none, 2, |console| console.line("x"));
    assert_eq!(code, 2);
}

#[test]
fn outside_a_workflow_run_nothing_is_a_command() {
    let (text, code) = written(false, none, 0, |console| {
        console.command("::group::A step");
        console.line("::warning::x stays as it is");
        console.annotate("::error file=a.md,line=3::wrong");
        console.command("::endgroup::");
        console.command("plain");
        assert!(!console.workflow());
    });
    assert_eq!(code, 0);
    assert_eq!(
        text,
        "A step\n::warning::x stays as it is\na.md:3: error: wrong\nplain\n"
    );
}

#[test]
fn an_annotation_is_taken_apart() {
    assert_eq!(
        console::annotation("::error title=parse,file=a.json,line=1::m"),
        Some(("error", "title=parse,file=a.json,line=1", "m"))
    );
    assert_eq!(
        console::annotation("::notice::m"),
        Some(("notice", "", "m"))
    );
    assert_eq!(console::annotation("::debug::m"), None);
    assert_eq!(console::annotation("::error file=a:b::m"), None);
    assert_eq!(console::annotation("plain"), None);
    assert_eq!(console::unescape("a%0Ab%0Dc%3Ad%2Ce%25"), "a\nb\rc:d,e%");
}
