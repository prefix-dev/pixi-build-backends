mod build_script;
mod config;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use build_script::BuildScriptContext;
use config::MojoBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    generated_recipe::{GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
};
use rattler_build::{recipe::variable::Variable, NormalizedKey};
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
        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), manifest_root.clone());

        // we need to add compilers
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

        // Check if the host platform has a host python dependency
        // TODO: surely this will be needed for compiling bindings or something? or maybe those
        // will be handled by uv?
        let has_host_python = resolved_requirements.contains(&PackageName::new_unchecked("python"));

        let build_script = BuildScriptContext {
            source_dir: manifest_root.display().to_string(),
            dist: config.dist_dir.clone().map(|d| d.display().to_string()),
            bins: config.bins.clone(),
            pkg: config.pkg.clone(),
            has_host_python,
        }
        .render();

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config.env.clone(),
            ..Default::default()
        };

        // How do I set globs on the build??
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
            "**/pixi.toml",
            "**/pixi.lock",
            "**/recipe.yaml",
        ]
        .iter()
        .map(|s: &&str| s.to_string())
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) =
        pixi_build_backend::cli::main(IntermediateBackendInstantiator::<MojoGenerator>::new).await
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
                        name: String::from("example"),
                        path: String::from("./main.mojo"),
                        extra_args: Some(vec![String::from("-I"), String::from(".")]),
                        env: IndexMap::new(),
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
                        name: String::from("example"),
                        path: String::from("./main.mojo"),
                        extra_args: Some(vec![String::from("-i"), String::from(".")]),
                        env: IndexMap::new(),
                    }]),
                    pkg: Some(MojoPkgConfig {
                        name: String::from("lib"),
                        path: String::from("mylib"),
                        extra_args: Some(vec![String::from("-i"), String::from(".")]),
                        env: IndexMap::new(),
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

        let generated_recipe = MojoGenerator::default()
            .generate_recipe(
                &project_model,
                &MojoBackendConfig::default(),
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

        let generated_recipe = MojoGenerator::default()
            .generate_recipe(
                &project_model,
                &MojoBackendConfig {
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

    // I think we'll want this back at some point
    //    #[test]
    //    fn test_has_python_is_set_in_build_script() {
    //        let project_model = project_fixture!({
    //            "name": "foobar",
    //            "version": "0.1.0",
    //            "targets": {
    //                "defaultTarget": {
    //                    "runDependencies": {
    //                        "boltons": {
    //                            "binary": {
    //                                "version": "*"
    //                            }
    //                        }
    //                    },
    //                    "hostDependencies": {
    //                        "python": {
    //                            "binary": {
    //                                "version": "*"
    //                            }
    //                        }
    //                    }
    //                },
    //            }
    //        });
    //
    //        let generated_recipe = MojoGenerator::default()
    //            .generate_recipe(
    //                &project_model,
    //                &MojoBackendConfig::default(),
    //                PathBuf::from("."),
    //                Platform::Linux64,
    //                None,
    //            )
    //            .expect("Failed to generate recipe");
    //
    //        // we want to check that
    //        // -DPython_EXECUTABLE=$PYTHON is set in the build script
    //        insta::assert_yaml_snapshot!(generated_recipe.recipe.build,
    //
    //            {
    //            ".script.content" => insta::dynamic_redaction(|value, _path| {
    //                dbg!(&value);
    //                // assert that the value looks like a uuid here
    //                assert!(value
    //                    .as_slice()
    //                    .unwrap()
    //                    .iter()
    //                    .any(|c| c.as_str().unwrap().contains("-DPython_EXECUTABLE"))
    //                );
    //                "[content]"
    //            })
    //        });
    //    }

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

        let generated_recipe = MojoGenerator::default()
            .generate_recipe(
                &project_model,
                &MojoBackendConfig::default(),
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
}
