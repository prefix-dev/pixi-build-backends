use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RattlerBuildBackendConfig {
    /// If set, internal state will be logged as files in that directory
    pub debug_dir: Option<PathBuf>,
    /// Extra input globs to include in addition to the default ones
    #[serde(default)]
    pub extra_input_globs: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::RattlerBuildBackendConfig;
    use serde_json::json;

    #[test]
    fn test_ensure_deseralize_from_empty() {
        let json_data = json!({});
        serde_json::from_value::<RattlerBuildBackendConfig>(json_data).unwrap();
    }
}
