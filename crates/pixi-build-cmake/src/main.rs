mod build_script;
mod config;

use build_script::{BuildPlatform, BuildScriptContext};
use config::CMakeBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    compilers::add_compilers_and_stdlib_to_requirements,
    generated_recipe::{DefaultMetadataProvider, GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
};
use pixi_build_types::ProjectModelV1;
use rattler_build::{NormalizedKey, recipe::variable::Variable};
use rattler_conda_types::{ChannelUrl, PackageName, Platform};
use recipe_stage0::recipe::{ConditionalRequirements, Script};
use std::collections::HashSet;
use std::path::PathBuf;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

#[derive(Default, Clone)]
pub struct CMakeGenerator {}

impl GenerateRecipe for CMakeGenerator {
    type Config = CMakeBackendConfig;

    fn generate_recipe(
        &self,
        model: &ProjectModelV1,
        config: &Self::Config,
        manifest_root: PathBuf,
        host_platform: Platform,
        _python_params: Option<PythonParams>,
        variants: &HashSet<NormalizedKey>,
        _channels: Vec<ChannelUrl>,
    ) -> miette::Result<GeneratedRecipe> {
        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), &mut DefaultMetadataProvider)
                .into_diagnostic()?;

        // we need to add compilers

        let requirements = &mut generated_recipe.recipe.requirements;

        let resolved_requirements = ConditionalRequirements::resolve(
            requirements.build.as_ref(),
            requirements.host.as_ref(),
            requirements.run.as_ref(),
            requirements.run_constraints.as_ref(),
            Some(host_platform),
        );

        // Get the list of compilers from config, defaulting to ["cxx"] if not specified
        let compilers = config
            .compilers
            .clone()
            .unwrap_or_else(|| vec!["cxx".to_string()]);

        // Add configured compilers to build requirements
        add_compilers_and_stdlib_to_requirements(
            &compilers,
            &mut requirements.build,
            &resolved_requirements.build,
            &host_platform,
            variants,
        );

        // add necessary build tools
        for tool in ["cmake", "ninja"] {
            let tool_name = PackageName::new_unchecked(tool);
            if !resolved_requirements.build.contains_key(&tool_name) {
                requirements.build.push(tool.parse().into_diagnostic()?);
            }
        }

        // Check if the host platform has a host python dependency
        // This is used to determine if we need to the cmake argument for the python
        // executable
        let has_host_python = resolved_requirements.contains(&PackageName::new_unchecked("python"));

        let build_script = BuildScriptContext {
            build_platform: if Platform::current().is_windows() {
                BuildPlatform::Windows
            } else {
                BuildPlatform::Unix
            },
            source_dir: manifest_root.display().to_string(),
            extra_args: config.extra_args.clone(),
            has_host_python,
        }
        .render();

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config.env.clone(),
            ..Default::default()
        };

        Ok(generated_recipe)
    }

    fn extract_input_globs_from_build(
        &self,
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> miette::Result<BTreeSet<String>> {
        Ok([
            // Source files
            "**/*.{c,cc,cxx,cpp,h,hpp,hxx}",
            // CMake files
            "**/*.{cmake,cmake.in}",
            "**/CMakeFiles.txt",
        ]
        .iter()
        .map(|s: &&str| s.to_string())
        .chain(config.extra_input_globs.clone())
        .collect())
    }

    fn default_variants(
        &self,
        host_platform: Platform,
    ) -> miette::Result<BTreeMap<NormalizedKey, Vec<Variable>>> {
        let mut variants = BTreeMap::new();

        if host_platform.is_windows() {
            // Default to the Visual Studio 2019 compiler on Windows
            //
            // rattler-build will default to vs2017 which for most github runners is too
            // old.
            variants.insert(NormalizedKey::from("cxx_compiler"), vec!["vs2019".into()]);
        }

        Ok(variants)
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(|log| {
        IntermediateBackendInstantiator::<CMakeGenerator>::new(log, Arc::default())
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
    use pixi_build_backend::protocol::ProtocolInstantiator;
    use pixi_build_types::{
        ProjectModelV1,
        procedures::{conda_outputs::CondaOutputsParams, initialize::InitializeParams},
    };
    use rattler_build::console_utils::LoggingOutputHandler;
    use recipe_stage0::recipe::{Item, Value};

    use super::*;

    #[test]
    fn test_input_globs_includes_extra_globs() {
        let config = CMakeBackendConfig {
            extra_input_globs: vec!["custom/*.c".to_string()],
            ..Default::default()
        };

        let generator = CMakeGenerator::default();

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
    fn test_cxx_is_in_build_requirements() {
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

        let generated_recipe = CMakeGenerator::default()
            .generate_recipe(
                &project_model,
                &CMakeBackendConfig::default(),
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
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

        let generated_recipe = CMakeGenerator::default()
            .generate_recipe(
                &project_model,
                &CMakeBackendConfig {
                    env: env.clone(),
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe.build.script,
        {
            ".content" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_has_python_is_set_in_build_script() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {
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

        let generated_recipe = CMakeGenerator::default()
            .generate_recipe(
                &project_model,
                &CMakeBackendConfig::default(),
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
            )
            .expect("Failed to generate recipe");

        // we want to check that
        // -DPython_EXECUTABLE=$PYTHON is set in the build script
        insta::assert_yaml_snapshot!(generated_recipe.recipe.build,

            {
            ".script.content" => insta::dynamic_redaction(|value, _path| {
                dbg!(&value);
                // assert that the value looks like a uuid here
                assert!(value.as_str().unwrap().lines()
                    .any(|c| c.contains("-DPython_EXECUTABLE"))
                );
                "[content]"
            })
        });
    }

    #[test]
    fn test_cxx_is_not_added_if_gcc_is_already_present() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {
                    "buildDependencies": {
                        "gxx": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let generated_recipe = CMakeGenerator::default()
            .generate_recipe(
                &project_model,
                &CMakeBackendConfig::default(),
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
            )
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script" => "[ ... script ... ]",
        });
    }

    #[tokio::test]
    async fn test_windows_default_compiler() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
        });

        let factory = IntermediateBackendInstantiator::<CMakeGenerator>::new(
            LoggingOutputHandler::default(),
            Arc::default(),
        )
        .initialize(InitializeParams {
            workspace_root: None,
            source_dir: None,
            manifest_path: PathBuf::from("pixi.toml"),
            project_model: Some(project_model.into()),
            configuration: None,
            target_configuration: None,
            cache_directory: None,
        })
        .await
        .unwrap();

        let current_dir = std::env::current_dir().unwrap();
        let outputs = factory
            .0
            .conda_outputs(CondaOutputsParams {
                channels: vec![],
                host_platform: Platform::Win64,
                build_platform: Platform::Win64,
                variant_configuration: None,
                variant_files: None,
                work_directory: current_dir,
            })
            .await
            .unwrap();

        assert_eq!(
            outputs.outputs[0]
                .metadata
                .variant
                .get("cxx_compiler")
                .map(String::as_str),
            Some("vs2019"),
            "On windows the default cxx_compiler variant should be vs2019"
        );
    }

    #[test]
    fn test_multiple_compilers_configuration() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
        });

        let generated_recipe = CMakeGenerator::default()
            .generate_recipe(
                &project_model,
                &CMakeBackendConfig {
                    compilers: Some(vec!["c".to_string(), "cxx".to_string(), "cuda".to_string()]),
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
            )
            .expect("Failed to generate recipe");

        // Check that we have exactly the expected compilers
        let build_reqs = &generated_recipe.recipe.requirements.build;
        let compiler_templates: Vec<String> = build_reqs
            .iter()
            .filter_map(|item| match item {
                Item::Value(Value::Template(s)) if s.contains("compiler") => Some(s.clone()),
                _ => None,
            })
            .collect();

        // Should have exactly three compilers
        assert_eq!(
            compiler_templates.len(),
            3,
            "Should have exactly three compilers"
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
        assert!(
            compiler_templates.contains(&"${{ compiler('cuda') }}".to_string()),
            "CUDA compiler should be in build requirements"
        );
    }

    #[test]
    fn test_default_compiler_when_not_specified() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
        });

        let generated_recipe = CMakeGenerator::default()
            .generate_recipe(
                &project_model,
                &CMakeBackendConfig {
                    compilers: None,
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::default(),
                vec![],
            )
            .expect("Failed to generate recipe");

        // Check that we have exactly the expected compilers and build tools
        let build_reqs = &generated_recipe.recipe.requirements.build;
        let compiler_templates: Vec<String> = build_reqs
            .iter()
            .filter_map(|item| match item {
                Item::Value(Value::Template(s)) if s.contains("compiler") => Some(s.clone()),
                _ => None,
            })
            .collect();

        // Should have exactly one compiler: cxx
        assert_eq!(
            compiler_templates.len(),
            1,
            "Should have exactly one compiler when not specified"
        );
        assert_eq!(
            compiler_templates[0], "${{ compiler('cxx') }}",
            "Default compiler should be cxx"
        );
    }

    #[test]
    fn test_stdlib_is_added() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
        });

        let generated_recipe = CMakeGenerator::default()
            .generate_recipe(
                &project_model,
                &CMakeBackendConfig {
                    compilers: None,
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::from_iter([NormalizedKey("c_stdlib".into())]),
                vec![],
            )
            .expect("Failed to generate recipe");

        // Check that we have exactly the expected compilers and build tools
        let build_reqs = &generated_recipe.recipe.requirements.build;
        let stdlib_templates: Vec<String> = build_reqs
            .iter()
            .filter_map(|item| match item {
                Item::Value(Value::Template(s)) if s.contains("stdlib") => Some(s.clone()),
                _ => None,
            })
            .collect();

        // Should have exactly one compiler: cxx
        assert_eq!(stdlib_templates.len(), 1, "Should have exactly one stdlib");
        assert_eq!(
            stdlib_templates[0], "${{ stdlib('c') }}",
            "Default stdlib should be c"
        );
    }
}
