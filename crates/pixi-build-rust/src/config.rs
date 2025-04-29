use indexmap::IndexMap;

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RustBackendConfig {
    /// Extra args to pass for cargo
    #[serde(default)]
    pub extra_args: Vec<String>,

    /// Environment Variables
    #[serde(default)]
    pub env: IndexMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::RustBackendConfig;
    use serde_json::json;

    #[test]
    fn test_ensure_deseralize_from_empty() {
        let json_data = json!({});
        serde_json::from_value::<RustBackendConfig>(json_data).unwrap();
    }
}
