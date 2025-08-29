use std::{collections::BTreeSet, path::PathBuf, str::FromStr};

use miette::Diagnostic;
use once_cell::unsync::OnceCell;
use pixi_build_backend::generated_recipe::MetadataProvider;
use pyproject_toml::PyProjectToml;
use rattler_conda_types::{ParseVersionError, Version};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum MetadataError {
    #[error("failed to parse pyproject.toml, {0}")]
    PyProjectToml(#[from] toml_edit::de::Error),
    #[error("failed to parse version from pyproject.toml, {0}")]
    ParseVersion(ParseVersionError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// An implementation of [`MetadataProvider`] that reads metadata from a
/// pyproject.toml file.
pub struct PyprojectMetadataProvider {
    manifest_root: PathBuf,
    pyproject_manifest: OnceCell<PyProjectToml>,
    ignore_pyproject_manifest: bool,
}

impl PyprojectMetadataProvider {
    /// Constructs a new `PyprojectMetadataProvider` with the given manifest root.
    ///
    /// # Arguments
    ///
    /// * `manifest_root` - The directory that contains the `pyproject.toml` file
    /// * `ignore_pyproject_manifest` - If `true`, all metadata methods will return
    ///   `None`, effectively disabling pyproject.toml metadata extraction
    pub fn new(manifest_root: impl Into<PathBuf>, ignore_pyproject_manifest: bool) -> Self {
        Self {
            manifest_root: manifest_root.into(),
            pyproject_manifest: OnceCell::default(),
            ignore_pyproject_manifest,
        }
    }

    /// Ensures that the manifest is loaded and returns the project metadata.
    fn ensure_manifest_project(&self) -> Result<Option<&pyproject_toml::Project>, MetadataError> {
        Ok(self.ensure_manifest()?.project.as_ref())
    }

    /// Ensures that the manifest is loaded
    fn ensure_manifest(&self) -> Result<&PyProjectToml, MetadataError> {
        self.pyproject_manifest.get_or_try_init(move || {
            let pyproject_toml_content =
                fs_err::read_to_string(self.manifest_root.join("pyproject.toml"))?;
            toml_edit::de::from_str(&pyproject_toml_content).map_err(MetadataError::PyProjectToml)
        })
    }

    /// Returns the set of globs that match files that influence the metadata of
    /// this package.
    ///
    /// This includes the package's own `pyproject.toml` file. These globs
    /// can be used for incremental builds to determine when metadata might
    /// have changed.
    ///
    /// # Returns
    ///
    /// A `BTreeSet` of glob patterns as strings. Common patterns include:
    /// - `"pyproject.toml"` - The package's manifest file
    pub fn input_globs(&self) -> BTreeSet<String> {
        let mut input_globs = BTreeSet::new();

        let Some(_) = self.pyproject_manifest.get() else {
            return input_globs;
        };

        // Add the pyproject.toml manifest file itself.
        input_globs.insert(String::from("pyproject.toml"));

        input_globs
    }
}

impl MetadataProvider for PyprojectMetadataProvider {
    type Error = MetadataError;

    /// Returns the package name from the pyproject.toml manifest.
    ///
    /// If `ignore_pyproject_manifest` is true, returns `None`. Otherwise, extracts
    /// the name from the project section of the pyproject.toml file.
    fn name(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_pyproject_manifest {
            return Ok(None);
        }
        Ok(self
            .ensure_manifest_project()?
            .map(|proj| proj.name.clone()))
    }

    /// Returns the package version from the pyproject.toml manifest.
    ///
    /// If `ignore_pyproject_manifest` is true, returns `None`. Otherwise, extracts
    /// the version from the project section. The version string is parsed into a
    /// `rattler_conda_types::Version`.
    fn version(&mut self) -> Result<Option<Version>, Self::Error> {
        if self.ignore_pyproject_manifest {
            return Ok(None);
        }
        let Some(project) = self.ensure_manifest_project()? else {
            return Ok(None);
        };
        let Some(version) = &project.version else {
            return Ok(None);
        };
        Ok(Some(
            Version::from_str(&version.to_string()).map_err(MetadataError::ParseVersion)?,
        ))
    }

    /// Returns the package description from the pyproject.toml manifest.
    ///
    /// If `ignore_pyproject_manifest` is true, returns `None`. Otherwise, extracts
    /// the description from the project section.
    fn description(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_pyproject_manifest {
            return Ok(None);
        }
        Ok(self
            .ensure_manifest_project()?
            .and_then(|proj| proj.description.clone()))
    }

    /// Returns the package homepage URL from the pyproject.toml manifest.
    ///
    /// If `ignore_pyproject_manifest` is true, returns `None`. Otherwise, extracts
    /// the homepage from the project.urls section.
    fn homepage(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_pyproject_manifest {
            return Ok(None);
        }
        Ok(self
            .ensure_manifest_project()?
            .and_then(|proj| proj.urls.as_ref())
            .and_then(|urls| urls.get("Homepage").cloned()))
    }

    /// Returns the package license from the pyproject.toml manifest.
    ///
    /// If `ignore_pyproject_manifest` is true, returns `None`. Otherwise, extracts
    /// the license from the project section.
    fn license(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_pyproject_manifest {
            return Ok(None);
        }
        Ok(self
            .ensure_manifest_project()?
            .and_then(|proj| proj.license.as_ref())
            .map(|license| match license {
                pyproject_toml::License::Text { text } => text.clone(),
                pyproject_toml::License::File { file } => file.to_string_lossy().to_string(),
                pyproject_toml::License::Spdx(spdx) => spdx.clone(),
            }))
    }

    /// Returns the package license file path from the pyproject.toml manifest.
    ///
    /// If `ignore_pyproject_manifest` is true, returns `None`. Otherwise, extracts
    /// the license file path from the project section if the license is specified
    /// as a file reference.
    fn license_file(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_pyproject_manifest {
            return Ok(None);
        }
        Ok(self
            .ensure_manifest_project()?
            .and_then(|proj| proj.license.as_ref())
            .and_then(|license| match license {
                pyproject_toml::License::File { file } => Some(file.to_string_lossy().to_string()),
                pyproject_toml::License::Text { text: _ } => None,
                pyproject_toml::License::Spdx(_) => None,
            }))
    }

    /// Returns the package summary from the pyproject.toml manifest.
    ///
    /// This returns the same as description since pyproject.toml doesn't have
    /// a separate summary field.
    fn summary(&mut self) -> Result<Option<String>, Self::Error> {
        self.description()
    }

    /// Returns the package documentation URL from the pyproject.toml manifest.
    ///
    /// If `ignore_pyproject_manifest` is true, returns `None`. Otherwise, extracts
    /// the documentation URL from the project.urls section.
    fn documentation(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_pyproject_manifest {
            return Ok(None);
        }
        Ok(self
            .ensure_manifest_project()?
            .and_then(|proj| proj.urls.as_ref())
            .and_then(|urls| {
                urls.get("Documentation")
                    .or_else(|| urls.get("Docs"))
                    .cloned()
            }))
    }

    /// Returns the package repository URL from the pyproject.toml manifest.
    ///
    /// If `ignore_pyproject_manifest` is true, returns `None`. Otherwise, extracts
    /// the repository URL from the project.urls section.
    fn repository(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_pyproject_manifest {
            return Ok(None);
        }
        Ok(self
            .ensure_manifest_project()?
            .and_then(|proj| proj.urls.as_ref())
            .and_then(|urls| {
                urls.get("Repository")
                    .or_else(|| urls.get("Source"))
                    .or_else(|| urls.get("Source Code"))
                    .cloned()
            }))
    }
}

impl PyprojectMetadataProvider {
    /// Returns the required Python version from the pyproject.toml manifest.
    ///
    /// If `ignore_pyproject_manifest` is true, returns `None`. Otherwise, extracts
    /// the requires-python from the project section.
    pub fn requires_python(&self) -> Result<Option<String>, MetadataError> {
        if self.ignore_pyproject_manifest {
            return Ok(None);
        }
        Ok(self
            .ensure_manifest_project()?
            .and_then(|proj| proj.requires_python.as_ref())
            .map(|req_py| req_py.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fs};

    use pixi_build_backend::generated_recipe::{GenerateRecipe, MetadataProvider};
    use rattler_conda_types::Platform;
    use tempfile::TempDir;

    use crate::{PythonGenerator, config::PythonBackendConfig, project_fixture};
    use pixi_build_types::ProjectModelV1;

    use super::*;

    /// Helper function to create a temporary directory with a pyproject.toml file
    fn create_temp_pyproject_project(pyproject_toml_content: &str) -> TempDir {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let pyproject_toml_path = temp_dir.path().join("pyproject.toml");
        fs::write(pyproject_toml_path, pyproject_toml_content)
            .expect("Failed to write pyproject.toml");
        temp_dir
    }

    /// Helper function to create a PyprojectMetadataProvider for testing
    fn create_metadata_provider(manifest_root: &std::path::Path) -> PyprojectMetadataProvider {
        PyprojectMetadataProvider::new(manifest_root, false)
    }

    #[test]
    fn test_basic_metadata_extraction() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "1.0.0"
description = "A test package"
license = {text = "MIT"}

[project.urls]
Homepage = "https://example.com"
Repository = "https://github.com/example/test-package"
Documentation = "https://docs.example.com"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = create_metadata_provider(temp_dir.path());

        assert_eq!(provider.name().unwrap(), Some("test-package".to_string()));
        assert_eq!(provider.version().unwrap().unwrap().to_string(), "1.0.0");
        assert_eq!(
            provider.description().unwrap(),
            Some("A test package".to_string())
        );
        assert_eq!(provider.license().unwrap(), Some("MIT".to_string()));
        assert_eq!(
            provider.homepage().unwrap(),
            Some("https://example.com".to_string())
        );
        assert_eq!(
            provider.repository().unwrap(),
            Some("https://github.com/example/test-package".to_string())
        );
        assert_eq!(
            provider.documentation().unwrap(),
            Some("https://docs.example.com".to_string())
        );
    }

    #[test]
    fn test_license_from_file() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "1.0.0"
license = {file = "LICENSE.txt"}
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = create_metadata_provider(temp_dir.path());

        assert_eq!(provider.license().unwrap(), Some("LICENSE.txt".to_string()));
        assert_eq!(
            provider.license_file().unwrap(),
            Some("LICENSE.txt".to_string())
        );
    }

    #[test]
    fn test_missing_project_section() {
        let pyproject_toml_content = r#"
[build-system]
requires = ["setuptools", "wheel"]
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = create_metadata_provider(temp_dir.path());

        assert_eq!(provider.name().unwrap(), None);
        assert_eq!(provider.version().unwrap(), None);
        assert_eq!(provider.description().unwrap(), None);
    }

    #[test]
    fn test_input_globs() {
        let pyproject_toml_content = r#"
    [project]
    name = "test-package"
    version = "1.0.0"
    "#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = create_metadata_provider(temp_dir.path());

        // Force loading of manifest
        let _ = provider.name().unwrap();

        let globs = provider.input_globs();
        assert_eq!(globs.len(), 1);
        assert!(globs.contains("pyproject.toml"));
    }

    #[test]
    fn test_ignore_pyproject_manifest_flag() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "1.0.0"
description = "Test description"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = PyprojectMetadataProvider::new(temp_dir.path(), true);

        // All methods should return None when ignore_pyproject_manifest is true
        assert_eq!(provider.name().unwrap(), None);
        assert_eq!(provider.version().unwrap(), None);
        assert_eq!(provider.description().unwrap(), None);
        assert_eq!(provider.license().unwrap(), None);
        assert_eq!(provider.homepage().unwrap(), None);
        assert_eq!(provider.repository().unwrap(), None);
        assert_eq!(provider.documentation().unwrap(), None);
        assert_eq!(provider.license_file().unwrap(), None);
        assert_eq!(provider.summary().unwrap(), None);
        assert_eq!(provider.requires_python().unwrap(), None);
    }

    #[test]
    fn test_alternative_url_keys() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "1.0.0"

[project.urls]
"Source Code" = "https://github.com/example/test-package"
Docs = "https://docs.example.com"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = create_metadata_provider(temp_dir.path());

        assert_eq!(
            provider.repository().unwrap(),
            Some("https://github.com/example/test-package".to_string())
        );
        assert_eq!(
            provider.documentation().unwrap(),
            Some("https://docs.example.com".to_string())
        );
    }

    #[test]
    fn test_invalid_version_format() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "1.0.0a1"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = create_metadata_provider(temp_dir.path());

        // This should parse successfully since it's a valid PEP440 version
        let result = provider.version();
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_pyproject_toml_parse_error() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "not.a.valid.version.at.all"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = create_metadata_provider(temp_dir.path());

        let result = provider.version();
        // The pyproject-toml parser should fail to parse this
        match result {
            Err(MetadataError::PyProjectToml(_)) => {
                // This is expected - invalid version in pyproject.toml
            }
            other => panic!(
                "Expected PyProjectTomlError for invalid version, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_malformed_pyproject_toml() {
        let pyproject_toml_content = r#"
[project
name = "test-package"
version = "1.0.0"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = create_metadata_provider(temp_dir.path());

        let result = provider.name();
        assert!(result.is_err());
        match result.unwrap_err() {
            MetadataError::PyProjectToml(_) => {}
            err => panic!("Expected PyProjectToml, got: {:?}", err),
        }
    }

    #[test]
    fn test_summary_equals_description() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "1.0.0"
description = "Test description"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let mut provider = create_metadata_provider(temp_dir.path());

        let description = provider.description().unwrap();
        let summary = provider.summary().unwrap();

        assert_eq!(description, summary);
        assert_eq!(summary, Some("Test description".to_string()));
    }

    #[test]
    fn test_requires_python_extraction() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "1.0.0"
requires-python = ">=3.13"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let provider = create_metadata_provider(temp_dir.path());

        assert_eq!(
            provider.requires_python().unwrap(),
            Some(">=3.13".to_string())
        );
    }

    #[test]
    fn test_requires_python_with_ignore_flag() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "1.0.0"
requires-python = ">=3.13"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        let provider = PyprojectMetadataProvider::new(temp_dir.path(), true);

        assert_eq!(provider.requires_python().unwrap(), None);
    }

    #[test]
    fn test_generated_recipe_contains_pyproject_values() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "99.0.0"
description = "A test package"
license = {text = "MIT"}

[project.urls]
Homepage = "https://example.com"
Repository = "https://github.com/example/test-package"
Documentation = "https://docs.example.com"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);
        // let mut provider = create_metadata_provider(temp_dir.path());

        // Now create project model and generate a recipe from it
        let project_model = project_fixture!({
            "name": "foobar",
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
                // when using the default here we should read values from the pyproject.toml
                &PythonBackendConfig::default(),
                temp_dir.path().to_path_buf(),
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
    fn test_generated_recipe_respects_requires_python() {
        let pyproject_toml_content = r#"
[project]
name = "test-package"
version = "1.0.0"
requires-python = ">=3.13"
"#;

        let temp_dir = create_temp_pyproject_project(pyproject_toml_content);

        // Now create project model and generate a recipe from it
        let project_model = project_fixture!({
            "name": "foobar",
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
                // when using the default here we should read values from the pyproject.toml
                &PythonBackendConfig::default(),
                temp_dir.path().to_path_buf(),
                Platform::Linux64,
                None,
                &HashSet::new(),
            )
            .expect("Failed to generate recipe");

        // Check that Python requirements include the version constraint
        let host_requirements = &generated_recipe.recipe.requirements.host;
        let run_requirements = &generated_recipe.recipe.requirements.run;

        let has_python_constraint_host = host_requirements
            .iter()
            .any(|req| req.to_string().starts_with("python >=3.13"));
        let has_python_constraint_run = run_requirements
            .iter()
            .any(|req| req.to_string().starts_with("python >=3.13"));

        assert!(
            has_python_constraint_host,
            "Host requirements should include 'python >=3.13', found: {:?}",
            host_requirements
        );
        assert!(
            has_python_constraint_run,
            "Run requirements should include 'python >=3.13', found: {:?}",
            run_requirements
        );
    }
}
