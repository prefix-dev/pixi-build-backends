use std::{collections::BTreeMap, ffi::OsStr, path::PathBuf, str::FromStr};

use miette::IntoDiagnostic;
use pixi_build_backend::{
    dependencies::extract_dependencies, traits::project::new_spec, ProjectModel, Targets,
};
use pixi_build_types::PlatformAndVirtualPackages;
use pyproject_toml::PyProjectToml;
use rattler_build::{
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{BuildConfiguration, Directories, PackagingSettings, PlatformWithVirtualPackages},
    recipe::{
        parser::{Build, Package, PathSource, Python, Requirements, ScriptContent, Source},
        variable::Variable,
        Recipe,
    },
    variant_config::VariantConfig,
    NormalizedKey,
};
use rattler_conda_types::{
    package::{ArchiveType, EntryPoint},
    ChannelConfig, NoArchType, PackageName, Platform,
};
use rattler_package_streaming::write::CompressionLevel;
use rattler_virtual_packages::VirtualPackageOverrides;
use reqwest::Url;

use crate::{
    build_script::{BuildPlatform, BuildScriptContext, Installer},
    config::PythonBackendConfig,
};

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

        let pyproject_manifest = if manifest_path
            .file_name()
            .and_then(OsStr::to_str)
            .map(|str| str.to_lowercase())
            == Some("pyproject.toml".to_string())
        {
            // Load the manifest as a pyproject
            let contents = fs_err::read_to_string(&manifest_path).into_diagnostic()?;

            // Load the manifest as a pyproject
            Some(toml_edit::de::from_str(&contents).into_diagnostic()?)
        } else {
            None
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

    /// Returns the requirements of the project that should be used for a
    /// recipe.
    pub(crate) fn requirements(
        &self,
        host_platform: Platform,
        channel_config: &ChannelConfig,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<(Requirements, Installer)> {
        let mut requirements = Requirements::default();

        let mut dependencies = self
            .project_model
            .targets()
            .map(|t| t.dependencies(Some(host_platform)))
            .unwrap_or_default();

        let uv = "uv".to_string();
        // Determine the installer to use
        let installer = if dependencies.host.contains_key(&uv)
            || dependencies.run.contains_key(&uv)
            || dependencies.build.contains_key(&uv)
        {
            Installer::Uv
        } else {
            Installer::Pip
        };

        let any = new_spec::<P>();

        // Ensure python and pip/uv are available in the host dependencies section.
        let installers = [installer.package_name().to_string(), "python".to_string()];
        for pkg_name in installers.iter() {
            if dependencies.host.contains_key(&pkg_name) {
                // If the host dependencies already contain the package,
                // we don't need to add it again.
                continue;
            }

            dependencies.host.insert(pkg_name, &any);
        }

        requirements.build = extract_dependencies(channel_config, dependencies.build, variant)?;
        requirements.host = extract_dependencies(channel_config, dependencies.host, variant)?;
        requirements.run = extract_dependencies(channel_config, dependencies.run, variant)?;

        Ok((requirements, installer))
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
        channel_config: &ChannelConfig,
        editable: bool,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<Recipe> {
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

        let (requirements, installer) =
            self.requirements(host_platform, channel_config, variant)?;

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
            })])
        };

        Ok(Recipe {
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
                string: Default::default(),

                // skip: Default::default(),
                script: ScriptContent::Commands(build_script).into(),
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
            requirements,
            tests: vec![],
            about: Default::default(),
            extra: Default::default(),
        })
    }

    /// Returns the build configuration for a recipe
    pub fn build_configuration(
        &self,
        recipe: &Recipe,
        channels: Vec<Url>,
        build_platform: Option<PlatformAndVirtualPackages>,
        host_platform: Option<PlatformAndVirtualPackages>,
        variant: BTreeMap<NormalizedKey, Variable>,
        directories: Directories,
    ) -> miette::Result<BuildConfiguration> {
        let build_platform = build_platform.map(|p| PlatformWithVirtualPackages {
            platform: p.platform,
            virtual_packages: p.virtual_packages.unwrap_or_default(),
        });

        let host_platform = host_platform.map(|p| PlatformWithVirtualPackages {
            platform: p.platform,
            virtual_packages: p.virtual_packages.unwrap_or_default(),
        });

        let (build_platform, host_platform) = match (build_platform, host_platform) {
            (Some(build_platform), Some(host_platform)) => (build_platform, host_platform),
            (build_platform, host_platform) => {
                let current_platform =
                    rattler_build::metadata::PlatformWithVirtualPackages::detect(
                        &VirtualPackageOverrides::from_env(),
                    )
                    .into_diagnostic()?;
                (
                    build_platform.unwrap_or_else(|| current_platform.clone()),
                    host_platform.unwrap_or(current_platform),
                )
            }
        };

        let channels = channels.into_iter().map(Into::into).collect();

        Ok(BuildConfiguration {
            // TODO: NoArch??
            target_platform: Platform::NoArch,
            host_platform,
            build_platform,
            hash: HashInfo::from_variant(&variant, &recipe.build.noarch),
            variant,
            directories,
            channels,
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
        })
    }

    /// Determine the all the variants that can be built for this package.
    ///
    /// The variants are computed based on the dependencies of the package and
    /// the input variants. Each package that has a `*` as its version we
    /// consider as a potential variant. If an input variant configuration for
    /// it exists we add it.
    pub fn compute_variants(
        &self,
        input_variant_configuration: Option<BTreeMap<NormalizedKey, Vec<Variable>>>,
        host_platform: Platform,
    ) -> miette::Result<Vec<BTreeMap<NormalizedKey, Variable>>> {
        // Create a variant config from the variant configuration in the parameters.
        let variant_config = VariantConfig {
            variants: input_variant_configuration.unwrap_or_default(),
            pin_run_as_build: None,
            zip_keys: None,
        };

        // Determine the variant keys that are used in the recipe.
        let used_variants = self.project_model.used_variants(Some(host_platform));

        // Determine the combinations of the used variants.
        variant_config
            .combinations(&used_variants, None)
            .into_diagnostic()
    }
}

#[cfg(test)]
mod tests {

    use std::{collections::BTreeMap, path::PathBuf};

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
            .recipe(
                Platform::current(),
                &channel_config,
                false,
                &BTreeMap::new(),
            )
            .unwrap()
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

        let channel_config = ChannelConfig::default_with_root_dir(PathBuf::new());

        let host_platform = Platform::current();
        let variant = BTreeMap::new();

        let (reqs, _) = python_backend
            .requirements(host_platform, &channel_config, &variant)
            .unwrap();

        insta::assert_yaml_snapshot!(reqs);

        let recipe = python_backend.recipe(host_platform, &channel_config, false, &BTreeMap::new());
        insta::assert_yaml_snapshot!(recipe.unwrap(), {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });
    }
}
