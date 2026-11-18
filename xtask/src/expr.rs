//! The expressions of GitHub Actions, as far as the building blocks use them.
//!
//! A building block decides with them whether a step runs (`if:`) and what a
//! step is handed (`${{ … }}` in `env:`, `with:`, `working-directory:` and
//! `run:`). This module follows GitHub's rules for exactly that part of the
//! language: string literals, `true`, `false` and `null`, property paths, `!`,
//! `==`, `!=`, `&&`, `||`, parentheses and the four status functions. Anything
//! else is refused by name instead of being guessed, so a block that starts to
//! use more of the language fails here until the xtask learns it.

use std::cmp::Ordering;

use crate::Error;

/// A value an expression evaluates to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// Nothing: a property that is not there.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A string: every input, output and context value the blocks read.
    String(String),
}

impl Value {
    /// Whether GitHub counts the value as true in a condition.
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::String(value) => !value.is_empty(),
        }
    }

    /// The value as `${{ … }}` puts it into a string.
    #[must_use]
    pub fn to_text(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Bool(value) => value.to_string(),
            Self::String(value) => value.clone(),
        }
    }

    /// GitHub's `==`: strings compare without regard to case, and values of
    /// different types are compared as numbers.
    fn equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(left), Self::String(right)) => {
                left.to_lowercase() == right.to_lowercase()
            }
            _ => self.number().partial_cmp(&other.number()) == Some(Ordering::Equal),
        }
    }

    /// The value as GitHub turns it into a number for a comparison.
    fn number(&self) -> f64 {
        match self {
            Self::Null | Self::Bool(false) => 0.0,
            Self::Bool(true) => 1.0,
            Self::String(text) => {
                let text = text.trim();
                if text.is_empty() {
                    0.0
                } else if text.chars().all(|c| "0123456789+-.eE".contains(c)) {
                    text.parse().unwrap_or(f64::NAN)
                } else {
                    f64::NAN
                }
            }
        }
    }
}

/// What an expression can read.
pub trait Scope {
    /// The value at a property path such as `steps.files.outputs.count`; the
    /// first segment names the context. A context the scope does not know is
    /// an error; a property it does not have is [`Value::Null`], as on GitHub.
    ///
    /// # Errors
    ///
    /// When the context is unknown, or the path names no single value.
    fn lookup(&self, path: &[String]) -> Result<Value, Error>;

    /// Whether a step before this one failed.
    fn failed(&self) -> bool;
}

/// Evaluates one expression, the text between `${{` and `}}`.
///
/// # Errors
///
/// When the expression uses something this module does not evaluate, or reads
/// a context the scope does not know.
pub fn evaluate(source: &str, scope: &dyn Scope) -> Result<Value, Error> {
    parse(source)?.evaluate(scope)
}

/// Decides an `if:`. It may be written with or without `${{ }}` around it,
/// and a condition that calls no status function runs only when no step before
/// it failed, as on GitHub.
///
/// # Errors
///
/// As [`evaluate`].
pub fn condition(source: &str, scope: &dyn Scope) -> Result<bool, Error> {
    let trimmed = source.trim();
    let inner = trimmed
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .unwrap_or(trimmed);
    let expression = parse(inner)?;
    if !expression.asks_status() && scope.failed() {
        return Ok(false);
    }
    Ok(expression.evaluate(scope)?.is_truthy())
}

