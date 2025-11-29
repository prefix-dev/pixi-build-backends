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
    /// Enable experimental features in rattler-build (e.g., cache support for multi-output recipes)
    #[serde(default)]
    pub experimental: bool,
}

impl BackendConfig for RattlerBuildBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    /// Merge this configuration with a target-specific configuration.
    /// Target-specific values override base values using the following rules:
    /// - debug_dir: Not allowed to have target specific value
    /// - extra_input_globs: Platform-specific completely replaces base
    /// - experimental: Enabled if either base or target has it enabled (OR logic)
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
            // Enable experimental if either base or target has it enabled
            experimental: self.experimental || target_config.experimental,
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
            experimental: false,
        };

        let target_config = RattlerBuildBackendConfig {
            debug_dir: None,
            extra_input_globs: vec!["*.target".to_string()],
            experimental: false,
        };

        let merged = base_config
            .merge_with_target_config(&target_config)
            .unwrap();

        // debug_dir should use base value
        assert_eq!(merged.debug_dir, Some(PathBuf::from("/base/debug")));

        // extra_input_globs should be completely overridden
        assert_eq!(merged.extra_input_globs, vec!["*.target".to_string()]);

        // experimental should be false when both are false
        assert!(!merged.experimental);
    }

    #[test]
    fn test_merge_with_empty_target_config() {
        let base_config = RattlerBuildBackendConfig {
            debug_dir: Some(PathBuf::from("/base/debug")),
            extra_input_globs: vec!["*.base".to_string()],
            experimental: true,
        };

        let empty_target_config = RattlerBuildBackendConfig::default();

        let merged = base_config
            .merge_with_target_config(&empty_target_config)
            .unwrap();

        // Should keep base values when target is empty
        assert_eq!(merged.debug_dir, Some(PathBuf::from("/base/debug")));
        assert_eq!(merged.extra_input_globs, vec!["*.base".to_string()]);
        // experimental should be true when base has it enabled
        assert!(merged.experimental);
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
    fn test_merge_experimental_or_logic() {
        // Test that experimental uses OR logic: enabled if either base or target has it enabled

        // Case 1: base true, target false -> true
        let base = RattlerBuildBackendConfig {
            experimental: true,
            ..Default::default()
        };
        let target = RattlerBuildBackendConfig {
            experimental: false,
            ..Default::default()
        };
        let merged = base.merge_with_target_config(&target).unwrap();
        assert!(merged.experimental);

        // Case 2: base false, target true -> true
        let base = RattlerBuildBackendConfig {
            experimental: false,
            ..Default::default()
        };
        let target = RattlerBuildBackendConfig {
            experimental: true,
            ..Default::default()
        };
        let merged = base.merge_with_target_config(&target).unwrap();
        assert!(merged.experimental);

        // Case 3: both true -> true
        let base = RattlerBuildBackendConfig {
            experimental: true,
            ..Default::default()
        };
        let target = RattlerBuildBackendConfig {
            experimental: true,
            ..Default::default()
        };
        let merged = base.merge_with_target_config(&target).unwrap();
        assert!(merged.experimental);

        // Case 4: both false -> false
        let base = RattlerBuildBackendConfig {
            experimental: false,
            ..Default::default()
        };
        let target = RattlerBuildBackendConfig {
            experimental: false,
            ..Default::default()
        };
        let merged = base.merge_with_target_config(&target).unwrap();
        assert!(!merged.experimental);
    }

    #[test]
    fn test_deserialize_experimental() {
        let json_data = json!({
            "experimental": true
        });
        let config: RattlerBuildBackendConfig = serde_json::from_value(json_data).unwrap();
        assert!(config.experimental);

        let json_data = json!({
            "experimental": false
        });
        let config: RattlerBuildBackendConfig = serde_json::from_value(json_data).unwrap();
        assert!(!config.experimental);
    }
}
