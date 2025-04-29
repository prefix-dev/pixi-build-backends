use indexmap::IndexMap;
use serde::Deserialize;
use std::convert::identity;

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
}

impl PythonBackendConfig {
    /// Whether to build a noarch package or a platform-specific package.
    pub fn noarch(&self) -> bool {
        self.noarch.map_or(true, identity)
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
