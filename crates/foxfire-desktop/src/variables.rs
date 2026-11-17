use regex::{Captures, Regex};
use std::env;
use std::sync::LazyLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VarError {
    #[error("Environment variable '{0}' not found")]
    NotFound(String),
}

/// Values for `$VAR` / `${VAR}` references in agent settings: the agent's workspace, then the process environment.
pub(crate) struct Vars {
    workspace: String,
}

impl Vars {
    pub(crate) fn new(workspace: impl Into<String>) -> Self {
        Self { workspace: workspace.into() }
    }

    /// Expands every reference in `template`; `$$` escapes to a literal `$`.
    pub(crate) fn expand(&self, template: &str) -> Result<String, VarError> {
        let mut missing = None;
        let expanded = REFERENCE.replace_all(template, |caps: &Captures| {
            let Some(name) = caps.get(1).or_else(|| caps.get(2)).map(|name| name.as_str()) else {
                return "$".to_owned();
            };
            self.get(name).unwrap_or_else(|| {
                missing.get_or_insert_with(|| name.to_owned());
                caps[0].to_owned()
            })
        });
        missing.map_or_else(|| Ok(expanded.into_owned()), |name| Err(VarError::NotFound(name)))
    }

    fn get(&self, name: &str) -> Option<String> {
        if name == "WORKSPACE" { Some(self.workspace.clone()) } else { env::var(name).ok() }
    }
}

static REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$(?:\$|\{([^}]+)\}|([A-Za-z_][A-Za-z0-9_]*))").expect("valid regex"));
