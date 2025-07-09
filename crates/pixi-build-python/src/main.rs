mod build_script;
mod config;

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use build_script::{BuildPlatform, BuildScriptContext, Installer};
use config::PythonBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    generated_recipe::{GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
};
use pixi_build_types::ProjectModelV1;
use pyproject_toml::PyProjectToml;
use rattler_conda_types::{PackageName, Platform, package::EntryPoint};
use recipe_stage0::recipe::{NoArchKind, Python, Script};

#[derive(Default, Clone)]
pub struct PythonGenerator {}

impl PythonGenerator {
    /// Read the entry points from the pyproject.toml and return them as a list.
    ///
    /// If the manifest is not a pyproject.toml file no entry-points are added.
    pub(crate) fn entry_points(pyproject_manifest: Option<PyProjectToml>) -> Vec<EntryPoint> {
        let scripts = pyproject_manifest
            .as_ref()
            .and_then(|p| p.project.as_ref())
            .and_then(|p| p.scripts.as_ref());

        scripts
            .into_iter()
            .flatten()
            .flat_map(|(name, entry_point)| {
                EntryPoint::from_str(&format!("{name} = {entry_point}"))
            })
            .collect()
    }
}

impl GenerateRecipe for PythonGenerator {
    type Config = PythonBackendConfig;

    fn generate_recipe(
        &self,
        model: &ProjectModelV1,
        config: &Self::Config,
        manifest_root: PathBuf,
        host_platform: Platform,
        python_params: Option<PythonParams>,
    ) -> miette::Result<GeneratedRecipe> {
        let params = python_params.unwrap_or_default();

        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), manifest_root.clone());

        let requirements = &mut generated_recipe.recipe.requirements;

        let resolved_requirements = requirements.resolve(Some(&host_platform));

        // Ensure the python build tools are added to the `host` requirements.
        // Please note: this is a subtle difference for python, where the build tools are
        // added to the `host` requirements, while for cmake/rust they are added to the `build` requirements.
        let installer = Installer::determine_installer(&resolved_requirements);

        let installer_name = installer.package_name().to_string();

        // add installer in the host requirements
        if !resolved_requirements
            .host
            .contains_key(&PackageName::new_unchecked(&installer_name))
        {
            requirements
                .host
                .push(installer_name.parse().into_diagnostic()?);
        }

        // add python in both host and run requirements
        if !resolved_requirements
            .host
            .contains_key(&PackageName::new_unchecked("python"))
        {
            requirements.host.push("python".parse().into_diagnostic()?);
        }
        if !resolved_requirements
            .run
            .contains_key(&PackageName::new_unchecked("python"))
        {
            requirements.run.push("python".parse().into_diagnostic()?);
        }

        let build_platform = Platform::current();

        // TODO: remove this env var override as soon as we have profiles
        let editable = std::env::var("BUILD_EDITABLE_PYTHON")
            .map(|val| val == "true")
            .unwrap_or(params.editable);

        let build_script = BuildScriptContext {
            installer,
            build_platform: if build_platform.is_windows() {
                BuildPlatform::Windows
            } else {
                BuildPlatform::Unix
            },
            editable,
            manifest_root: manifest_root.clone(),
        }
        .render();

        // Determine whether the package should be built as a noarch package or as a
        // generic package.
        let noarch_kind = if config.noarch() {
            Some(NoArchKind::Python)
        } else {
            None
        };

        // read pyproject.toml content if it exists
        let pyproject_manifest_path = manifest_root.join("pyproject.toml");
        let pyproject_manifest = if pyproject_manifest_path.exists() {
            let contents = std::fs::read_to_string(&pyproject_manifest_path).into_diagnostic()?;
            generated_recipe.build_input_globs =
                vec![pyproject_manifest_path.to_string_lossy().to_string()];
            Some(toml_edit::de::from_str(&contents).into_diagnostic()?)
        } else {
            None
        };

        // Construct python specific settings
        let python = Python {
            entry_points: PythonGenerator::entry_points(pyproject_manifest),
        };

        generated_recipe.recipe.build.python = python;
        generated_recipe.recipe.build.noarch = noarch_kind;

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config.env.clone(),
            ..Script::default()
        };

        Ok(generated_recipe)
    }

    /// Determines the build input globs for given python package
    /// even this will be probably backend specific, e.g setuptools
    /// has a different way of determining the input globs than hatch etc.
    ///
    /// However, lets take everything in the directory as input for now
    fn extract_input_globs_from_build(
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        editable: bool,
    ) -> Vec<String> {
        let base_globs = Vec::from([
            // Source files
            "**/*.c",
            "**/*.cpp",
            "**/*.rs",
            "**/*.sh",
            // Common data files
            "**/*.json",
            "**/*.yaml",
            "**/*.yml",
            "**/*.txt",
            // Project configuration
            "setup.py",
            "setup.cfg",
            "pyproject.toml",
            "requirements*.txt",
            "Pipfile",
            "Pipfile.lock",
            "poetry.lock",
            "tox.ini",
            // Build configuration
            "Makefile",
            "MANIFEST.in",
            "tests/**/*.py",
            "docs/**/*.rst",
            "docs/**/*.md",
            // Versioning
            "VERSION",
            "version.py",
        ]);

        let python_globs = if editable {
            Vec::new()
        } else {
            Vec::from(["**/*.py", "**/*.pyx"])
        };

        base_globs
            .iter()
            .chain(python_globs.iter())
            .map(|s| s.to_string())
            .chain(config.extra_input_globs.clone())
            .collect()
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) =
        pixi_build_backend::cli::main(IntermediateBackendInstantiator::<PythonGenerator>::new).await
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
        let config = PythonBackendConfig {
            extra_input_globs: vec!["custom/*.py".to_string()],
            ..Default::default()
        };

        let result =
            PythonGenerator::extract_input_globs_from_build(&config, PathBuf::new(), false);

        insta::assert_debug_snapshot!(result);
    }

    #[test]
    fn test_input_globs_includes_extra_globs_editable() {
        let config = PythonBackendConfig {
            extra_input_globs: vec!["custom/*.py".to_string()],
            ..Default::default()
        };

        let result = PythonGenerator::extract_input_globs_from_build(&config, PathBuf::new(), true);

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
    fn test_pip_is_in_host_requirements() {
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

        let generated_recipe = PythonGenerator::default()
            .generate_recipe(
                &project_model,
                &PythonBackendConfig::default(),
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
    fn test_python_is_not_added_if_already_present() {
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
                    "hostDependencies": {
                        "python": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let generated_recipe = PythonGenerator::default()
            .generate_recipe(
                &project_model,
                &PythonBackendConfig::default(),
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

        let generated_recipe = PythonGenerator::default()
            .generate_recipe(
                &project_model,
                &PythonBackendConfig {
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
}
