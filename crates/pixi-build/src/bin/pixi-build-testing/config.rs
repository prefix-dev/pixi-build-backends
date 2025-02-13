use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TestingBackendConfig {
    pub data_dir: PathBuf,
}
