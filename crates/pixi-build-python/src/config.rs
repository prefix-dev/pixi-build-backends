use indexmap::IndexMap;
use pixi_build_backend::generated_recipe::BackendConfig;
use serde::{Deserialize, Serialize};
use std::{
    convert::identity,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PythonBackendConfig {
    /// True if the package should be build as a python noarch package. Defaults
    /// to `true`.
    #[serde(default)]
    pub noarch: Option<bool>,
    /// Environment Variables
    #[serde(default)]
    pub env: IndexMap<String, String>,
    /// If set, internal state will be logged as files in that directory
    pub debug_dir: Option<PathBuf>,
    /// Extra input globs to include in addition to the default ones
    #[serde(default)]
    pub extra_input_globs: Vec<String>,
}

impl PythonBackendConfig {
    /// Whether to build a noarch package or a platform-specific package.
    pub fn noarch(&self) -> bool {
        self.noarch.is_none_or(identity)
    }
}

impl BackendConfig for PythonBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    /// Merge this configuration with a target-specific configuration.
    /// Target-specific values override base values using the following rules:
    /// - noarch: Platform-specific takes precedence (critical for cross-platform)
    /// - env: Platform env vars override base, others merge
    /// - debug_dir: Not allowed to have target specific value
    /// - extra_input_globs: Platform-specific completely replaces base
    fn merge_with_target_config(&self, target_config: &Self) -> miette::Result<Self> {
        if target_config.debug_dir.is_some() {
            miette::bail!("`debug_dir` cannot have a target specific value");
        }

        Ok(Self {
            noarch: target_config.noarch.or(self.noarch),
            env: {
                let mut merged_env = self.env.clone();
                merged_env.extend(target_config.env.clone());
                merged_env
            },
            debug_dir: target_config.debug_dir.clone().or(self.debug_dir.clone()),
            extra_input_globs: if target_config.extra_input_globs.is_empty() {
                self.extra_input_globs.clone()
            } else {
                target_config.extra_input_globs.clone()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PythonBackendConfig;
    use pixi_build_backend::generated_recipe::BackendConfig;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn test_ensure_deseralize_from_empty() {
        let json_data = json!({});
        serde_json::from_value::<PythonBackendConfig>(json_data).unwrap();
    }

    #[test]
    fn test_merge_with_target_config() {
        let mut base_env = indexmap::IndexMap::new();
        base_env.insert("BASE_VAR".to_string(), "base_value".to_string());
        base_env.insert("SHARED_VAR".to_string(), "base_shared".to_string());

        let base_config = PythonBackendConfig {
            noarch: Some(true),
            env: base_env,
            debug_dir: Some(PathBuf::from("/base/debug")),
            extra_input_globs: vec!["*.base".to_string()],
        };

        let mut target_env = indexmap::IndexMap::new();
        target_env.insert("TARGET_VAR".to_string(), "target_value".to_string());
        target_env.insert("SHARED_VAR".to_string(), "target_shared".to_string());

        let target_config = PythonBackendConfig {
            noarch: Some(false),
            env: target_env,
            debug_dir: Some(PathBuf::from("/target/debug")),
            extra_input_globs: vec!["*.target".to_string()],
        };

        let merged = base_config
            .merge_with_target_config(&target_config)
            .unwrap();

        // noarch should use target value
        assert_eq!(merged.noarch, Some(false));

        // env should merge with target taking precedence
        assert_eq!(merged.env.get("BASE_VAR"), Some(&"base_value".to_string()));
        assert_eq!(
            merged.env.get("TARGET_VAR"),
            Some(&"target_value".to_string())
        );
        assert_eq!(
            merged.env.get("SHARED_VAR"),
            Some(&"target_shared".to_string())
        );

        // debug_dir should use target value
        assert_eq!(merged.debug_dir, Some(PathBuf::from("/target/debug")));

        // extra_input_globs should be completely overridden
        assert_eq!(merged.extra_input_globs, vec!["*.target".to_string()]);
    }

    #[test]
    fn test_merge_with_empty_target_config() {
        let mut base_env = indexmap::IndexMap::new();
        base_env.insert("BASE_VAR".to_string(), "base_value".to_string());

        let base_config = PythonBackendConfig {
            noarch: Some(true),
            env: base_env,
            debug_dir: Some(PathBuf::from("/base/debug")),
            extra_input_globs: vec!["*.base".to_string()],
        };

        let empty_target_config = PythonBackendConfig::default();

        let merged = base_config
            .merge_with_target_config(&empty_target_config)
            .unwrap();

        // Should keep base values when target is empty
        assert_eq!(merged.noarch, Some(true));
        assert_eq!(merged.env.get("BASE_VAR"), Some(&"base_value".to_string()));
        assert_eq!(merged.debug_dir, Some(PathBuf::from("/base/debug")));
        assert_eq!(merged.extra_input_globs, vec!["*.base".to_string()]);
    }

    #[test]
    fn test_merge_noarch_behavior() {
        let base_config = PythonBackendConfig {
            noarch: Some(true),
            ..Default::default()
        };

        let target_config = PythonBackendConfig {
            noarch: None,
            ..Default::default()
        };

        let merged = base_config
            .merge_with_target_config(&target_config)
            .unwrap();

        // When target has None, should keep base value
        assert_eq!(merged.noarch, Some(true));

        // Test the reverse
        let base_config = PythonBackendConfig {
            noarch: None,
            ..Default::default()
        };

        let target_config = PythonBackendConfig {
            noarch: Some(false),
            ..Default::default()
        };

        let merged = base_config
            .merge_with_target_config(&target_config)
            .unwrap();

        // When target has value, should use target value
        assert_eq!(merged.noarch, Some(false));
    }
}
