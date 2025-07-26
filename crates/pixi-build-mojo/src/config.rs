use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use pixi_build_backend::generated_recipe::BackendConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MojoBackendConfig {
    /// Environment Variables
    #[serde(default)]
    pub env: IndexMap<String, String>,

    /// Directory that will be created to place output artifacts.
    ///
    /// This is releative to the manifest dir.
    pub dist_dir: Option<PathBuf>,

    /// Dir that can be specified for outputting pixi debug state.
    pub debug_dir: Option<PathBuf>,

    /// Extra input globs to include in addition to the default ones.
    #[serde(default)]
    pub extra_input_globs: Vec<String>,

    /// Binary executables to produce.
    pub bins: Option<Vec<MojoBinConfig>>,

    /// Packages to produce.
    pub pkg: Option<MojoPkgConfig>,
}

impl BackendConfig for MojoBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }
}

/// Config object for a Mojo binary.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MojoBinConfig {
    /// Name of the binary.
    pub name: String,
    /// Path to file that has the `main` method.
    pub path: String,
    /// Extra args to pass to the compiler.
    #[serde(default, rename(serialize = "extra_args"))]
    pub extra_args: Option<Vec<String>>,
}

/// Config object for a Mojo package.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct MojoPkgConfig {
    /// Name to give the mojo package (.mojopkg suffix will be added).
    pub name: String,
    /// Path to the directory that constitutes the package.
    pub path: String,
    /// Extra args to pass to the compiler.
    #[serde(default, rename(serialize = "extra_args"))]
    pub extra_args: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::MojoBackendConfig;

    #[test]
    fn test_ensure_deseralize_from_empty() {
        let json_data = json!({});
        serde_json::from_value::<MojoBackendConfig>(json_data).unwrap();
    }
}
