mod build_script;
mod config;
mod metadata;

use build_script::{BuildPlatform, BuildScriptContext};
use config::RBackendConfig;
use metadata::DescriptionMetadataProvider;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    generated_recipe::{GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
    traits::ProjectModel,
};
use pixi_build_types::{ProjectModelV1, SourcePackageName};
use rattler_build::{recipe::variable::Variable, NormalizedKey};
use rattler_conda_types::{ChannelUrl, Platform};
use recipe_stage0::recipe::Script;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct RGenerator {}

impl RGenerator {
    /// Detect if package has native code requiring compilers
    fn detect_native_code(manifest_root: &Path) -> bool {
        let src_dir = manifest_root.join("src");
        src_dir.exists() && src_dir.is_dir()
    }

    /// Auto-detect required compilers based on package structure
    fn auto_detect_compilers(
        manifest_root: &Path,
        provider: &DescriptionMetadataProvider,
    ) -> miette::Result<Vec<String>> {
        let has_native = Self::detect_native_code(manifest_root);
        let has_linking = provider.has_linking_to().into_diagnostic()?;

        if !has_native && !has_linking {
            return Ok(Vec::new());
        }

        // Default to C, C++, and Fortran for packages with native code
        // This covers most R packages with compiled code
        Ok(vec![
            "c".to_string(),
            "cxx".to_string(),
            "fortran".to_string(),
        ])
    }
}

impl GenerateRecipe for RGenerator {
    type Config = RBackendConfig;

