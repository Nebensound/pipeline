//! The expressions, against GitHub's rules for the part the blocks use.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use xtask::Error;
use xtask::expr::{self, Scope, Value};

/// A scope with three values and a switch for "a step before failed".
struct Fake {
    failed: bool,
}

impl Scope for Fake {
    fn lookup(&self, path: &[String]) -> Result<Value, Error> {
        match path.join(".").as_str() {
            "a.text" => Ok(Value::String("x".to_owned())),
            "a.null" => Ok(Value::Null),
            "a.flag" => Ok(Value::Bool(true)),
            other => Err(Error::new(format!("unknown: {other}"))),
        }
    }

    fn failed(&self) -> bool {
        self.failed
    }
}

const PASSED: Fake = Fake { failed: false };
const FAILED: Fake = Fake { failed: true };

fn value(source: &str) -> Value {
    expr::evaluate(source, &PASSED).unwrap_or_else(|error| panic!("{source}: {error}"))
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

#[test]
fn literals_are_what_they_say() {
    assert_eq!(value("'a'"), text("a"));
    assert_eq!(value("'it''s'"), text("it's"));
    assert_eq!(value("''"), text(""));
    assert_eq!(value("true"), Value::Bool(true));
    assert_eq!(value("false"), Value::Bool(false));
    assert_eq!(value("null"), Value::Null);
    assert_eq!(value(" \t\n'spaced'\r\n"), text("spaced"));
}

#[test]
fn paths_read_the_scope() {
    assert_eq!(value("a.text"), text("x"));
    assert_eq!(value("a.null"), Value::Null);
    assert_eq!(value("a.flag"), Value::Bool(true));
    assert!(expr::evaluate("a.missing", &PASSED).is_err());
}

#[test]
fn equality_follows_github() {
    for (source, expected) in [
        ("'a' == 'A'", true),
        ("'a' != 'b'", true),
        ("'a' == 'b'", false),
        ("null == ''", true),
        ("null == '0'", true),
        ("null == 'x'", false),
        ("null == null", true),
        ("'1' == true", true),
        ("' 1 ' == true", true),
        ("'1e0' == true", true),
        ("'1x' == true", false),
        ("'1.2.3' == true", false),
        ("false == ''", true),
        ("true == false", false),
        ("a.null != '0'", false),
        ("a.text != '0'", true),
    ] {
        assert_eq!(value(source), Value::Bool(expected), "{source}");
    }
}

#[test]
fn logic_returns_the_operand_that_decided() {
    assert_eq!(value("'' && 'x'"), text(""));
    assert_eq!(value("'a' && 'b'"), text("b"));
    assert_eq!(value("'' || 'b'"), text("b"));
    assert_eq!(value("'a' || 'b'"), text("a"));
    assert_eq!(value("!''"), Value::Bool(true));
    assert_eq!(value("!'a'"), Value::Bool(false));
    assert_eq!(value("!null"), Value::Bool(true));
    assert_eq!(value("!!true"), Value::Bool(true));
    assert_eq!(value("!('a' == 'b')"), Value::Bool(true));
    assert_eq!(
        value("'a' == 'a' && 'b' == 'c' || 'd' != 'e'"),
        Value::Bool(true)
    );
}

#[test]
fn status_functions_read_whether_a_step_failed() {
    for (source, passed, failed) in [
        ("success()", true, false),
        ("failure()", false, true),
        ("cancelled()", false, false),
        ("always()", true, true),
        ("SUCCESS()", true, false),
    ] {
        assert_eq!(
            expr::evaluate(source, &PASSED).unwrap(),
            Value::Bool(passed),
            "{source}"
        );
        assert_eq!(
            expr::evaluate(source, &FAILED).unwrap(),
            Value::Bool(failed),
            "{source}"
        );
    }
}

#[test]
fn what_is_not_understood_is_refused() {
    for (source, message) in [
        ("1", "does not evaluate `1`"),
        ("'a' < 'b'", "does not evaluate `<`"),
        ("'a' & 'b'", "does not evaluate `&`"),
        ("'a' | 'b'", "does not evaluate `|`"),
        ("'a' = 'b'", "does not evaluate `=`"),
        ("'open", "without its closing quote"),
        ("contains()", "the function `contains`"),
        ("'a', 'b'", "does not evaluate `,`"),
        ("success(", "ends too early"),
        ("success('a')", "cannot be read at"),
        ("(", "ends too early"),
        ("('a'", "ends too early"),
        ("('a' 'b')", "cannot be read at"),
        ("a.", "ends too early"),
        ("a.'x'", "cannot be read at"),
        ("'a' 'b'", "cannot be read at"),
        ("== 'a'", "cannot be read at"),
        ("'a' ||", "ends too early"),
        ("'a' &&", "ends too early"),
        ("'a' ==", "ends too early"),
        ("!", "ends too early"),
    ] {
        let error = expr::evaluate(source, &PASSED).expect_err(source);
        assert!(error.to_string().contains(message), "{source}: {error}");
    }
}

#[test]
fn an_error_inside_any_operator_reaches_the_caller() {
    for source in [
        "!x.y",
        "x.y == 'a'",
        "'a' == x.y",
        "x.y && true",
        "'a' && x.y",
        "x.y || true",
        "'' || x.y",
    ] {
        assert!(expr::evaluate(source, &PASSED).is_err(), "{source}");
    }
}

#[test]
fn a_condition_runs_after_a_failure_only_when_it_asks_for_status() {
    assert!(expr::condition("${{ 'a' == 'a' }}", &PASSED).unwrap());
    assert!(expr::condition("'a' == 'a'", &PASSED).unwrap());
    assert!(!expr::condition("'a' == 'b'", &PASSED).unwrap());
    assert!(!expr::condition("'a' == 'a'", &FAILED).unwrap());
    assert!(expr::condition("always()", &FAILED).unwrap());
    assert!(expr::condition("${{ !cancelled() && 'a' == 'a' }}", &FAILED).unwrap());
    assert!(expr::condition("!(failure())", &PASSED).unwrap());
    assert!(expr::condition("failure() || 'a' == 'b'", &FAILED).unwrap());
    // After a failure, a condition without a status function is not even
    // evaluated, as on GitHub.
    assert!(!expr::condition("x.y", &FAILED).unwrap());
    assert!(expr::condition("x.y", &PASSED).is_err());
    assert!(expr::condition("1", &PASSED).is_err());
}

#[test]
fn interpolation_fills_in_every_expression() {
    assert_eq!(
        expr::interpolate("a ${{ a.text }} b ${{a.flag}} c ${{ a.null }}.", &PASSED).unwrap(),
        "a x b true c ."
    );
    assert_eq!(expr::interpolate("plain", &PASSED).unwrap(), "plain");
    assert_eq!(
        expr::interpolate("${{ '}}' }}", &PASSED).unwrap(),
        "}}",
        "a `}}` inside a string does not close the expression"
    );
    assert_eq!(expr::interpolate("${{ false }}", &PASSED).unwrap(), "false");
    let open = expr::interpolate("${{ a.text", &PASSED).unwrap_err();
    assert!(open.to_string().contains("without its"), "{open}");
    assert!(expr::interpolate("${{ x.y }}", &PASSED).is_err());
}
