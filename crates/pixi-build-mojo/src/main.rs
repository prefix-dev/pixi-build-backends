mod build_script;
mod config;

use build_script::BuildScriptContext;
use config::{MojoBackendConfig, clean_project_name};
use miette::{Error, IntoDiagnostic};
use pixi_build_backend::generated_recipe::DefaultMetadataProvider;
use pixi_build_backend::{
    compilers::add_compilers_and_stdlib_to_requirements,
    generated_recipe::{GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
};
use rattler_build::NormalizedKey;
use rattler_conda_types::{PackageName, Platform};
use recipe_stage0::recipe::{ConditionalRequirements, Script};
use std::collections::HashSet;
use std::{collections::BTreeSet, path::Path, sync::Arc};

#[derive(Default, Clone)]
pub struct MojoGenerator {}

impl GenerateRecipe for MojoGenerator {
    type Config = MojoBackendConfig;

    fn generate_recipe(
        &self,
        model: &pixi_build_types::ProjectModelV1,
        config: &Self::Config,
        manifest_root: std::path::PathBuf,
        host_platform: Platform,
        _python_params: Option<PythonParams>,
        variants: &HashSet<NormalizedKey>,
    ) -> miette::Result<GeneratedRecipe> {
        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), &mut DefaultMetadataProvider)
                .into_diagnostic()?;

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
        let resolved_requirements = ConditionalRequirements::resolve(
            requirements.build.as_ref(),
            requirements.host.as_ref(),
            requirements.run.as_ref(),
            requirements.run_constraints.as_ref(),
            Some(host_platform),
        );

        // Get the list of compilers from config, defaulting to ["mojo"] if not specified
        let mut compilers = config
            .compilers
            .clone()
            .unwrap_or_else(|| vec!["mojo".to_string()]);

        // Handle mojo compiler specially if it's in the list
        if let Some(idx) = compilers.iter().position(|name| name == "mojo") {
            let mojo_compiler_pkg = "mojo-compiler".to_string();
            // All of these packages also contain the mojo compiler and maintain backward compat.
            // They should be removable at a future point.
            let alt_names = ["max", "mojo", "modular"];

            if !resolved_requirements
                .build
                .contains_key(&PackageName::new_unchecked(&mojo_compiler_pkg))
                && !alt_names
                    .iter()
                    .any(|alt| resolved_requirements.build.contains_key(*alt))
            {
                requirements
                    .build
                    .push(mojo_compiler_pkg.parse().into_diagnostic()?);
            }

            // Remove the mojo compiler from the list of compilers.
            compilers.swap_remove(idx);
        }

        add_compilers_and_stdlib_to_requirements(
            &compilers,
            &mut requirements.build,
            &resolved_requirements.build,
            &host_platform,
            variants,
        );

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
        &self,
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> miette::Result<BTreeSet<String>> {
        Ok(Self::globs()
            .chain(config.extra_input_globs.clone())
            .collect())
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

    use crate::config::{MojoBinConfig, MojoPkgConfig};
    use indexmap::IndexMap;
    use pixi_build_types::ProjectModelV1;
    use recipe_stage0::recipe::{Item, Value};

    use super::*;

    #[test]
    fn test_input_globs_includes_extra_globs() {
        let config = MojoBackendConfig {
            extra_input_globs: vec![String::from("**/.c")],
            ..Default::default()
        };

        let generator = MojoGenerator::default();

        let result = generator.extract_input_globs_from_build(&config, PathBuf::new(), false);

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
                &HashSet::new(),
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
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        });
    }

    #[test]
    fn test_compiler_is_in_build_requirements() {
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
                &HashSet::new(),
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
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe.build.script,
        {
            ".content" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_compiler_is_not_added_if_compiler_is_already_present() {
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
                        "mojo-compiler": {
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
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_mojo_with_additional_compilers() {
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
                &MojoBackendConfig {
                    compilers: Some(vec!["mojo".to_string(), "c".to_string(), "cxx".to_string()]),
                    ..Default::default()
                },
                temp.path().to_path_buf(),
                Platform::Linux64,
                None,
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        // Check that we have both the mojo-compiler package and the additional compilers
        let build_reqs = &generated_recipe.recipe.requirements.build;

        // Check for mojo-compiler package (should be present)
        let has_mojo_compiler = build_reqs
            .iter()
            .any(|item| format!("{item:?}").contains("mojo-compiler"));
        assert!(has_mojo_compiler, "Should have mojo-compiler package");

        // Check for additional compiler templates
        let compiler_templates: Vec<String> = build_reqs
            .iter()
            .filter_map(|item| match item {
                Item::Value(Value::Template(s)) if s.contains("compiler") => Some(s.clone()),
                _ => None,
            })
            .collect();

        // Should have exactly two additional compilers (c and cxx, but not mojo template)
        assert_eq!(
            compiler_templates.len(),
            2,
            "Should have exactly two additional compilers"
        );

        // Check we have the expected additional compilers
        assert!(
            compiler_templates.contains(&"${{ compiler('c') }}".to_string()),
            "C compiler should be in build requirements"
        );
        assert!(
            compiler_templates.contains(&"${{ compiler('cxx') }}".to_string()),
            "C++ compiler should be in build requirements"
        );

        // Ensure we don't have a mojo template (since mojo uses special package)
        assert!(
            !compiler_templates.contains(&"${{ compiler('mojo') }}".to_string()),
            "Should not have mojo compiler template since it uses special package"
        );
    }

    #[test]
    fn test_default_mojo_compiler_behavior() {
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
                &MojoBackendConfig {
                    compilers: None,
                    ..Default::default()
                },
                temp.path().to_path_buf(),
                Platform::Linux64,
                None,
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        // Check that we have only the mojo-compiler package by default
        let build_reqs = &generated_recipe.recipe.requirements.build;

        // Check for mojo-compiler package (should be present by default)
        let has_mojo_compiler = build_reqs
            .iter()
            .any(|item| format!("{item:?}").contains("mojo-compiler"));
        assert!(
            has_mojo_compiler,
            "Should have mojo-compiler package by default"
        );

        // Check that no additional compiler templates are present
        let compiler_templates: Vec<String> = build_reqs
            .iter()
            .filter_map(|item| match item {
                Item::Value(Value::Template(s)) if s.contains("compiler") => Some(s.clone()),
                _ => None,
            })
            .collect();

        // Should have no additional compiler templates by default
        assert_eq!(
            compiler_templates.len(),
            0,
            "Should have no additional compiler templates by default"
        );
    }

    #[test]
    fn test_opt_out_of_mojo_compiler() {
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
                &MojoBackendConfig {
                    compilers: Some(vec!["c".to_string(), "cxx".to_string()]),
                    ..Default::default()
                },
                temp.path().to_path_buf(),
                Platform::Linux64,
                None,
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        // Check that mojo-compiler is NOT present when user opts out
        let build_reqs = &generated_recipe.recipe.requirements.build;

        // Check for mojo-compiler package (should NOT be present)
        let has_mojo_compiler = build_reqs
            .iter()
            .any(|item| format!("{item:?}").contains("mojo-compiler"));
        assert!(
            !has_mojo_compiler,
            "Should NOT have mojo-compiler package when user opts out"
        );

        // Check for other compiler templates
        let compiler_templates: Vec<String> = build_reqs
            .iter()
            .filter_map(|item| match item {
                Item::Value(Value::Template(s)) if s.contains("compiler") => Some(s.clone()),
                _ => None,
            })
            .collect();

        // Should have exactly two compilers (c and cxx)
        assert_eq!(
            compiler_templates.len(),
            2,
            "Should have exactly two compilers when opting out of mojo"
        );

        // Check we have the expected compilers
        assert!(
            compiler_templates.contains(&"${{ compiler('c') }}".to_string()),
            "C compiler should be in build requirements"
        );
        assert!(
            compiler_templates.contains(&"${{ compiler('cxx') }}".to_string()),
            "C++ compiler should be in build requirements"
        );
    }
}
