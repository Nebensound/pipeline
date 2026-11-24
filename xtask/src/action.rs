//! An action as its `action.yml` describes it, read as far as the xtask needs
//! it: the inputs it declares, which decide whether a block is handed the plan,
//! and the keys each of its steps carries, which the layout holds a block's
//! steps to.

use std::fs;
use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

use crate::Error;

/// One action.
#[derive(Debug)]
pub struct Action {
    /// The names of its inputs, in the order they are declared.
    pub inputs: Vec<String>,
    /// The keys each of its steps under `runs.steps` carries, step by step,
    /// each in the order written. None when it has no steps.
    pub steps: Vec<Vec<String>>,
}

impl Action {
    /// Whether it declares the input `name`. GitHub reads input names without
    /// regard to case, so `Plan` declares `plan` as well.
    #[must_use]
    pub fn takes(&self, name: &str) -> bool {
        self.inputs
            .iter()
            .any(|input| input.eq_ignore_ascii_case(name))
    }

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
    /// When the text is no YAML, is empty, its inputs are no mapping with a
    /// name for each, its `runs.steps` is no list, or one of its steps is no
    /// mapping with a name for each key.
    pub fn parse(text: &str, origin: &str) -> Result<Self, Error> {
        let documents = YamlLoader::load_from_str(text)
            .map_err(|error| Error::new(format!("{origin} is no YAML: {error}")))?;
        let Some(document) = documents.first() else {
            return Err(Error::new(format!("{origin} is empty")));
        };
        let inputs = keys(&document["inputs"], &format!("inputs in {origin}"))?;
        let steps = match &document["runs"]["steps"] {
            Yaml::BadValue | Yaml::Null => Vec::new(),
            Yaml::Array(steps) => steps
                .iter()
                .enumerate()
                .map(|(index, step)| keys(step, &format!("step {} in {origin}", index + 1)))
                .collect::<Result<_, _>>()?,
            _ => return Err(Error::new(format!("runs.steps in {origin} is no list"))),
        };
        Ok(Self { inputs, steps })
    }
}

/// The keys of a mapping as text, in the order written; none when it is not
/// there. `at` names it in a message.
fn keys(value: &Yaml, at: &str) -> Result<Vec<String>, Error> {
    match value {
        Yaml::BadValue | Yaml::Null => Ok(Vec::new()),
        Yaml::Hash(hash) => hash
            .keys()
            .map(|key| match key {
                Yaml::String(name) | Yaml::Real(name) => Ok(name.clone()),
                Yaml::Integer(number) => Ok(number.to_string()),
                Yaml::Boolean(flag) => Ok(flag.to_string()),
                _ => Err(Error::new(format!("{at} is no single value"))),
            })
            .collect(),
        _ => Err(Error::new(format!("{at} is no mapping"))),
    }
}
