use std::{
    borrow::Cow,
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use chrono::Utc;
use indexmap::IndexMap;
use itertools::Itertools;
use miette::{Context, IntoDiagnostic};
use pixi_build_backend::{
    dependencies::extract_dependencies,
    protocol::{Protocol, ProtocolFactory},
    utils::TemporaryRenderedRecipe,
    variants::can_be_used_as_variant,
    TargetExt,
};
use pixi_build_types::{
    procedures::{
        conda_build::{CondaBuildParams, CondaBuildResult, CondaBuiltPackage},
        conda_metadata::{CondaMetadataParams, CondaMetadataResult},
        initialize::{InitializeParams, InitializeResult},
        negotiate_capabilities::{NegotiateCapabilitiesParams, NegotiateCapabilitiesResult},
    },
    BackendCapabilities, CondaPackageMetadata, FrontendCapabilities, PixiSpecV1,
    PlatformAndVirtualPackages, ProjectModelV1, SourcePackageName, VersionedProjectModel,
};
use pixi_manifest::{Dependencies, Manifest, SpecType};
use pixi_spec::PixiSpec;
use pyproject_toml::PyProjectToml;
use rattler_build::{
    build::run_build,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{
        BuildConfiguration, Directories, Output, PackagingSettings, PlatformWithVirtualPackages,
    },
    recipe::{
        parser::{
            Build, BuildString, Package, PathSource, Python, Requirements, ScriptContent, Source,
        },
        Jinja, Recipe,
    },
    render::resolved_dependencies::DependencyInfo,
    tool_configuration::Configuration,
    variant_config::VariantConfig,
    NormalizedKey,
};
use rattler_conda_types::{
    package::{ArchiveType, EntryPoint},
    ChannelConfig, MatchSpec, NoArchType, PackageName, Platform,
};
use rattler_package_streaming::write::CompressionLevel;
use rattler_virtual_packages::VirtualPackageOverrides;
use reqwest::Url;

use crate::{
    build_script::{BuildPlatform, BuildScriptContext, Installer},
    config::PythonBackendConfig,
};

pub struct PythonBuildBackend {
    logging_output_handler: LoggingOutputHandler,
    manifest_path: PathBuf,
    manifest_root: PathBuf,
    project_model: ProjectModelV1,
    config: PythonBackendConfig,
    cache_dir: Option<PathBuf>,
    pyproject_manifest: Option<PyProjectToml>,
}

impl PythonBuildBackend {
    /// Returns a new instance of [`PythonBuildBackendFactory`].
    ///
    /// This type implements [`ProtocolFactory`] and can be used to initialize a
    /// new [`PythonBuildBackend`].
    pub fn factory(logging_output_handler: LoggingOutputHandler) -> PythonBuildBackendFactory {
        PythonBuildBackendFactory {
            logging_output_handler,
        }
    }

    /// Returns a new instance of [`PythonBuildBackend`] by reading the manifest
    /// at the given path.
    pub fn new(
        manifest_path: &Path,
        project_model: VersionedProjectModel,
        config: Option<PythonBackendConfig>,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
    ) -> miette::Result<Self> {
        // Load the manifest from the source directory
        let manifest = Manifest::from_path(manifest_path).with_context(|| {
            format!("failed to parse manifest from {}", manifest_path.display())
        })?;

        // Determine the root directory of the manifest
        let manifest_root = manifest
            .path
            .parent()
            .ok_or_else(|| miette::miette!("the project manifest must reside in a directory"))?
            .to_path_buf();

        // Read config from the manifest itself if its not provided
        // TODO: I guess this should also be passed over the protocol.
        let config = match config {
            Some(config) => config,
            None => PythonBackendConfig::from_path(manifest_path)?,
        };

        let pyproject_manifest = if manifest_path
            .file_name()
            .and_then(OsStr::to_str)
            .map(|str| str.to_lowercase())
            == Some("pyproject.toml".to_string())
        {
            // Load the manifest as a pyproject
            let contents = fs_err::read_to_string(manifest_path).into_diagnostic()?;

            // Load the manifest as a pyproject
            Some(toml_edit::de::from_str(&contents).into_diagnostic()?)
        } else {
            None
        };

        let v1 = project_model
            .into_v1()
            .ok_or_else(|| miette::miette!("project model is required"))?;

        Ok(Self {
            manifest_path: manifest.path,
            manifest_root,
            project_model: v1,
            config,
            logging_output_handler,
            cache_dir,
            pyproject_manifest,
        })
    }

    /// Returns the capabilities of this backend based on the capabilities of
    /// the frontend.
    pub fn capabilities(_frontend_capabilities: &FrontendCapabilities) -> BackendCapabilities {
        BackendCapabilities {
            provides_conda_metadata: Some(true),
            provides_conda_build: Some(true),
            highest_supported_project_model: Some(
                pixi_build_types::VersionedProjectModel::highest_version(),
            ),
        }
    }

    /// Returns the requirements of the project that should be used for a
    /// recipe.
    fn requirements(
        &self,
        host_platform: Platform,
        channel_config: &ChannelConfig,
        variant: &BTreeMap<NormalizedKey, String>,
    ) -> miette::Result<(Requirements, Installer)> {
        let mut requirements = Requirements::default();

        let targets = self
            .project_model
            .targets
            .resolve(Some(host_platform))
            .collect_vec();

        // let run_dependencies = Dependencies::from(
        //     targets
        //         .iter()
        //         .filter_map(|f| f.dependencies(SpecType::Run).cloned().map(Cow::Owned)),
        // );

        // let build_dependencies = Dependencies::from(
        //     targets
        //         .iter()
        //         .filter_map(|f| f.dependencies(SpecType::Build).cloned().map(Cow::Owned)),
        // );

        // let mut host_dependencies = Dependencies::from(
        //     targets
        //         .iter()
        //         .filter_map(|f| f.dependencies(SpecType::Host).cloned().map(Cow::Owned)),
        // );

        let run_dependencies = targets
            .iter()
            .flat_map(|t| t.run_dependencies.iter())
            .collect::<IndexMap<&SourcePackageName, &PixiSpecV1>>();
        let host_dependencies = targets
            .iter()
            .flat_map(|t| t.run_dependencies.iter())
            .collect::<IndexMap<&SourcePackageName, &PixiSpecV1>>();
        let build_dependencies = targets
            .iter()
            .flat_map(|t| t.run_dependencies.iter())
            .collect::<IndexMap<&SourcePackageName, &PixiSpecV1>>();

        // Determine the installer to use
        let installer = if host_dependencies.contains_key("uv")
            || run_dependencies.contains_key("uv")
            || build_dependencies.contains_key("uv")
        {
            Installer::Uv
        } else {
            Installer::Pip
        };

        // Ensure python and pip/uv are available in the host dependencies section.
        for pkg_name in [installer.package_name(), "python"] {
            if host_dependencies.contains_key(pkg_name) {
                // If the host dependencies already contain the package,
                // we don't need to add it again.
                continue;
            }

            host_dependencies.insert(
                PackageName::from_str(pkg_name).unwrap(),
                PixiSpec::default(),
            );
        }

        requirements.build = extract_dependencies(channel_config, build_dependencies, variant)?;
        requirements.host = extract_dependencies(channel_config, host_dependencies, variant)?;
        requirements.run = extract_dependencies(channel_config, run_dependencies, variant)?;

        Ok((requirements, installer))
    }

    /// Constructs a [`Recipe`] from the current manifest.
    fn recipe(
        &self,
        host_platform: Platform,
        channel_config: &ChannelConfig,
        editable: bool,
        variant: &BTreeMap<NormalizedKey, String>,
    ) -> miette::Result<Recipe> {
        // Parse the package name from the manifest
        let project_model = &self.project_model;

        let name = PackageName::from_str(&project_model.name).into_diagnostic()?;

        let noarch_type = if self.config.noarch() {
            NoArchType::python()
        } else {
            NoArchType::none()
        };

        // Determine the entry points from the pyproject.toml
        // which would be passed into recipe
        let python = if let Some(pyproject_manifest) = &self.pyproject_manifest {
            let mut python = Python::default();
            let scripts = pyproject_manifest
                .project
                .as_ref()
                .and_then(|p| p.scripts.as_ref());
            if let Some(scripts) = scripts {
                python.entry_points = scripts
                    .into_iter()
                    .flat_map(|(name, entry_point)| {
                        EntryPoint::from_str(&format!("{name} = {entry_point}"))
                    })
                    .collect();
            }
            python
        } else {
            Python::default()
        };

        let (requirements, installer) =
            self.requirements(host_platform, channel_config, variant)?;
        let build_platform = Platform::current();
        let build_number = 0;

        let build_script = BuildScriptContext {
            installer,
            build_platform: if build_platform.is_windows() {
                BuildPlatform::Windows
            } else {
                BuildPlatform::Unix
            },
            // TODO: remove this as soon as we have profiles
            editable: std::env::var("BUILD_EDITABLE_PYTHON")
                .map(|val| val == "true")
                .unwrap_or(editable),
            manifest_root: self.manifest_root.clone(),
        }
        .render();

        let source = if editable {
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
                version: project_model.version.clone().into(),
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
            // TODO read from manifest
            requirements,
            tests: vec![],
            about: Default::default(),
            extra: Default::default(),
        })
    }

    /// Returns the build configuration for a recipe
    pub async fn build_configuration(
        &self,
        recipe: &Recipe,
        channels: Vec<Url>,
        build_platform: Option<PlatformAndVirtualPackages>,
        host_platform: Option<PlatformAndVirtualPackages>,
        work_directory: &Path,
    ) -> miette::Result<BuildConfiguration> {
        // Parse the package name from the manifest
        let name = self.project_model.name.clone();
        let name = PackageName::from_str(&name).into_diagnostic()?;

        std::fs::create_dir_all(work_directory)
            .into_diagnostic()
            .context("failed to create output directory")?;
        let directories = Directories::setup(
            name.as_normalized(),
            &self.manifest_path,
            work_directory,
            true,
            &Utc::now(),
        )
        .into_diagnostic()
        .context("failed to setup build directories")?;

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

        let variant = BTreeMap::new();
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
}

/// Determines the build input globs for given python package
/// even this will be probably backend specific, e.g setuptools
/// has a different way of determining the input globs than hatch etc.
///
/// However, lets take everything in the directory as input for now
fn input_globs() -> Vec<String> {
    vec![
        // Source files
        "**/*.py",
        "**/*.pyx",
        "**/*.c",
        "**/*.cpp",
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[async_trait::async_trait]
impl Protocol for PythonBuildBackend {
    async fn get_conda_metadata(
        &self,
        params: CondaMetadataParams,
    ) -> miette::Result<CondaMetadataResult> {
        let channel_config = ChannelConfig {
            channel_alias: params.channel_configuration.base_url,
            root_dir: self.manifest_root.to_path_buf(),
        };
        let channels = params.channel_base_urls.unwrap_or_default();

        let host_platform = params
            .host_platform
            .as_ref()
            .map(|p| p.platform)
            .unwrap_or(Platform::current());

        // Build the tool configuration
        let tool_config = Arc::new(
            Configuration::builder()
                .with_opt_cache_dir(self.cache_dir.clone())
                .with_logging_output_handler(self.logging_output_handler.clone())
                .with_channel_config(channel_config.clone())
                .with_testing(false)
                .with_keep_build(true)
                .finish(),
        );

        // Create a variant config from the variant configuration in the parameters.
        let variant_config = VariantConfig {
            variants: params
                .variant_configuration
                .unwrap_or_default()
                .into_iter()
                .map(|(key, values)| (key.into(), values))
                .collect(),
            pin_run_as_build: None,
            zip_keys: None,
        };

        // Determine the variant keys that are used in the recipe.
        let used_variants = self
            .package_manifest
            .targets
            .resolve(Some(host_platform))
            .flat_map(|dep| dep.dependencies.values().flatten())
            .filter(|(_, spec)| can_be_used_as_variant(spec))
            .map(|(name, _)| name.into())
            .collect();

        // Determine the combinations of the used variants.
        let combinations = variant_config
            .combinations(&used_variants, None)
            .into_diagnostic()?;

        // Construct the different outputs
        let mut packages = Vec::new();
        for variant in combinations {
            // TODO: Determine how and if we can determine this from the manifest.
            let recipe = self.recipe(host_platform, &channel_config, false, &variant)?;
            let output = Output {
                build_configuration: self
                    .build_configuration(
                        &recipe,
                        channels.clone(),
                        params.build_platform.clone(),
                        params.host_platform.clone(),
                        &params.work_directory.clone(),
                    )
                    .await?,
                recipe,
                finalized_dependencies: None,
                finalized_cache_dependencies: None,
                finalized_cache_sources: None,
                finalized_sources: None,
                build_summary: Arc::default(),
                system_tools: Default::default(),
                extra_meta: None,
            };

            let temp_recipe = TemporaryRenderedRecipe::from_output(&output)?;
            let tool_config = tool_config.clone();
            let output = temp_recipe
                .within_context_async(move || async move {
                    output
                        .resolve_dependencies(&tool_config)
                        .await
                        .into_diagnostic()
                })
                .await?;

            let finalized_deps = &output
                .finalized_dependencies
                .as_ref()
                .expect("dependencies should be resolved at this point")
                .run;

            let selector_config = output.build_configuration.selector_config();

            let jinja = Jinja::new(selector_config.clone()).with_context(&output.recipe.context);

            let hash = HashInfo::from_variant(output.variant(), output.recipe.build().noarch());
            let build_string = output.recipe.build().string().resolve(
                &hash,
                output.recipe.build().number(),
                &jinja,
            );

            packages.push(CondaPackageMetadata {
                name: output.name().clone(),
                version: output.version().clone().into(),
                build: build_string.to_string(),
                build_number: output.recipe.build.number,
                subdir: output.build_configuration.target_platform,
                depends: finalized_deps
                    .depends
                    .iter()
                    .map(DependencyInfo::spec)
                    .map(MatchSpec::to_string)
                    .collect(),
                constraints: finalized_deps
                    .constraints
                    .iter()
                    .map(DependencyInfo::spec)
                    .map(MatchSpec::to_string)
                    .collect(),
                license: output.recipe.about.license.map(|l| l.to_string()),
                license_family: output.recipe.about.license_family,
                noarch: output.recipe.build.noarch,
            });
        }

        Ok(CondaMetadataResult {
            packages,
            input_globs: None,
        })
    }

    async fn build_conda(&self, params: CondaBuildParams) -> miette::Result<CondaBuildResult> {
        let channel_config = ChannelConfig {
            channel_alias: params.channel_configuration.base_url,
            root_dir: self.manifest_root.to_path_buf(),
        };
        let channels = params.channel_base_urls.unwrap_or_default();

        let host_platform = params
            .host_platform
            .as_ref()
            .map(|p| p.platform)
            .unwrap_or_else(Platform::current);

        let variant = BTreeMap::new();

        let recipe = self.recipe(host_platform, &channel_config, params.editable, &variant)?;
        let output = Output {
            build_configuration: self
                .build_configuration(&recipe, channels, None, None, &params.work_directory)
                .await?,
            recipe,
            finalized_dependencies: None,
            finalized_cache_dependencies: None,
            finalized_cache_sources: None,
            finalized_sources: None,
            build_summary: Arc::default(),
            system_tools: Default::default(),
            extra_meta: None,
        };
        let tool_config = Configuration::builder()
            .with_opt_cache_dir(self.cache_dir.clone())
            .with_logging_output_handler(self.logging_output_handler.clone())
            .with_channel_config(channel_config.clone())
            .with_testing(false)
            .finish();

        let temp_recipe = TemporaryRenderedRecipe::from_output(&output)?;

        let mut output_with_build_string = output.clone();

        let selector_config = output.build_configuration.selector_config();

        let jinja = Jinja::new(selector_config.clone()).with_context(&output.recipe.context);

        let hash = HashInfo::from_variant(output.variant(), output.recipe.build().noarch());
        let build_string =
            output
                .recipe
                .build()
                .string()
                .resolve(&hash, output.recipe.build().number(), &jinja);
        output_with_build_string.recipe.build.string =
            BuildString::Resolved(build_string.to_string());

        let (output, package) = temp_recipe
            .within_context_async(move || async move {
                run_build(output_with_build_string, &tool_config).await
            })
            .await?;

        Ok(CondaBuildResult {
            packages: vec![CondaBuiltPackage {
                output_file: package,
                input_globs: input_globs(),
                name: output.name().as_normalized().to_string(),
                version: output.version().to_string(),
                build: build_string.to_string(),
                subdir: output.target_platform().to_string(),
            }],
        })
    }
}

pub struct PythonBuildBackendFactory {
    logging_output_handler: LoggingOutputHandler,
}

#[async_trait::async_trait]
impl ProtocolFactory for PythonBuildBackendFactory {
    type Protocol = PythonBuildBackend;

    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> miette::Result<(Self::Protocol, InitializeResult)> {
        let instance = PythonBuildBackend::new(
            params.manifest_path.as_path(),
            params
                .project_model
                .ok_or_else(|| miette::miette!("project model is required"))?,
            None,
            self.logging_output_handler.clone(),
            params.cache_directory,
        )?;

        Ok((instance, InitializeResult {}))
    }

    async fn negotiate_capabilities(
        params: NegotiateCapabilitiesParams,
    ) -> miette::Result<NegotiateCapabilitiesResult> {
        let capabilities = Self::Protocol::capabilities(&params.capabilities);
        Ok(NegotiateCapabilitiesResult { capabilities })
    }
}

#[cfg(test)]
mod tests {

    use std::{collections::BTreeMap, path::PathBuf};

    use pixi_build_type_conversions::to_project_model_v1;
    use pixi_manifest::Manifest;
    use rattler_build::{console_utils::LoggingOutputHandler, recipe::Recipe};
    use rattler_conda_types::{ChannelConfig, Platform};
    use tempfile::tempdir;

    use crate::{config::PythonBackendConfig, python::PythonBuildBackend};

    fn recipe(manifest_source: &str, config: PythonBackendConfig) -> Recipe {
        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");
        std::fs::write(&tmp_manifest, manifest_source).unwrap();
        let manifest = pixi_manifest::Manifest::from_path(&tmp_manifest).unwrap();
        let package = manifest.package.unwrap();
        let project_model = to_project_model_v1(&package);

        let python_backend = PythonBuildBackend::new(
            &tmp_manifest,
            project_model,
            Some(config),
            LoggingOutputHandler::default(),
            None,
        )
        .unwrap();

        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
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

        let manifest = Manifest::from_str(&tmp_manifest, package_with_host_and_build_deps).unwrap();
        let project_model = to_project_model_v1(&manifest.package.unwrap());
        let python_backend = PythonBuildBackend::new(
            &manifest.path,
            project_model,
            Some(PythonBackendConfig::default()),
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
