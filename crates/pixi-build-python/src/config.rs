use indexmap::IndexMap;
use serde::Deserialize;
use std::{convert::identity, path::PathBuf};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PythonBackendConfig {
    /// True if the package should be build as a python noarch package. Defaults
    /// to `true`.
    #[serde(default)]
    pub noarch: Option<bool>,
    /// Environment Variables
    pub env: IndexMap<String, String>,
    /// If set, internal state will be logged as files in that directory
    pub debug_dir: Option<PathBuf>,
}

impl PythonBackendConfig {
    /// Whether to build a noarch package or a platform-specific package.
    pub fn noarch(&self) -> bool {
        self.noarch.map_or(true, identity)
    }
}
