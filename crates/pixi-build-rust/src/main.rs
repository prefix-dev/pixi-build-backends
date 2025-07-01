mod build_script;
mod config;

use std::path::{Path, PathBuf};

use build_script::BuildScriptContext;
use config::RustBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    cache::{enable_sccache, sccache_tools},
    compilers::{Language, compiler_requirements},
    generated_recipe::{GenerateRecipe, GeneratedRecipe},
    intermediate_backend::IntermediateBackendInstantiator,
};
use pixi_build_types::ProjectModelV1;
use rattler_conda_types::Platform;
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
        let conditional_compiler_requirements = compiler_requirements(&Language::Rust);

        let requirements = &mut generated_recipe.recipe.requirements;
        requirements
            .build
            .extend(conditional_compiler_requirements.clone());

        let has_openssl = requirements
            .resolve(Some(&host_platform))
            .contains(&"openssl".parse().into_diagnostic()?);

        let mut has_sccache = false;

        if enable_sccache(std::env::vars().collect()) {
            let sccache_dep: Vec<Item<PackageDependency>> = sccache_tools()
                .iter()
                .map(|tool| tool.parse().into_diagnostic())
                .collect::<miette::Result<Vec<_>>>()?;

            requirements.build.extend(sccache_dep);

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
                "default_target": {
                    "run_dependencies": {
                        "boltons": "*"
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
                "default_target": {
                    "run_dependencies": {
                        "boltons": "*"
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
            ".script" => "[ ... script ... ]",
        });
    }
}
