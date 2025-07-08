use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use crate::{
    build_script::{BuildPlatform, BuildScriptContext, Installer},
    config::PythonBackendConfig,
};
use miette::IntoDiagnostic;
use pixi_build_backend::common::{PackageRequirements, SourceRequirements};
use pixi_build_backend::{
    ProjectModel, Targets,
    common::{BuildConfigurationParams, requirements},
    traits::{Dependencies, project::new_spec},
};
use pyproject_toml::PyProjectToml;
use rattler_build::recipe::parser::{BuildString, GlobVec};
use rattler_build::{
    NormalizedKey,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{BuildConfiguration, Debug, PackagingSettings},
    recipe::{
        Recipe,
        parser::{Build, Package, PathSource, Python, Script, ScriptContent, Source},
        variable::Variable,
    },
};
use rattler_conda_types::{
    NoArchType, PackageName, Platform,
    package::{ArchiveType, EntryPoint},
};
use rattler_package_streaming::write::CompressionLevel;

#[derive(Debug)]
pub struct PythonBuildBackend<P: ProjectModel> {
    pub(crate) logging_output_handler: LoggingOutputHandler,
    pub(crate) manifest_path: PathBuf,
    pub(crate) manifest_root: PathBuf,
    pub(crate) project_model: P,
    pub(crate) config: PythonBackendConfig,
    pub(crate) cache_dir: Option<PathBuf>,
    pub(crate) pyproject_manifest: Option<PyProjectToml>,
}

impl<P: ProjectModel> PythonBuildBackend<P> {
    /// Returns a new instance of [`PythonBuildBackend`] by reading the manifest
    /// at the given path.
    pub fn new(
        manifest_path: PathBuf,
        project_model: P,
        config: PythonBackendConfig,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
    ) -> miette::Result<Self> {
        // Determine the root directory of the manifest
        let manifest_root = manifest_path
            .parent()
            .ok_or_else(|| miette::miette!("the project manifest must reside in a directory"))?
            .to_path_buf();

        let pyproject_manifest = {
            let pyproject_path = manifest_path.with_file_name("pyproject.toml");
            if pyproject_path.exists() {
                let contents = std::fs::read_to_string(&pyproject_path).into_diagnostic()?;
                Some(toml_edit::de::from_str(&contents).into_diagnostic()?)
            } else {
                None
            }
        };

        Ok(Self {
            manifest_path,
            manifest_root,
            project_model,
            config,
            logging_output_handler,
            cache_dir,
            pyproject_manifest,
        })
    }

