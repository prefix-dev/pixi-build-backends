use miette::IntoDiagnostic;
use serde::Deserialize;
use std::convert::identity;
use std::path::Path;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PythonBackendConfig {
    /// True if the package should be build as a python noarch package. Defaults
    /// to `true`.
    #[serde(default)]
    pub noarch: Option<bool>,
}

impl PythonBackendConfig {
    /// Whether to build a noarch package or a platform-specific package.
    pub fn noarch(&self) -> bool {
        self.noarch.map_or(true, identity)
    }

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
            pixi_build_python: PythonBackendConfig,
        }

        let manifest = fs_err::read_to_string(path).into_diagnostic()?;
        let document: Manifest = toml_edit::de::from_str(&manifest).into_diagnostic()?;
        Ok(document.tool.pixi_build_python)
    }
}
