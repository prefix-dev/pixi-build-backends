use miette::IntoDiagnostic;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CMakeBackendConfig {}

impl CMakeBackendConfig {
    /// Parse the configuration from a manifest file.
    pub fn from_path(path: &Path) -> miette::Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case")]
        struct Manifest {
            #[serde(default)]
            tool: Tool,
        }

        #[derive(Default, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        struct Tool {
            #[serde(default)]
            pixi_build_python: CMakeBackendConfig,
        }

        let manifest = fs_err::read_to_string(path).into_diagnostic()?;
        let document: Manifest = toml_edit::de::from_str(&manifest).into_diagnostic()?;
        Ok(document.tool.pixi_build_python)
    }
}
