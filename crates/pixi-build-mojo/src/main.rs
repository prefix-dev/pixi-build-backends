mod build_script;
mod config;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use build_script::BuildScriptContext;
use config::{MojoBackendConfig, clean_project_name};
use miette::{Error, IntoDiagnostic};
use pixi_build_backend::{
    generated_recipe::{GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
};
use rattler_build::{NormalizedKey, recipe::variable::Variable};
use rattler_conda_types::{PackageName, Platform};
use recipe_stage0::recipe::Script;

#[derive(Default, Clone)]
pub struct MojoGenerator {}

impl GenerateRecipe for MojoGenerator {
    type Config = MojoBackendConfig;

    fn generate_recipe(
        &self,
        model: &pixi_build_types::ProjectModelV1,
        config: &Self::Config,
        manifest_root: std::path::PathBuf,
        host_platform: rattler_conda_types::Platform,
        _python_params: Option<PythonParams>,
    ) -> miette::Result<GeneratedRecipe> {
        let mut generated_recipe = GeneratedRecipe::from_model(model.clone());

        let cleaned_project_name = clean_project_name(
            generated_recipe
                .recipe
                .package
                .name
                .concrete()
                .ok_or(Error::msg("Package is missing a name"))?,
        );

        // Auto-derive bins and pkg fields/configs if needed
        let (bins, pkg) = config.auto_derive(&manifest_root, &cleaned_project_name)?;

        // Add compiler
        let requirements = &mut generated_recipe.recipe.requirements;
        let resolved_requirements = requirements.resolve(Some(host_platform));

        // Ensure the compiler function is added to the build requirements
        // only if a specific compiler is not already present.
        let mojo_compiler_pkg = "max".to_string();

        if !resolved_requirements
            .build
            .contains_key(&PackageName::new_unchecked(&mojo_compiler_pkg))
        {
            requirements
                .build
                .push(mojo_compiler_pkg.parse().into_diagnostic()?);
        }

        let build_script = BuildScriptContext {
            source_dir: manifest_root.display().to_string(),
            bins,
            pkg,
        }
        .render();

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config.env.clone(),
            ..Default::default()
        };

        generated_recipe.build_input_globs = Self::globs().collect::<BTreeSet<_>>();

        Ok(generated_recipe)
    }

    fn extract_input_globs_from_build(
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> BTreeSet<String> {
        Self::globs()
            .chain(config.extra_input_globs.clone())
            .collect()
    }

    fn default_variants(&self, _host_platform: Platform) -> BTreeMap<NormalizedKey, Vec<Variable>> {
        BTreeMap::new()
    }
}

impl MojoGenerator {
    fn globs() -> impl Iterator<Item = String> {
        [
            // Source files
            "**/*.{mojo,🔥}",
        ]
        .iter()
        .map(|s: &&str| s.to_string())
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(|log| {
        IntermediateBackendInstantiator::<MojoGenerator>::new(log, Arc::default())
    })
    .await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use indexmap::IndexMap;
    use pixi_build_types::ProjectModelV1;

    use crate::config::{MojoBinConfig, MojoPkgConfig};

    use super::*;

    #[test]
    fn test_input_globs_includes_extra_globs() {
        let config = MojoBackendConfig {
            extra_input_globs: vec![String::from("**/.c")],
            ..Default::default()
        };

        let result = MojoGenerator::extract_input_globs_from_build(&config, PathBuf::new(), false);

        insta::assert_debug_snapshot!(result);
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
    fn test_mojo_bin_is_set() {
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

        let generated_recipe = MojoGenerator::default()
            .generate_recipe(
                &project_model,
                &MojoBackendConfig {
                    bins: Some(vec![MojoBinConfig {
                        name: Some(String::from("example")),
                        path: Some(String::from("./main.mojo")),
                        extra_args: Some(vec![String::from("-I"), String::from(".")]),
                    }]),
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        });
    }

    #[test]
    fn test_mojo_pkg_is_set() {
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

        let generated_recipe = MojoGenerator::default()
            .generate_recipe(
                &project_model,
                &MojoBackendConfig {
                    bins: Some(vec![MojoBinConfig {
                        name: Some(String::from("example")),
                        path: Some(String::from("./main.mojo")),
                        extra_args: Some(vec![String::from("-i"), String::from(".")]),
                    }]),
                    pkg: Some(MojoPkgConfig {
                        name: Some(String::from("lib")),
                        path: Some(String::from("mylib")),
                        extra_args: Some(vec![String::from("-i"), String::from(".")]),
                    }),
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        });
    }

    #[test]
    fn test_max_is_in_build_requirements() {
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

        // Create a temporary directory with a main.mojo file so the test has something to build
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("main.mojo"), "def main():\n    pass").unwrap();

        let generated_recipe = MojoGenerator::default()
            .generate_recipe(
                &project_model,
                &MojoBackendConfig::default(),
                temp.path().to_path_buf(),
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

        // Create a temporary directory with a main.mojo file so the test has something to build
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("main.mojo"), "def main():\n    pass").unwrap();

        let generated_recipe = MojoGenerator::default()
            .generate_recipe(
                &project_model,
                &MojoBackendConfig {
                    env: env.clone(),
                    ..Default::default()
                },
                temp.path().to_path_buf(),
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
    fn test_max_is_not_added_if_max_is_already_present() {
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
                        "max": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        // Create a temporary directory with a main.mojo file so the test has something to build
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("main.mojo"), "def main():\n    pass").unwrap();

        let generated_recipe = MojoGenerator::default()
            .generate_recipe(
                &project_model,
                &MojoBackendConfig::default(),
                temp.path().to_path_buf(),
                Platform::Linux64,
                None,
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script" => "[ ... script ... ]",
        });
    }
}
