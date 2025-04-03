use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RattlerBuildBackendConfig {
    /// If set, internal state will be logged as files in that directory
    pub debug_dir: Option<PathBuf>,
}
