use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CMakeBackendConfig {
    /// Extra args for CMake invocation
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Environment Variables
    #[serde(default)]
    pub env: IndexMap<String, String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::CMakeBackendConfig;

    #[test]
    fn test_ensure_deseralize_from_empty() {
        let json_data = json!({});
        serde_json::from_value::<CMakeBackendConfig>(json_data).unwrap();
    }
}
