use pixi_build_backend::generated_recipe::BackendConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct RattlerBuildBackendConfig {
    /// If set, internal state will be logged as files in that directory
    pub debug_dir: Option<PathBuf>,
    /// Extra input globs to include in addition to the default ones
    #[serde(default)]
    pub extra_input_globs: Vec<String>,
}

impl BackendConfig for RattlerBuildBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    /// Merge this configuration with a target-specific configuration.
    /// Target-specific values override base values using the following rules:
    /// - debug_dir: Platform-specific takes precedence
    /// - extra_input_globs: Platform-specific completely replaces base
    fn merge_with_target_config(&self, target_config: &Self) -> Self {
        Self {
            debug_dir: target_config.debug_dir.clone().or(self.debug_dir.clone()),
            extra_input_globs: if target_config.extra_input_globs.is_empty() {
                self.extra_input_globs.clone()
            } else {
                target_config.extra_input_globs.clone()
            },
        }
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
        };

        let target_config = RattlerBuildBackendConfig {
            debug_dir: Some(PathBuf::from("/target/debug")),
            extra_input_globs: vec!["*.target".to_string()],
        };

        let merged = base_config.merge_with_target_config(&target_config);

        // debug_dir should use target value
        assert_eq!(merged.debug_dir, Some(PathBuf::from("/target/debug")));

        // extra_input_globs should be completely overridden
        assert_eq!(merged.extra_input_globs, vec!["*.target".to_string()]);
    }

    #[test]
    fn test_merge_with_empty_target_config() {
        let base_config = RattlerBuildBackendConfig {
            debug_dir: Some(PathBuf::from("/base/debug")),
            extra_input_globs: vec!["*.base".to_string()],
        };

        let empty_target_config = RattlerBuildBackendConfig::default();

        let merged = base_config.merge_with_target_config(&empty_target_config);

        // Should keep base values when target is empty
        assert_eq!(merged.debug_dir, Some(PathBuf::from("/base/debug")));
        assert_eq!(merged.extra_input_globs, vec!["*.base".to_string()]);
    }
}
