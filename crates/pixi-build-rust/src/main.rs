mod build_script;
mod config;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use build_script::BuildScriptContext;
use config::RustBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    cache::{sccache_envs, sccache_tools},
    compilers::{Language, compiler_requirement},
    generated_recipe::{GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
};
use pixi_build_types::ProjectModelV1;
use rattler_conda_types::{PackageName, Platform};
use recipe_stage0::{
    matchspec::PackageDependency,
    recipe::{Item, Script},
};

#[derive(Default, Clone)]
pub struct RustGenerator {}

impl GenerateRecipe for RustGenerator {
    type Config = RustBackendConfig;

    fn generate_recipe(
        &self,
        model: &ProjectModelV1,
        config: &Self::Config,
        manifest_root: PathBuf,
        host_platform: Platform,
        _python_params: Option<PythonParams>,
    ) -> miette::Result<GeneratedRecipe> {
        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), manifest_root.clone());

        // we need to add compilers
        let compiler_function = compiler_requirement(&Language::Rust);

        let requirements = &mut generated_recipe.recipe.requirements;

        let resolved_requirements = requirements.resolve(Some(&host_platform));

        // Ensure the compiler function is added to the build requirements
        // only if it is not already present.

        if !resolved_requirements
            .build
            .contains_key(&PackageName::new_unchecked(Language::Rust.to_string()))
        {
            requirements.build.push(compiler_function.clone());
        }

        let has_openssl = resolved_requirements.contains(&"openssl".parse().into_diagnostic()?);

        let mut has_sccache = false;

        let config_env = config.env.clone();

        let system_env_vars = std::env::vars().collect::<HashMap<String, String>>();

        let all_env_vars = config_env
            .clone()
            .into_iter()
            .chain(system_env_vars.clone())
            .collect();

        let mut sccache_secrets = Vec::default();

        // Verify if user has set any sccache environment variables
        if sccache_envs(&all_env_vars).is_some() {
            // check if we set some sccache in system env vars
            if let Some(system_sccache_keys) = sccache_envs(&system_env_vars) {
                // If sccache_envs are used in the system environment variables,
                // we need to set them as secrets
                let system_sccache_keys = system_env_vars
                    .keys()
                    // we set only those keys that are present in the system environment variables and not in the config env
                    .filter(|key| {
                        system_sccache_keys.contains(&key.as_str())
                            && !config_env.contains_key(*key)
                    })
                    .cloned()
                    .collect();

                sccache_secrets = system_sccache_keys;
            };

            let sccache_dep: Vec<Item<PackageDependency>> = sccache_tools()
                .iter()
                .map(|tool| tool.parse().into_diagnostic())
                .collect::<miette::Result<Vec<_>>>()?;

            // Add sccache tools to the build requirements
            // only if they are not already present
            let existing_reqs: Vec<_> = requirements.build.clone().into_iter().collect();

            requirements.build.extend(
                sccache_dep
                    .into_iter()
                    .filter(|dep| !existing_reqs.contains(dep)),
            );

            has_sccache = true;
        }

        let build_script = BuildScriptContext {
            source_dir: manifest_root.display().to_string(),
            extra_args: config.extra_args.clone(),
            has_openssl,
            has_sccache,
            is_bash: !Platform::current().is_windows(),
        }
        .render();

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config_env,
            secrets: sccache_secrets,
        };

        Ok(generated_recipe)
    }

    /// Returns the build input globs used by the backend.
    fn extract_input_globs_from_build(
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> Vec<String> {
        [
            "**/*.rs",
            // Cargo configuration files
            "Cargo.toml",
            "Cargo.lock",
            // Build scripts
            "build.rs",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain(config.extra_input_globs.clone())
        .collect()
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) =
        pixi_build_backend::cli::main(IntermediateBackendInstantiator::<RustGenerator>::new).await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;

    #[test]
    fn test_input_globs_includes_extra_globs() {
        let config = RustBackendConfig {
            extra_input_globs: vec!["custom/*.txt".to_string(), "extra/**/*.py".to_string()],
            ..Default::default()
        };

        let result = RustGenerator::extract_input_globs_from_build(&config, PathBuf::new(), false);

        // Verify that all extra globs are included in the result
        for extra_glob in &config.extra_input_globs {
            assert!(
                result.contains(extra_glob),
                "Result should contain extra glob: {}",
                extra_glob
            );
        }

        // Verify that default globs are still present
        assert!(result.contains(&"**/*.rs".to_string()));
        assert!(result.contains(&"Cargo.toml".to_string()));
        assert!(result.contains(&"Cargo.lock".to_string()));
        assert!(result.contains(&"build.rs".to_string()));
    }

    #[macro_export]
    macro_rules! project_fixture {
        ($($json:tt)+) => {
            serde_json::from_value::<ProjectModelV1>(
                serde_json::json!($($json)+)
            ).expect("Failed to create TestProjectModel from JSON fixture.")
        };
    }

    #[test]
    fn test_rust_is_in_build_requirements() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {
                    "runDependencies": {
                        "boltons": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let generated_recipe = RustGenerator::default()
            .generate_recipe(
                &project_model,
                &RustBackendConfig::default(),
                PathBuf::from("."),
                Platform::Linux64,
                None,
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_rust_is_not_added_if_already_present() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {
                    "runDependencies": {
                        "boltons": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    },
                    "buildDependencies": {
                        "rust": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let generated_recipe = RustGenerator::default()
            .generate_recipe(
                &project_model,
                &RustBackendConfig::default(),
                PathBuf::from("."),
                Platform::Linux64,
                None,
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_env_vars_are_set() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {
                    "runDependencies": {
                        "boltons": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let env = IndexMap::from([("foo".to_string(), "bar".to_string())]);

        let generated_recipe = RustGenerator::default()
            .generate_recipe(
                &project_model,
                &RustBackendConfig {
                    env: env.clone(),
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe.build.script,
        {
            ".content" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_sccache_is_enabled() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "default_target": {
                    "run_dependencies": {
                        "boltons": "*"
                    }
                },
            }
        });

        let env = IndexMap::from([("SCCACHE_BUCKET".to_string(), "my-bucket".to_string())]);

        let system_env_vars = [
            ("SCCACHE_SYSTEM", Some("SOME_VALUE")),
            // We want to test that config env variable wins over system env variable
            ("SCCACHE_BUCKET", Some("system-bucket")),
        ];

        let generated_recipe = temp_env::with_vars(system_env_vars, || {
            RustGenerator::default()
                .generate_recipe(
                    &project_model,
                    &RustBackendConfig {
                        env,
                        ..Default::default()
                    },
                    PathBuf::from("."),
                    Platform::Linux64,
                    None,
                )
                .expect("Failed to generate recipe")
        });

        // Verify that sccache is added to the build requirements
        // when some env variables are set
        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script.content" => "[ ... script ... ]",
        });
    }
}
