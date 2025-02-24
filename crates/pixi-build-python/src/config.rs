use serde::Deserialize;
use std::convert::identity;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PythonBackendConfig {
    /// True if the package should be build as a python noarch package. Defaults
    /// to `true`.
    #[serde(default)]
    pub noarch: Option<bool>,
}

impl PythonBackendConfig {
    /// Whether to build a noarch package or a platform-specific package.
    pub fn noarch(&self) -> bool {
        self.noarch.map_or(true, identity)
    }
}
