//! The JSON Schema of `.nebensound/repo.toml`, derived from the types that
//! read it.
//!
//! It exists for authors, not for CI: phase 0 reads the file with these same
//! types and is the judge. The schema gives an editor and `taplo lint` the
//! shape — completion, and an unknown key marked where it is written.
//!
//! Most of it is derived. What a type reads by hand — a name, a path, a
//! pattern, the two forms of a pipeline — is described here, beside the rule
//! it mirrors. Where a rule cannot be said in a schema, the schema says less
//! than the parser and never more: it may let a file through that phase 0
//! then rejects, but it does not reject one that phase 0 accepts.

use std::borrow::Cow;

use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

use crate::model::{Declaration, ExplicitPipeline, Pipeline, Publish};
use crate::names::{
    BuildName, CheckName, ComponentName, EcosystemName, PresetName, Reason, TargetName,
    VersionWriterName,
};
use crate::path::RelativePath;
use crate::pattern::Pattern;

/// The schema of `.nebensound/repo.toml`, as JSON Schema draft 7.
///
/// Draft 7 because it is the draft the schemas `check-toml` already assigns
/// are written in, and the one Taplo reads. An optional field is the type it
/// holds and not "that type or null": TOML has no null.
#[must_use]
pub fn json_schema() -> Schema {
    let mut schema = SchemaSettings::draft07()
        .with_transform(RecursiveTransform(without_null))
        .into_generator()
        .into_root_schema_for::<Declaration>();
    schema.insert(
        "title".to_owned(),
        "Nebensound repository declaration".into(),
    );
    schema
}

/// Takes "or null" out of every choice: schemars writes an optional field as
/// its type or null, and TOML has no null to write.
fn without_null(schema: &mut Schema) {
    if let Some(choices) = schema
        .get_mut("anyOf")
        .and_then(|value| value.as_array_mut())
    {
        choices.retain(|choice| choice.get("type").and_then(|kind| kind.as_str()) != Some("null"));
    }
}

/// A name as `names::parse` accepts it: lowercase letters, digits, `-` and
/// `.`, starting and ending on a letter or digit, never `..`.
///
/// Every `.` is followed by a character that is no `.`; that is how the
/// pattern refuses `..` without a lookahead.
const NAME: &str = r"[a-z0-9](?:(?:\.?[a-z0-9-])*\.?[a-z0-9])?";

/// One segment of a path as `RelativePath::new` accepts it: not empty, not
/// `.` or `..`, and neither `/` nor a backslash in it.
const SEGMENT: &str = r"(?:[^/\\.][^/\\]*|\.[^/\\.][^/\\]*|\.\.[^/\\]+)";

fn name_schema(kind: &str) -> Schema {
    json_schema!({
        "type": "string",
        "description": format!(
            "A valid {kind}: lowercase letters, digits, '-' and '.', starting and ending on a letter \
             or digit, without '..'."
        ),
        "pattern": format!("^{NAME}$"),
    })
}

fn path_pattern() -> String {
    format!(r"^(?:\.|{SEGMENT}(?:/{SEGMENT})*/?)$")
}

macro_rules! name_schema {
    ($($name:ident => $kind:literal),* $(,)?) => {
        $(
            impl JsonSchema for $name {
                fn schema_name() -> Cow<'static, str> {
                    stringify!($name).into()
                }

                fn json_schema(_: &mut SchemaGenerator) -> Schema {
                    name_schema($kind)
                }
            }
        )*
    };
}

name_schema!(
    ComponentName => "component name",
    PresetName => "preset name",
    CheckName => "check name",
    BuildName => "build name",
    TargetName => "target name",
    VersionWriterName => "version writer name",
    EcosystemName => "ecosystem name",
);

impl JsonSchema for Reason {
    fn schema_name() -> Cow<'static, str> {
        "Reason".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "Why, in words. It must say something: a blank reason is an error.",
            "pattern": r"\S",
        })
    }
}

impl JsonSchema for RelativePath {
    fn schema_name() -> Cow<'static, str> {
        "RelativePath".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "A path inside the repository, separated by '/': '.' for the \
                            repository itself, never absolute, no '.' or '..' segment.",
            "pattern": path_pattern(),
        })
    }
}

impl JsonSchema for Pattern {
    fn schema_name() -> Cow<'static, str> {
        "Pattern".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "A path inside the repository, a type ('*.png') or a name anywhere \
                            ('**/LICENSE').",
            "anyOf": [
                { "pattern": format!(r"^\*\.{NAME}$") },
                { "pattern": r"^\*\*/[^/]+$" },
                { "pattern": path_pattern() },
            ],
        })
    }
}

impl JsonSchema for Publish {
    fn schema_name() -> Cow<'static, str> {
        "Publish".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Where the artifact goes: `false` for nowhere, said out loud, or a \
                            list of targets from the central register.",
            "oneOf": [
                { "const": false },
                { "type": "array", "items": generator.subschema_for::<TargetName>() },
            ],
        })
    }
}

impl JsonSchema for Pipeline {
    fn schema_name() -> Cow<'static, str> {
        "Pipeline".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "What runs for a unit: a preset name, `{ repo = true }` for what \
                            `[repo.component]` offers, or the table written out.",
            "oneOf": [
                generator.subschema_for::<PresetName>(),
                {
                    "type": "object",
                    "properties": { "repo": { "const": true } },
                    "required": ["repo"],
                    "additionalProperties": false,
                },
                generator.subschema_for::<ExplicitPipeline>(),
            ],
        })
    }
}
