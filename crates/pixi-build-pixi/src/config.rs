use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use pixi_build_backend::generated_recipe::BackendConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PixiBackendConfig {
    /// Environment Variables
    #[serde(default)]
    pub env: IndexMap<String, String>,
    /// If set, internal state will be logged as files in that directory
    pub debug_dir: Option<PathBuf>,
    /// Extra input globs to include in addition to the default ones
    #[serde(default)]
    pub extra_input_globs: Vec<String>,
    /// Name of the build task in pixi.toml (defaults to "build")
    #[serde(default = "default_build_task")]
    pub build_task: String,
}

fn default_build_task() -> String {
    "build".to_string()
}

impl BackendConfig for PixiBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    /// Merge this configuration with a target-specific configuration.
    /// Target-specific values override base values using the following rules:
    /// - extra_args: Platform-specific completely replaces base
    /// - env: Platform env vars override base, others merge
    /// - debug_dir: Not allowed to have target specific value
    /// - extra_input_globs: Platform-specific completely replaces base
    /// - compilers: Platform-specific completely replaces base
    fn merge_with_target_config(&self, target_config: &Self) -> miette::Result<Self> {
        if target_config.debug_dir.is_some() {
            miette::bail!("`debug_dir` cannot have a target specific value");
        }

        Ok(Self {
            env: {
                let mut merged_env = self.env.clone();
                merged_env.extend(target_config.env.clone());
                merged_env
            },
            debug_dir: self.debug_dir.clone(),
            extra_input_globs: if target_config.extra_input_globs.is_empty() {
                self.extra_input_globs.clone()
            } else {
                target_config.extra_input_globs.clone()
            },
            build_task: if target_config.build_task == default_build_task() {
                self.build_task.clone()
            } else {
                target_config.build_task.clone()
            },
        })
    }
}