/// Fills in every `${{ … }}` of a text.
///
/// # Errors
///
/// When a `${{` is not closed, or as [`evaluate`].
pub fn interpolate(text: &str, scope: &dyn Scope) -> Result<String, Error> {
    let mut filled = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${{") {
        filled.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        let Some(end) = closing(after) else {
            return Err(Error::new(format!(
                "a `${{{{` without its `}}}}` in {text:?}"
            )));
        };
        filled.push_str(&evaluate(&after[..end], scope)?.to_text());
        rest = &after[end + 2..];
    }
    filled.push_str(rest);
    Ok(filled)
}

/// Where the `}}` that closes an expression stands, outside its strings.
fn closing(text: &str) -> Option<usize> {
    let mut quoted = false;
    for (index, c) in text.char_indices() {
        if c == '\'' {
            quoted = !quoted;
        } else if !quoted && text[index..].starts_with("}}") {
            return Some(index);
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Text(String),
    Name(String),
    Dot,
    Open,
    Close,
    Not,
    Equal,
    NotEqual,
    And,
    Or,
}

fn tokens(source: &str) -> Result<Vec<Token>, Error> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(&c) = chars.get(index) {
        let next = chars.get(index + 1).copied();
        let (token, width) = match (c, next) {
            (c, _) if c.is_whitespace() => {
                index += 1;
                continue;
            }
            ('\'', _) => {
                let (text, width) = string(&chars[index + 1..], source)?;
                (Token::Text(text), width + 1)
            }
            ('.', _) => (Token::Dot, 1),
            ('(', _) => (Token::Open, 1),
            (')', _) => (Token::Close, 1),
            ('!', Some('=')) => (Token::NotEqual, 2),
            ('!', _) => (Token::Not, 1),
            ('=', Some('=')) => (Token::Equal, 2),
            ('&', Some('&')) => (Token::And, 2),
            ('|', Some('|')) => (Token::Or, 2),
            (c, _) if c.is_ascii_alphabetic() || c == '_' => {
                let name: String = chars[index..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || **c == '_' || **c == '-')
                    .collect();
                let width = name.chars().count();
                (Token::Name(name), width)
            }
            (c, _) => {
                return Err(Error::new(format!(
                    "the xtask does not evaluate `{c}` in the expression {source:?}"
                )));
            }
        };
        found.push(token);
        index += width;
    }
    Ok(found)
}

/// A string literal after its opening quote: its text, and how many
/// characters it took including the closing quote. `''` stands for one quote.
fn string(chars: &[char], source: &str) -> Result<(String, usize), Error> {
    let mut text = String::new();
    let mut index = 0;
    loop {
        match (chars.get(index), chars.get(index + 1)) {
            (Some('\''), Some('\'')) => {
                text.push('\'');
                index += 2;
            }
            (Some('\''), _) => return Ok((text, index + 1)),
            (Some(c), _) => {
                text.push(*c);
                index += 1;
            }
            (None, _) => {
                return Err(Error::new(format!(
                    "a string without its closing quote in the expression {source:?}"
                )));
            }
        }
    }
}

#[derive(Debug)]
enum Expression {
    Literal(Value),
    Path(Vec<String>),
    Status(Status),
    Not(Box<Expression>),
    Equal(Box<Expression>, Box<Expression>, bool),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
}

#[derive(Clone, Copy, Debug)]
enum Status {
    Success,
    Failure,
    Cancelled,
    Always,
}

impl Expression {
    fn evaluate(&self, scope: &dyn Scope) -> Result<Value, Error> {
        Ok(match self {
            Self::Literal(value) => value.clone(),
            Self::Path(path) => scope.lookup(path)?,
            Self::Status(status) => Value::Bool(match status {
                Status::Success => !scope.failed(),
                Status::Failure => scope.failed(),
                // Nothing is ever cancelled here: the xtask runs to the end.
                Status::Cancelled => false,
                Status::Always => true,
            }),
            Self::Not(inner) => Value::Bool(!inner.evaluate(scope)?.is_truthy()),
            Self::Equal(left, right, negated) => {
                let left = left.evaluate(scope)?;
                Value::Bool(left.equals(&right.evaluate(scope)?) != *negated)
            }
            Self::And(left, right) => {
                let left = left.evaluate(scope)?;
                if left.is_truthy() {
                    right.evaluate(scope)?
                } else {
                    left
                }
            }
            Self::Or(left, right) => {
                let left = left.evaluate(scope)?;
                if left.is_truthy() {
                    left
                } else {
                    right.evaluate(scope)?
                }
            }
        })
    }

    /// Whether the expression calls a status function anywhere.
    fn asks_status(&self) -> bool {
        match self {
            Self::Literal(_) | Self::Path(_) => false,
            Self::Status(_) => true,
            Self::Not(inner) => inner.asks_status(),
            Self::Equal(left, right, _) | Self::And(left, right) | Self::Or(left, right) => {
                left.asks_status() || right.asks_status()
            }
        }
    }
}

fn parse(source: &str) -> Result<Expression, Error> {
    let mut parser = Parser {
        tokens: tokens(source)?,
        position: 0,
        source,
    };
    let expression = parser.or()?;
    match parser.tokens.get(parser.position) {
        None => Ok(expression),
        Some(token) => Err(parser.unexpected(Some(token))),
    }
}

struct Parser<'a> {
    tokens: Vec<Token>,
    position: usize,
    source: &'a str,
}

impl Parser<'_> {
    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        self.position += 1;
        token
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn unexpected(&self, token: Option<&Token>) -> Error {
        Error::new(match token {
            Some(token) => format!(
                "the expression {:?} cannot be read at {token:?}",
                self.source
            ),
            None => format!("the expression {:?} ends too early", self.source),
        })
    }

    fn or(&mut self) -> Result<Expression, Error> {
        let mut left = self.and()?;
        while self.peek() == Some(&Token::Or) {
            self.position += 1;
            left = Expression::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expression, Error> {
        let mut left = self.equality()?;
        while self.peek() == Some(&Token::And) {
            self.position += 1;
            left = Expression::And(Box::new(left), Box::new(self.equality()?));
        }
        Ok(left)
    }

    fn equality(&mut self) -> Result<Expression, Error> {
        let mut left = self.unary()?;
        loop {
            let negated = match self.peek() {
                Some(Token::Equal) => false,
                Some(Token::NotEqual) => true,
                _ => return Ok(left),
            };
            self.position += 1;
            left = Expression::Equal(Box::new(left), Box::new(self.unary()?), negated);
        }
    }

    fn unary(&mut self) -> Result<Expression, Error> {
        match self.next() {
            Some(Token::Not) => Ok(Expression::Not(Box::new(self.unary()?))),
            Some(Token::Open) => {
                let inner = self.or()?;
                match self.next() {
                    Some(Token::Close) => Ok(inner),
                    other => Err(self.unexpected(other.as_ref())),
                }
            }
            Some(Token::Text(text)) => Ok(Expression::Literal(Value::String(text))),
            Some(Token::Name(name)) => self.name(name),
            other => Err(self.unexpected(other.as_ref())),
        }
    }

    fn name(&mut self, name: String) -> Result<Expression, Error> {
        if self.peek() == Some(&Token::Open) {
            self.position += 1;
            let status = match name.to_ascii_lowercase().as_str() {
                "success" => Status::Success,
                "failure" => Status::Failure,
                "cancelled" => Status::Cancelled,
                "always" => Status::Always,
                _ => {
                    return Err(Error::new(format!(
                        "the xtask does not evaluate the function `{name}` in the expression {:?}",
                        self.source
                    )));
                }
            };
            return match self.next() {
                Some(Token::Close) => Ok(Expression::Status(status)),
                other => Err(self.unexpected(other.as_ref())),
            };
        }
        match name.as_str() {
            "true" => return Ok(Expression::Literal(Value::Bool(true))),
            "false" => return Ok(Expression::Literal(Value::Bool(false))),
            "null" => return Ok(Expression::Literal(Value::Null)),
            _ => {}
        }
        let mut path = vec![name];
        while self.peek() == Some(&Token::Dot) {
            self.position += 1;
            match self.next() {
                Some(Token::Name(name)) => path.push(name),
                other => return Err(self.unexpected(other.as_ref())),
            }
        }
        Ok(Expression::Path(path))
    }
}
