mod build_script;
mod config;

use build_script::BuildScriptContext;
use config::ZigBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::variants::NormalizedKey;
use pixi_build_backend::{
    generated_recipe::{DefaultMetadataProvider, GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
};
use pixi_build_types::ProjectModelV1;
use rattler_conda_types::{MatchSpec, PackageName, Platform};
use recipe_stage0::matchspec::PackageDependency;
use recipe_stage0::recipe::{ConditionalRequirements, Script};
use std::collections::HashSet;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Default, Clone)]
pub struct ZigGenerator {}

impl GenerateRecipe for ZigGenerator {
    type Config = ZigBackendConfig;

    fn generate_recipe(
        &self,
        model: &ProjectModelV1,
        config: &Self::Config,
        _manifest_root: PathBuf,
        host_platform: Platform,
        _python_params: Option<PythonParams>,
        _variants: &HashSet<NormalizedKey>,
    ) -> miette::Result<GeneratedRecipe> {
        // Create the recipe using the default metadata provider
        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), &mut DefaultMetadataProvider)
                .into_diagnostic()?;

        let requirements = &mut generated_recipe.recipe.requirements;

        let resolved_requirements = ConditionalRequirements::resolve(
            requirements.build.as_ref(),
            requirements.host.as_ref(),
            requirements.run.as_ref(),
            requirements.run_constraints.as_ref(),
            Some(host_platform),
        );

        // Add zig to build requirements if not already present
        let zig_name = PackageName::new_unchecked("zig");
        if !resolved_requirements.build.contains_key(&zig_name) {
            requirements.build.push(
                PackageDependency::Binary(
                    MatchSpec::from_str("zig", rattler_conda_types::ParseStrictness::Lenient)
                        .into_diagnostic()?,
                )
                .into(),
            );
        }

        let build_script = BuildScriptContext {
            extra_args: config.extra_args.clone(),
            is_bash: !Platform::current().is_windows(),
        }
        .render();

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config.env.clone(),
            ..Default::default()
        };

        Ok(generated_recipe)
    }

    /// Returns the build input globs used by the backend.
    fn extract_input_globs_from_build(
        &self,
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> miette::Result<BTreeSet<String>> {
        Ok([
            "**/*.zig",
            // Zig build files
            "build.zig",
            "build.zig.zon",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain(config.extra_input_globs.clone())
        .collect())
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(|log| {
        IntermediateBackendInstantiator::<ZigGenerator>::new(log, Arc::default())
    })
    .await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;

    #[macro_export]
    macro_rules! project_fixture {
        ($($json:tt)+) => {
            serde_json::from_value::<ProjectModelV1>(
                serde_json::json!($($json)+)
            ).expect("Failed to create TestProjectModel from JSON fixture.")
        };
    }

    #[test]
    fn test_input_globs_includes_extra_globs() {
        let config = ZigBackendConfig {
            extra_input_globs: vec!["custom/*.txt".to_string(), "extra/**/*.zig".to_string()],
            ..Default::default()
        };

        let generator = ZigGenerator::default();

        let result = generator
            .extract_input_globs_from_build(&config, PathBuf::new(), false)
            .unwrap();

        // Verify that all extra globs are included in the result
        for extra_glob in &config.extra_input_globs {
            assert!(
                result.contains(extra_glob),
                "Result should contain extra glob: {extra_glob}"
            );
        }

        // Verify that default globs are still present
        assert!(result.contains("**/*.zig"));
        assert!(result.contains("build.zig"));
        assert!(result.contains("build.zig.zon"));
    }

    #[test]
    fn test_zig_is_in_build_requirements() {
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

        let generated_recipe = ZigGenerator::default()
            .generate_recipe(
                &project_model,
                &ZigBackendConfig::default(),
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script" => "[ ... script ... ]",
        }, @r#"
        context: {}
        package:
          name: foobar
          version: 0.1.0
        source: []
        build:
          number: ~
          script: "[ ... script ... ]"
        requirements:
          build:
            - zig
          host: []
          run:
            - boltons
          run_constraints: []
        tests: []
        about:
          homepage: ~
          license: ~
          license_file: ~
          summary: ~
          description: ~
          documentation: ~
          repository: ~
        extra: ~
        "#);
    }

    #[test]
    fn test_zig_is_not_added_if_already_present() {
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
                        "zig": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let generated_recipe = ZigGenerator::default()
            .generate_recipe(
                &project_model,
                &ZigBackendConfig::default(),
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script" => "[ ... script ... ]",
        }, @r#"
        context: {}
        package:
          name: foobar
          version: 0.1.0
        source: []
        build:
          number: ~
          script: "[ ... script ... ]"
        requirements:
          build:
            - zig
          host: []
          run:
            - boltons
          run_constraints: []
        tests: []
        about:
          homepage: ~
          license: ~
          license_file: ~
          summary: ~
          description: ~
          documentation: ~
          repository: ~
        extra: ~
        "#);
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

        let generated_recipe = ZigGenerator::default()
            .generate_recipe(
                &project_model,
                &ZigBackendConfig {
                    env: env.clone(),
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe.build.script,
        {
            ".content" => "[ ... script ... ]",
        }, @r#"
        content: "[ ... script ... ]"
        env:
          foo: bar
        secrets: []
        "#);
    }
}
