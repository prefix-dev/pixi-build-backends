use pixi_build_backend::generated_recipe::BackendConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RattlerBuildBackendConfig {
    /// Deprecated. Setting this has no effect; debug data is always written to
    /// the `debug` subdirectory of the work directory.
    #[serde(alias = "debug_dir")]
    pub debug_dir: Option<PathBuf>,
    /// Extra input globs to include in addition to the default ones
    #[serde(default)]
    pub extra_input_globs: Vec<String>,
    /// Extra arguments to pass to rattler-build.
    /// Supported arguments include:
    /// - `--experimental`: Enable experimental features (e.g., cache support)
    #[serde(default)]
    pub extra_args: Vec<String>,
}

impl RattlerBuildBackendConfig {
    /// Returns `true` if `--experimental` is present in `extra_args`.
    pub fn experimental(&self) -> bool {
        self.extra_args.iter().any(|arg| arg == "--experimental")
    }
}

impl BackendConfig for RattlerBuildBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    /// Merge this configuration with a target-specific configuration.
    /// Target-specific values override base values using the following rules:
    /// - debug_dir: Not allowed to have target specific value
    /// - extra_input_globs: Platform-specific completely replaces base
    /// - extra_args: Platform-specific completely replaces base
    fn merge_with_target_config(&self, target_config: &Self) -> miette::Result<Self> {
        if target_config.debug_dir.is_some() {
            miette::bail!("`debug_dir` cannot have a target specific value");
        }

        Ok(Self {
            debug_dir: self.debug_dir.clone(),
            extra_input_globs: if target_config.extra_input_globs.is_empty() {
                self.extra_input_globs.clone()
            } else {
                target_config.extra_input_globs.clone()
            },
            extra_args: if target_config.extra_args.is_empty() {
                self.extra_args.clone()
            } else {
                target_config.extra_args.clone()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::RattlerBuildBackendConfig;
    use pixi_build_backend::generated_recipe::BackendConfig;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn test_ensure_deseralize_from_empty() {
        let json_data = json!({});
        serde_json::from_value::<RattlerBuildBackendConfig>(json_data).unwrap();
    }

    #[test]
    fn test_merge_with_target_config() {
        let base_config = RattlerBuildBackendConfig {
            debug_dir: Some(PathBuf::from("/base/debug")),
            extra_input_globs: vec!["*.base".to_string()],
            extra_args: vec!["--base-arg".to_string()],
        };

        let target_config = RattlerBuildBackendConfig {
            debug_dir: None,
            extra_input_globs: vec!["*.target".to_string()],
            extra_args: vec!["--target-arg".to_string()],
        };

        let merged = base_config
            .merge_with_target_config(&target_config)
            .unwrap();

        // debug_dir should use base value
        assert_eq!(merged.debug_dir, Some(PathBuf::from("/base/debug")));

        // extra_input_globs should be completely overridden
        assert_eq!(merged.extra_input_globs, vec!["*.target".to_string()]);

        // extra_args should be completely overridden
        assert_eq!(merged.extra_args, vec!["--target-arg".to_string()]);
    }

    #[test]
    fn test_merge_with_empty_target_config() {
        let base_config = RattlerBuildBackendConfig {
            debug_dir: Some(PathBuf::from("/base/debug")),
            extra_input_globs: vec!["*.base".to_string()],
            extra_args: vec!["--experimental".to_string()],
        };

        let empty_target_config = RattlerBuildBackendConfig::default();

        let merged = base_config
            .merge_with_target_config(&empty_target_config)
            .unwrap();

        // Should keep base values when target is empty
        assert_eq!(merged.debug_dir, Some(PathBuf::from("/base/debug")));
        assert_eq!(merged.extra_input_globs, vec!["*.base".to_string()]);
        assert_eq!(merged.extra_args, vec!["--experimental".to_string()]);
    }

    #[test]
    fn test_merge_target_debug_dir_error() {
        let base_config = RattlerBuildBackendConfig {
            debug_dir: Some(PathBuf::from("/base/debug")),
            ..Default::default()
        };

        let target_config = RattlerBuildBackendConfig {
            debug_dir: Some(PathBuf::from("/target/debug")),
            ..Default::default()
        };

        let result = base_config.merge_with_target_config(&target_config);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("`debug_dir` cannot have a target specific value"));
    }

    #[test]
    fn test_experimental_flag() {
        // Test that experimental() returns true when --experimental is present
        let config_with_experimental = RattlerBuildBackendConfig {
            extra_args: vec!["--experimental".to_string()],
            ..Default::default()
        };
        assert!(config_with_experimental.experimental());

        // Test that experimental() returns true when --experimental is among other args
        let config_with_multiple_args = RattlerBuildBackendConfig {
            extra_args: vec![
                "--some-arg".to_string(),
                "--experimental".to_string(),
                "--another-arg".to_string(),
            ],
            ..Default::default()
        };
        assert!(config_with_multiple_args.experimental());

        // Test that experimental() returns false when --experimental is not present
        let config_without_experimental = RattlerBuildBackendConfig {
            extra_args: vec!["--other-flag".to_string()],
            ..Default::default()
        };
        assert!(!config_without_experimental.experimental());

        // Test that experimental() returns false when extra_args is empty
        let config_empty = RattlerBuildBackendConfig::default();
        assert!(!config_empty.experimental());
    }
}
