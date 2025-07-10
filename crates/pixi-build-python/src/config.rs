use indexmap::IndexMap;
use pixi_build_backend::generated_recipe::BackendConfig;
use serde::Deserialize;
use std::{
    convert::identity,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PythonBackendConfig {
    /// True if the package should be build as a python noarch package. Defaults
    /// to `true`.
    #[serde(default)]
    pub noarch: Option<bool>,
    /// Environment Variables
    #[serde(default)]
    pub env: IndexMap<String, String>,
    /// If set, internal state will be logged as files in that directory
    pub debug_dir: Option<PathBuf>,
    /// Extra input globs to include in addition to the default ones
    #[serde(default)]
    pub extra_input_globs: Vec<String>,
}

impl PythonBackendConfig {
    /// Whether to build a noarch package or a platform-specific package.
    pub fn noarch(&self) -> bool {
        self.noarch.is_none_or(identity)
    }
}

impl BackendConfig for PythonBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::PythonBackendConfig;
    use serde_json::json;

    #[test]
    fn test_ensure_deseralize_from_empty() {
        let json_data = json!({});
        serde_json::from_value::<PythonBackendConfig>(json_data).unwrap();
    }
}
