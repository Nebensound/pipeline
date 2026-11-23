//! An action as its `action.yml` describes it, read as far as the xtask needs
//! it: the inputs it declares, which decide whether a block is handed the plan.

use std::fs;
use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

use crate::Error;

/// One action.
#[derive(Debug)]
pub struct Action {
    /// The names of its inputs, in the order they are declared.
    pub inputs: Vec<String>,
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
    /// When the text is no YAML, is empty, or its inputs are no mapping with
    /// a name for each.
    pub fn parse(text: &str, origin: &str) -> Result<Self, Error> {
        let documents = YamlLoader::load_from_str(text)
            .map_err(|error| Error::new(format!("{origin} is no YAML: {error}")))?;
        let Some(document) = documents.first() else {
            return Err(Error::new(format!("{origin} is empty")));
        };
        let at = format!("inputs in {origin}");
        let inputs = match &document["inputs"] {
            Yaml::BadValue | Yaml::Null => Vec::new(),
            Yaml::Hash(hash) => hash
                .keys()
                .map(|key| match key {
                    Yaml::String(name) | Yaml::Real(name) => Ok(name.clone()),
                    Yaml::Integer(number) => Ok(number.to_string()),
                    Yaml::Boolean(flag) => Ok(flag.to_string()),
                    _ => Err(Error::new(format!("{at} is no single value"))),
                })
                .collect::<Result<_, _>>()?,
            _ => return Err(Error::new(format!("{at} is no mapping"))),
        };
        Ok(Self { inputs })
    }
}