    /// Read the entry points from the pyproject.toml and return them as a list.
    ///
    /// If the manifest is not a pyproject.toml file no entry-points are added.
    pub(crate) fn entry_points(&self) -> Vec<EntryPoint> {
        let scripts = self
            .pyproject_manifest
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

    /// Constructs a [`Recipe`] that will build the python package into a conda
    /// package.
    ///
    /// If the package is editable, the recipe will not include the source but
    /// only references to the original source files.
    ///
    /// Script entry points are read from the pyproject and added as entry
    /// points in the conda package.
    pub(crate) fn recipe(
        &self,
        host_platform: Platform,
        editable: bool,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<(Recipe, SourceRequirements<P>)> {
        // TODO: remove this env var override as soon as we have profiles
        let editable = std::env::var("BUILD_EDITABLE_PYTHON")
            .map(|val| val == "true")
            .unwrap_or(editable);

        // Parse the package name and version from the manifest
        let name = PackageName::from_str(self.project_model.name()).into_diagnostic()?;
        let version = self.project_model.version().clone().ok_or_else(|| {
            miette::miette!("a version is missing from the package but it is required")
        })?;

        // Determine whether the package should be built as a noarch package or as a
        // generic package.
        let noarch_type = if self.config.noarch() {
            NoArchType::python()
        } else {
            NoArchType::none()
        };

        // Construct python specific settings
        let python = Python {
            entry_points: self.entry_points(),
            ..Python::default()
        };

        let (installer, requirements) = self.requirements(host_platform, variant)?;

        // Create a build script
        let build_platform = Platform::current();
        let build_number = 0;

        let build_script = BuildScriptContext {
            installer,
            build_platform: if build_platform.is_windows() {
                BuildPlatform::Windows
            } else {
                BuildPlatform::Unix
            },
            editable,
            manifest_root: self.manifest_root.clone(),
        }
        .render();

        // Define the sources of the package.
        let source = if editable {
            // In editable mode we don't include the source in the package, the package will
            // refer back to the original source.
            Vec::new()
        } else {
            Vec::from([Source::Path(PathSource {
                // TODO: How can we use a git source?
                path: self.manifest_root.clone(),
                sha256: None,
                md5: None,
                patches: vec![],
                target_directory: None,
                file_name: None,
                use_gitignore: true,
                filter: GlobVec::from_vec(Vec::new(), Some(Vec::from([".pixi"]))),
            })])
        };

        let hash_info = HashInfo::from_variant(variant, &noarch_type);

        Ok((
            Recipe {
                schema_version: 1,
                package: Package {
                    version: version.into(),
                    name,
                },
                context: Default::default(),
                cache: None,
                source,
                build: Build {
                    number: build_number,
                    string: BuildString::Resolved(BuildString::compute(&hash_info, build_number)),

                    // skip: Default::default(),
                    script: Script {
                        content: ScriptContent::Commands(build_script),
                        env: self.config.env.clone(),
                        ..Default::default()
                    },
                    noarch: noarch_type,

                    python,
                    // dynamic_linking: Default::default(),
                    // always_copy_files: Default::default(),
                    // always_include_files: Default::default(),
                    // merge_build_and_host_envs: false,
                    // variant: Default::default(),
                    // prefix_detection: Default::default(),
                    // post_process: vec![],
                    // files: Default::default(),
                    ..Build::default()
                },
                requirements: requirements.requirements,
                tests: vec![],
                about: Default::default(),
                extra: Default::default(),
            },
            requirements.source,
        ))
    }

    pub(crate) fn requirements(
        &self,
        host_platform: Platform,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<(Installer, PackageRequirements<P>)> {
        let dependencies = self.project_model.dependencies(Some(host_platform));

        let empty_spec = new_spec::<P>();

        let installer = installer_tool::<P>(&dependencies);

        let installer_name = installer.package_name().to_string();
        let python_name = "python".to_string();

        let dependencies =
            add_build_tools::<P>(dependencies, &installer_name, &python_name, &empty_spec);

        Ok((installer, requirements::<P>(dependencies, variant)?))
    }
}

/// Construct a build configuration for the given recipe and parameters.
pub(crate) fn construct_configuration(
    recipe: &Recipe,
    params: BuildConfigurationParams,
) -> BuildConfiguration {
    BuildConfiguration {
        target_platform: if recipe.build.noarch == NoArchType::none() {
            params.host_platform.platform
        } else {
            Platform::NoArch
        },
        host_platform: params.host_platform,
        build_platform: params.build_platform,
        hash: HashInfo::from_variant(&params.variant, &recipe.build.noarch),
        variant: params.variant,
        directories: params.directories,
        channels: params.channels,
        channel_priority: Default::default(),
        solve_strategy: Default::default(),
        timestamp: chrono::Utc::now(),
        subpackages: Default::default(), // TODO: ???
        packaging_settings: PackagingSettings::from_args(
            ArchiveType::Conda,
            CompressionLevel::default(),
        ),
        store_recipe: false,
        force_colors: true,
        sandbox_config: None,
        // TODO: Expose to the user
        debug: Debug::new(false),
    }
}

/// Return the installer to be used and the build tools for the given project model.
pub(crate) fn installer_tool<P: ProjectModel>(
    dependencies: &Dependencies<<P::Targets as Targets>::Spec>,
) -> Installer {
    Installer::determine_installer::<P>(dependencies)
}

/// Add the build tools to the dependencies if they are not already present.
pub(crate) fn add_build_tools<'a, P: ProjectModel>(
    mut dependencies: Dependencies<'a, <P::Targets as Targets>::Spec>,
    installer_name: &'a String,
    python_name: &'a String,
    empty_spec: &'a <<P as ProjectModel>::Targets as Targets>::Spec,
) -> Dependencies<'a, <<P as ProjectModel>::Targets as Targets>::Spec> {
    // we always add installer as a host dependency
    if !dependencies.host.contains_key(installer_name) {
        dependencies.host.insert(installer_name, empty_spec);
    }

    // and `python` as both host and run dependency
    if !dependencies.host.contains_key(python_name) {
        dependencies.host.insert(python_name, empty_spec);
    }

    if !dependencies.run.contains_key(python_name) {
        dependencies.run.insert(python_name, empty_spec);
    }

    dependencies
}

#[cfg(test)]
mod tests {