    fn generate_recipe(
        &self,
        model: &ProjectModelV1,
        config: &Self::Config,
        manifest_path: PathBuf,
        host_platform: Platform,
        _python_params: Option<PythonParams>,
        variants: &HashSet<NormalizedKey>,
        _channels: Vec<ChannelUrl>,
    ) -> miette::Result<GeneratedRecipe> {
        // Determine the manifest root
        let manifest_root = if manifest_path.is_file() {
            manifest_path.parent().ok_or_else(|| {
                miette::miette!(
                    "Manifest path {} has no parent",
                    manifest_path.display()
                )
            })?.to_path_buf()
        } else {
            manifest_path.clone()
        };

        let mut metadata_provider = DescriptionMetadataProvider::new(&manifest_root);

        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), &mut metadata_provider)
                .into_diagnostic()?;

        let requirements = &mut generated_recipe.recipe.requirements;
        let model_dependencies = model.dependencies(Some(host_platform));

        // Auto-detect or use configured compilers
        let compilers = match &config.compilers {
            Some(c) => c.clone(),
            None => Self::auto_detect_compilers(&manifest_root, &metadata_provider)?,
        };

        // Add compilers to build requirements
        pixi_build_backend::compilers::add_compilers_to_requirements(
            &compilers,
            &mut requirements.build,
            &model_dependencies,
            &host_platform,
        );
        pixi_build_backend::compilers::add_stdlib_to_requirements(
            &compilers,
            &mut requirements.build,
            variants,
        );

        // Add R runtime to host requirements
        let r_pkg = SourcePackageName::from("r-base");
        if !model_dependencies.host.contains_key(&r_pkg) {
            requirements
                .host
                .push("r-base".parse().into_diagnostic()?);
        }

        // Add R runtime to run requirements
        if !model_dependencies.run.contains_key(&r_pkg) {
            requirements.run.push("r-base".parse().into_diagnostic()?);
        }

        // Generate build script
        let has_native_code = !compilers.is_empty();
        let build_script = BuildScriptContext {
            build_platform: if Platform::current().is_windows() {
                BuildPlatform::Windows
            } else {
                BuildPlatform::Unix
            },
            source_dir: manifest_root.display().to_string(),
            extra_args: config.extra_args.clone(),
            has_native_code,
        }
        .render();

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config.env.clone(),
            ..Default::default()
        };

        // Add metadata input globs
        generated_recipe
            .metadata_input_globs
            .extend(metadata_provider.input_globs());

        Ok(generated_recipe)
    }

    fn extract_input_globs_from_build(
        &self,
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> miette::Result<BTreeSet<String>> {
        let mut globs = BTreeSet::from(
            [
                // R package structure files
                "DESCRIPTION",
                "NAMESPACE",
                "**/*.R",  // R source files
                "**/*.Rd", // R documentation
            ]
            .map(String::from),
        );

        // Add compiler-specific globs if compilers are configured
        if let Some(compilers) = &config.compilers {
            for compiler in compilers {
                match compiler.as_str() {
                    "c" => {
                        globs.insert("**/*.c".to_string());
                        globs.insert("**/*.h".to_string());
                    }
                    "cxx" => {
                        globs.insert("**/*.cpp".to_string());
                        globs.insert("**/*.cc".to_string());
                        globs.insert("**/*.cxx".to_string());
                        globs.insert("**/*.hpp".to_string());
                        globs.insert("**/*.hxx".to_string());
                    }
                    "fortran" => {
                        globs.insert("**/*.f".to_string());
                        globs.insert("**/*.f90".to_string());
                        globs.insert("**/*.f95".to_string());
                    }
                    _ => {}
                }
            }
        }

        // Add extra globs from config
        globs.extend(config.extra_input_globs.clone());

        Ok(globs)
    }

    fn default_variants(
        &self,
        _host_platform: Platform,
    ) -> miette::Result<BTreeMap<NormalizedKey, Vec<Variable>>> {
        // R packages don't typically need special default variants
        // Compiler variants are handled by rattler-build defaults
        Ok(BTreeMap::new())
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(|log| {
        IntermediateBackendInstantiator::<RGenerator>::new(log, Arc::default())
    })
    .await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recipe_stage0::recipe::{Item, Value};
    use tempfile::TempDir;
    use tokio::fs;

    #[macro_export]
    macro_rules! project_fixture {
        ($($json:tt)+) => {
            serde_json::from_value::<ProjectModelV1>(
                serde_json::json!($($json)+)
            ).expect("Failed to create project model from JSON")
        };
    }

    #[tokio::test]
    async fn test_r_package_with_native_code() {
        let temp_dir = TempDir::new().unwrap();

        // Create DESCRIPTION file
        fs::write(
            temp_dir.path().join("DESCRIPTION"),
            r#"Package: testpkg
Version: 1.0.0
Title: Test Package
Description: A test package
License: GPL-3
LinkingTo: Rcpp
"#,
        )
        .await
        .unwrap();

        // Create src directory to trigger native code detection
        fs::create_dir(temp_dir.path().join("src"))
            .await
            .unwrap();

        let project_model = project_fixture!({
            "name": "r-testpkg",
            "version": "1.0.0",
            "targets": {
                "defaultTarget": {}
            }
        });

        let generated_recipe = RGenerator::default()
            .generate_recipe(
                &project_model,
                &RBackendConfig::default(),
                temp_dir.path().to_path_buf(),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
            )
            .expect("Failed to generate recipe");

        // Verify compilers were added
        let build_reqs = &generated_recipe.recipe.requirements.build;
        let has_compilers = build_reqs.iter().any(|item| {
            matches!(item, Item::Value(Value::Template(t)) if t.contains("compiler"))
        });
        assert!(
            has_compilers,
            "Native code package should have compilers"
        );

        // Verify r-base is in host and run requirements
        let host_reqs = &generated_recipe.recipe.requirements.host;
        let has_r_base_host = host_reqs
            .iter()
            .any(|item| matches!(item, Item::Value(Value::String(s)) if s == "r-base"));
        assert!(has_r_base_host, "Should have r-base in host requirements");

        let run_reqs = &generated_recipe.recipe.requirements.run;
        let has_r_base_run = run_reqs
            .iter()
            .any(|item| matches!(item, Item::Value(Value::String(s)) if s == "r-base"));
        assert!(has_r_base_run, "Should have r-base in run requirements");

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
            ".source[0].path" => "[path]",
            ".build.script.content" => "[build_script]",
        });
    }

    #[tokio::test]
    async fn test_pure_r_package_no_compilers() {
        let temp_dir = TempDir::new().unwrap();

        fs::write(
            temp_dir.path().join("DESCRIPTION"),
            "Package: purepkg\nVersion: 1.0.0\nTitle: Pure R Package\n",
        )
        .await
        .unwrap();

        let project_model = project_fixture!({
            "name": "r-purepkg",
            "version": "1.0.0",
            "targets": {
                "defaultTarget": {}
            }
        });

        let generated_recipe = RGenerator::default()
            .generate_recipe(
                &project_model,
                &RBackendConfig::default(),
                temp_dir.path().to_path_buf(),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
            )
            .expect("Failed to generate recipe");

        // Verify no compilers were added for pure R package
        let build_reqs = &generated_recipe.recipe.requirements.build;
        let has_compilers = build_reqs.iter().any(|item| {
            matches!(item, Item::Value(Value::Template(t)) if t.contains("compiler"))
        });
        assert!(
            !has_compilers,
            "Pure R package should not have compilers"
        );

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
            ".source[0].path" => "[path]",
            ".build.script.content" => "[build_script]",
        });
    }

    #[test]
    fn test_input_globs_for_r_package() {
        let config = RBackendConfig {
            compilers: Some(vec!["c".to_string(), "cxx".to_string()]),
            ..Default::default()
        };

        let generator = RGenerator::default();
        let globs = generator
            .extract_input_globs_from_build(&config, PathBuf::new(), false)
            .unwrap();

        assert!(globs.contains("DESCRIPTION"));
        assert!(globs.contains("NAMESPACE"));
        assert!(globs.contains("**/*.R"));
        assert!(globs.contains("**/*.c"));
        assert!(globs.contains("**/*.cpp"));
    }

    #[test]
    fn test_input_globs_with_extra_globs() {
        let config = RBackendConfig {
            extra_input_globs: vec!["inst/**/*".to_string()],
            ..Default::default()
        };

        let generator = RGenerator::default();
        let globs = generator
            .extract_input_globs_from_build(&config, PathBuf::new(), false)
            .unwrap();

        assert!(globs.contains("inst/**/*"));
    }

    #[tokio::test]
    async fn test_explicit_compilers_override() {
        let temp_dir = TempDir::new().unwrap();

        fs::write(
            temp_dir.path().join("DESCRIPTION"),
            "Package: testpkg\nVersion: 1.0.0\n",
        )
        .await
        .unwrap();

        // Create src directory (would normally trigger auto-detection)
        fs::create_dir(temp_dir.path().join("src"))
            .await
            .unwrap();

        let project_model = project_fixture!({
            "name": "r-testpkg",
            "version": "1.0.0",
            "targets": {
                "defaultTarget": {}
            }
        });

        // Explicitly specify only C compiler
        let config = RBackendConfig {
            compilers: Some(vec!["c".to_string()]),
            ..Default::default()
        };

        let generated_recipe = RGenerator::default()
            .generate_recipe(
                &project_model,
                &config,
                temp_dir.path().to_path_buf(),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
            )
            .expect("Failed to generate recipe");

        // Verify only one compiler template was added
        let build_reqs = &generated_recipe.recipe.requirements.build;
        let compiler_count = build_reqs
            .iter()
            .filter(|item| {
                matches!(item, Item::Value(Value::Template(t)) if t.contains("compiler('c')"))
            })
            .count();

        assert_eq!(
            compiler_count, 1,
            "Should have exactly one compiler when explicitly set"
        );
    }
}
