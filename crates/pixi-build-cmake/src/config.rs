use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CMakeBackendConfig {
    /// Extra args for CMake invocation
    pub extra_args: Vec<String>,
    /// Environment Variables
    pub env_vars: IndexMap<String, String>,
}
