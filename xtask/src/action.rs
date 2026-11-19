//! An action as its `action.yml` describes it, read as far as the xtask runs
//! it.
//!
//! A step may carry only the keys the xtask knows how to run. Any other key
//! (`continue-on-error`, `timeout-minutes`, …) changes what the step does on
//! GitHub, so it is refused instead of being dropped.

use std::fs;
use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

use crate::Error;

/// The keys a step may carry.
const STEP_KEYS: &[&str] = &[
    "name",
    "id",
    "if",
    "run",
    "shell",
    "env",
    "working-directory",
    "uses",
    "with",
];

/// One action.
#[derive(Debug)]
pub struct Action {
    /// Its inputs, in the order they are declared.
    pub inputs: Vec<Input>,
    /// Its outputs: each name with the expression of its value.
    pub outputs: Vec<(String, String)>,
    /// What it runs.
    pub runs: Runs,
}

/// One input of an action.
#[derive(Debug)]
pub struct Input {
    /// Its name.
    pub name: String,
    /// What it holds when the caller leaves it out, if anything.
    pub default: Option<String>,
}

/// What an action runs.
#[derive(Debug)]
pub enum Runs {
    /// Steps, one after another: a composite action.
    Composite(Vec<Step>),
    /// Something else, named by `runs.using` (`node24`, `docker`, …).
    Other(String),
}

/// One step of a composite action.
#[derive(Debug)]
pub struct Step {
    /// Its `name`.
    pub name: Option<String>,
    /// Its `id`, by which later steps read its outputs.
    pub id: Option<String>,
    /// Its `if`.
    pub condition: Option<String>,
    /// Its `env`, in the order written.
    pub env: Vec<(String, String)>,
    /// Its `working-directory`.
    pub working_directory: Option<String>,
    /// What it does.
    pub kind: Kind,
}

/// What a step does.
#[derive(Debug)]
pub enum Kind {
    /// A script.
    Run {
        /// The script.
        script: String,
        /// The shell it is written for.
        shell: Option<String>,
    },
    /// Another action.
    Uses {
        /// The action, as written after `uses:`.
        action: String,
        /// Its inputs, in the order written.
        with: Vec<(String, String)>,
    },
}

impl Step {
    /// How the step is shown: its name, else what it runs.
    #[must_use]
    pub fn title(&self) -> String {
        match (&self.name, &self.kind) {
            (Some(name), _) => name.clone(),
            (None, Kind::Uses { action, .. }) => action.clone(),
            (None, Kind::Run { script, .. }) => {
                script.lines().next().unwrap_or_default().trim().to_owned()
            }
        }
    }
}

impl Action {
    /// Reads the action in a directory, from `action.yml` or `action.yaml`.
    ///
    /// # Errors
    ///
    /// When there is neither file, or it is not an action the xtask can read.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let yml = dir.join("action.yml");
        let path = if yml.is_file() {
            yml
        } else {
            dir.join("action.yaml")
        };
        let text = fs::read_to_string(&path)
            .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
        Self::parse(&text, &path.display().to_string())
    }

    /// Reads an action from the text of its `action.yml`; `origin` names it in
    /// a message.
    ///
    /// # Errors
    ///
    /// When the text is no YAML, or not an action the xtask can read.
    pub fn parse(text: &str, origin: &str) -> Result<Self, Error> {
        let documents = YamlLoader::load_from_str(text)
            .map_err(|error| Error::new(format!("{origin} is no YAML: {error}")))?;
        let Some(document) = documents.first() else {
            return Err(Error::new(format!("{origin} is empty")));
        };
        let at = |what: &str| format!("{what} in {origin}");

        let mut inputs = Vec::new();
        for (name, input) in entries(&document["inputs"], &at("inputs"))? {
            let default = match &input["default"] {
                Yaml::BadValue => None,
                value => Some(scalar(
                    value,
                    &at(&format!("the default of input `{name}`")),
                )?),
            };
            inputs.push(Input { name, default });
        }

        let mut outputs = Vec::new();
        for (name, output) in entries(&document["outputs"], &at("outputs"))? {
            let value = scalar(
                &output["value"],
                &at(&format!("the value of output `{name}`")),
            )?;
            outputs.push((name, value));
        }

        let runs = &document["runs"];
        let using = scalar(&runs["using"], &at("runs.using"))?;
        if using != "composite" {
            return Ok(Self {
                inputs,
                outputs,
                runs: Runs::Other(using),
            });
        }
        let Some(steps) = runs["steps"].as_vec() else {
            return Err(Error::new(format!("{} is no list", at("runs.steps"))));
        };
        let steps = steps
            .iter()
            .enumerate()
            .map(|(index, step)| self::step(step, &at(&format!("step {}", index + 1))))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            inputs,
            outputs,
            runs: Runs::Composite(steps),
        })
    }
}

fn step(step: &Yaml, at: &str) -> Result<Step, Error> {
    let keys = entries(step, at)?;
    if keys.is_empty() {
        return Err(Error::new(format!("{at} is empty")));
    }
    if let Some((key, _)) = keys
        .iter()
        .find(|(key, _)| !STEP_KEYS.contains(&key.as_str()))
    {
        return Err(Error::new(format!(
            "{at} sets `{key}`, which the xtask does not run"
        )));
    }
    let optional = |key: &str| -> Result<Option<String>, Error> {
        match &step[key] {
            Yaml::BadValue => Ok(None),
            value => scalar(value, &format!("`{key}` of {at}")).map(Some),
        }
    };
    let pairs = |key: &str| -> Result<Vec<(String, String)>, Error> {
        let where_ = format!("`{key}` of {at}");
        entries(&step[key], &where_)?
            .into_iter()
            .map(|(name, value)| Ok((name, scalar(value, &where_)?)))
            .collect()
    };
    let kind = match (optional("run")?, optional("uses")?) {
        (Some(script), None) => Kind::Run {
            script,
            shell: optional("shell")?,
        },
        (None, Some(action)) => Kind::Uses {
            action,
            with: pairs("with")?,
        },
        _ => {
            return Err(Error::new(format!(
                "{at} needs exactly one of `run` and `uses`"
            )));
        }
    };
    Ok(Step {
        name: optional("name")?,
        id: optional("id")?,
        condition: optional("if")?,
        env: pairs("env")?,
        working_directory: optional("working-directory")?,
        kind,
    })
}

/// The entries of a mapping, keys as text; nothing when it is not there.
fn entries<'a>(value: &'a Yaml, at: &str) -> Result<Vec<(String, &'a Yaml)>, Error> {
    match value {
        Yaml::BadValue | Yaml::Null => Ok(Vec::new()),
        Yaml::Hash(hash) => hash
            .iter()
            .map(|(key, value)| Ok((scalar(key, at)?, value)))
            .collect(),
        _ => Err(Error::new(format!("{at} is no mapping"))),
    }
}

/// A scalar as text, the way GitHub hands a YAML value to an action.
fn scalar(value: &Yaml, at: &str) -> Result<String, Error> {
    match value {
        Yaml::String(text) | Yaml::Real(text) => Ok(text.clone()),
        Yaml::Integer(number) => Ok(number.to_string()),
        Yaml::Boolean(flag) => Ok(flag.to_string()),
        Yaml::Null => Ok(String::new()),
        _ => Err(Error::new(format!("{at} is no single value"))),
    }
}