    use std::collections::BTreeMap;

    use indexmap::IndexMap;
    use pixi_build_type_conversions::to_project_model_v1;

    use pixi_manifest::Manifests;
    use rattler_build::{console_utils::LoggingOutputHandler, recipe::Recipe};
    use rattler_conda_types::{ChannelConfig, Platform};
    use tempfile::tempdir;

    use crate::{config::PythonBackendConfig, python::PythonBuildBackend};

    fn recipe(manifest_source: &str, config: PythonBackendConfig) -> Recipe {
        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");
        std::fs::write(&tmp_manifest, manifest_source).unwrap();
        let manifest = Manifests::from_workspace_manifest_path(tmp_manifest.clone()).unwrap();
        let package = manifest.value.package.unwrap();
        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
        let project_model = to_project_model_v1(&package.value, &channel_config).unwrap();

        let python_backend = PythonBuildBackend::new(
            tmp_manifest,
            project_model,
            config,
            LoggingOutputHandler::default(),
            None,
        )
        .unwrap();

        python_backend
            .recipe(Platform::current(), false, &BTreeMap::new())
            .unwrap()
            .0
    }

    #[test]
    fn test_noarch_none() {
        insta::assert_yaml_snapshot!(recipe(r#"
        [workspace]
        platforms = []
        channels = []
        preview = ["pixi-build"]

        [package]
        name = "foobar"
        version = "0.1.0"

        [package.build]
        backend = { name = "pixi-build-python", version = "*" }
        "#, PythonBackendConfig {
            noarch: Some(false),
            ..Default::default()
        }), {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_noarch_python() {
        insta::assert_yaml_snapshot!(recipe(r#"
        [workspace]
        platforms = []
        channels = []
        preview = ["pixi-build"]

        [package]
        name = "foobar"
        version = "0.1.0"

        [package.build]
        backend = { name = "pixi-build-python", version = "*" }
        "#, PythonBackendConfig::default()), {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });
    }

    #[tokio::test]
    async fn test_setting_host_and_build_requirements() {
        let package_with_host_and_build_deps = r#"
        [workspace]
        name = "test-reqs"
        channels = ["conda-forge"]
        platforms = ["osx-arm64"]
        preview = ["pixi-build"]

        [package]
        name = "test-reqs"
        version = "1.2.3"

        [package.host-dependencies]
        hatchling = "*"

        [package.build-dependencies]
        boltons = "*"

        [package.run-dependencies]
        foobar = ">=3.2.1"
        source = { path = "src" }

        [package.build]
        backend = { name = "pixi-build-python", version = "*" }
        "#;

        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");

        // write the raw string into the file
        std::fs::write(&tmp_manifest, package_with_host_and_build_deps).unwrap();

        let manifest = Manifests::from_workspace_manifest_path(tmp_manifest.clone()).unwrap();
        let package = manifest.value.package.unwrap();
        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
        let project_model = to_project_model_v1(&package.value, &channel_config).unwrap();
        let python_backend = PythonBuildBackend::new(
            package.provenance.path,
            project_model,
            PythonBackendConfig::default(),
            LoggingOutputHandler::default(),
            None,
        )
        .unwrap();

        let host_platform = Platform::current();

        let (recipe, source_requirements) = python_backend
            .recipe(host_platform, false, &BTreeMap::new())
            .unwrap();
        insta::assert_yaml_snapshot!(recipe, {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });

        insta::assert_yaml_snapshot!(source_requirements);
    }

    #[tokio::test]
    async fn test_scripts_are_respected() {
        let package_with_host_and_build_deps = r#"
        [workspace]
        name = "test-scripts"
        channels = ["conda-forge"]
        platforms = ["osx-arm64"]
        preview = ["pixi-build"]

        [package]
        name = "test-scripts"
        version = "1.2.3"

        [package.build]
        backend = { name = "pixi-build-python", version = "*" }
        "#;

        let pyproject_toml_with_scripts = r#"
        [project]
dependencies = ["rich"]
name = "rich_example"
requires-python = ">= 3.11"
scripts = { rich-example-main = "rich_example:main" }
version = "0.1.0"

[build-system]
build-backend = "hatchling.build"
requires = ["hatchling"]
"#;

        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");

        // write the raw pixi toml into the file
        std::fs::write(&tmp_manifest, package_with_host_and_build_deps).unwrap();

        let tmp_pyproject_toml = tmp_dir.path().join("pyproject.toml");

        // write the raw pixi toml into the file
        std::fs::write(&tmp_pyproject_toml, pyproject_toml_with_scripts).unwrap();

        let manifest = Manifests::from_workspace_manifest_path(tmp_manifest.clone()).unwrap();
        let package = manifest.value.package.unwrap();
        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
        let project_model = to_project_model_v1(&package.value, &channel_config).unwrap();
        let python_backend = PythonBuildBackend::new(
            package.provenance.path,
            project_model,
            PythonBackendConfig::default(),
            LoggingOutputHandler::default(),
            None,
        )
        .unwrap();

        let recipe = python_backend
            .recipe(Platform::current(), false, &BTreeMap::new())
            .unwrap()
            .0;

        insta::assert_yaml_snapshot!(recipe, {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });
    }

    #[tokio::test]
    async fn test_recipe_from_pyproject_toml() {
        let pyproject_toml_with_build = r#"
        [project]
        dependencies = ["rich"]
        name = "rich_example"
        requires-python = ">= 3.11"
        scripts = { rich-example-main = "rich_example:main" }
        version = "0.1.0"

        [build-system]
        build-backend = "hatchling.build"
        requires = ["hatchling"]

        [tool.pixi.workspace]
        name = "test-scripts"
        channels = ["conda-forge"]
        platforms = ["osx-arm64"]
        preview = ["pixi-build"]

        [too.pixi.package]
        name = "test-scripts"
        version = "1.2.3"

        [tool.pixi.package.build]
        backend = { name = "pixi-build-python", version = "*" }
"#;

        let tmp_dir = tempdir().unwrap();

        let tmp_pyproject_toml = tmp_dir.path().join("pyproject.toml");

        // write the raw pyproject toml into the file
        std::fs::write(&tmp_pyproject_toml, pyproject_toml_with_build).unwrap();

        let manifest = Manifests::from_workspace_manifest_path(tmp_pyproject_toml.clone()).unwrap();
        let package = manifest.value.package.unwrap();
        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
        let project_model = to_project_model_v1(&package.value, &channel_config).unwrap();
        let python_backend = PythonBuildBackend::new(
            package.provenance.path,
            project_model,
            PythonBackendConfig::default(),
            LoggingOutputHandler::default(),
            None,
        )
        .unwrap();

        let recipe = python_backend
            .recipe(Platform::current(), false, &BTreeMap::new())
            .unwrap()
            .0;

        insta::assert_yaml_snapshot!(recipe, {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_env_vars_are_set() {
        let manifest_source = r#"
        [workspace]
        platforms = []
        channels = []
        preview = ["pixi-build"]

        [package]
        name = "foobar"
        version = "0.1.0"

        [package.build]
        backend = { name = "pixi-build-python", version = "*" }
        "#;

        let env = IndexMap::from([("foo".to_string(), "bar".to_string())]);

        let recipe = recipe(
            manifest_source,
            PythonBackendConfig {
                env: env.clone(),
                ..Default::default()
            },
        );

        assert_eq!(recipe.build.script.env, env);
    }
}
