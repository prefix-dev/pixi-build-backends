use indexmap::IndexMap;

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RustBackendConfig {
    /// Extra args to pass for cargo
    pub extra_args: Vec<String>,

    /// Environment Variables
    pub env: IndexMap<String, String>,
}
