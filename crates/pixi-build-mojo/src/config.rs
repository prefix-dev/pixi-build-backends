use indexmap::IndexMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MojoBackendConfig {
    /// Extra args to pass for mojo
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Environment Variables
    #[serde(default)]
    pub env: IndexMap<String, String>,
    /// If set, internal state will be logged as files in that directory
    pub debug_dir: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::MojoBackendConfig;
    use serde_json::json;

    #[test]
    fn test_ensure_deseralize_from_empty() {
        let json_data = json!({});
        serde_json::from_value::<MojoBackendConfig>(json_data).unwrap();
    }
}
