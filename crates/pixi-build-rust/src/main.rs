mod build_script;
mod config;

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use build_script::BuildScriptContext;
use config::RustBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    cache::{enable_sccache, sccache_tools},
    compilers::{Language, compiler_requirement},
    generated_recipe::{GenerateRecipe, GeneratedRecipe},
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
    ) -> miette::Result<GeneratedRecipe> {
        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), manifest_root.clone());

        // we need to add compilers
        let compiler_function = compiler_requirement(&Language::Rust);

        let requirements = &mut generated_recipe.recipe.requirements;

        let resolved_requirements = requirements.resolve(Some(&host_platform));

        // Ensure the compiler function is added to the build requirements
        // only if it is not already present.

        if !resolved_requirements.build.contains_key(
            &PackageName::from_str(&Language::Rust.to_string())
                .expect("we expect Language::Rust to be a valid package name"),
        ) {
            requirements.build.push(compiler_function.clone());
        }

        let has_openssl = resolved_requirements.contains(&"openssl".parse().into_diagnostic()?);

        let mut has_sccache = false;

        let env_vars = config
            .env
            .clone()
            .into_iter()
            .chain(std::env::vars())
            .collect();

        if enable_sccache(env_vars) {
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
            env: config.env.clone(),
        };

        Ok(generated_recipe)
    }

    /// Returns the build input globs used by the backend.
    fn build_input_globs(config: &Self::Config, _workdir: impl AsRef<Path>) -> Vec<String> {
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

        let result = RustGenerator::build_input_globs(&config, PathBuf::new());

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

        let generated_recipe = RustGenerator::default()
            .generate_recipe(
                &project_model,
                &RustBackendConfig {
                    env,
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
            )
            .expect("Failed to generate recipe");

        // Verify that sccache is added to the build requirements
        // when some env variables are set
        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script" => "[ ... script ... ]",
        });
    }
}
