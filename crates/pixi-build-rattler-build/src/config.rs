use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RattlerBuildBackendConfig {
    pub debug_dir: Option<PathBuf>,
}
