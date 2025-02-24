use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CMakeBackendConfig {
    /// Extra args for CMake invocation
    pub extra_args: Vec<String>,
}
