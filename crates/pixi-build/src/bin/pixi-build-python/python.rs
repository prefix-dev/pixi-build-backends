use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsStr,
    path::PathBuf,
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
    AnyVersion, TargetExt,
};
use pixi_build_types::{
    self as pbt,
    procedures::{
        conda_build::{
            CondaBuildParams, CondaBuildResult, CondaBuiltPackage, CondaOutputIdentifier,
        },
        conda_metadata::{CondaMetadataParams, CondaMetadataResult},
        initialize::{InitializeParams, InitializeResult},
        negotiate_capabilities::{NegotiateCapabilitiesParams, NegotiateCapabilitiesResult},
    },
    BackendCapabilities, CondaPackageMetadata, FrontendCapabilities, PlatformAndVirtualPackages,
    ProjectModelV1,
};
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
    project_model: pbt::ProjectModelV1,
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
        manifest_path: PathBuf,
        project_model: ProjectModelV1,
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
            .iter()
            .flat_map(|targets| targets.resolve(Some(host_platform)))
            .collect_vec();

        let run_dependencies = targets
            .iter()
            .flat_map(|t| t.run_dependencies.iter())
            .flatten()
            .collect::<IndexMap<&pbt::SourcePackageName, &pbt::PackageSpecV1>>();

        let mut host_dependencies = targets
            .iter()
            .flat_map(|t| t.host_dependencies.iter())
            .flatten()
            .collect::<IndexMap<&pbt::SourcePackageName, &pbt::PackageSpecV1>>();

        let build_dependencies = targets
            .iter()
            .flat_map(|t| t.build_dependencies.iter())
            .flatten()
            .collect::<IndexMap<&pbt::SourcePackageName, &pbt::PackageSpecV1>>();

        let uv = "uv".to_string();
        // Determine the installer to use
        let installer = if host_dependencies.contains_key(&uv)
            || run_dependencies.contains_key(&uv)
            || build_dependencies.contains_key(&uv)
        {
            Installer::Uv
        } else {
            Installer::Pip
        };

        let any = pbt::PackageSpecV1::any();

        // Ensure python and pip/uv are available in the host dependencies section.
        let installers = [installer.package_name().to_string(), "python".to_string()];
        for pkg_name in installers.iter() {
            if host_dependencies.contains_key(&pkg_name) {
                // If the host dependencies already contain the package,
                // we don't need to add it again.
                continue;
            }

            host_dependencies.insert(pkg_name, &any);
        }

        requirements.build = extract_dependencies(channel_config, build_dependencies, variant)?;
        requirements.host = extract_dependencies(channel_config, host_dependencies, variant)?;
        requirements.run = extract_dependencies(channel_config, run_dependencies, variant)?;

        Ok((requirements, installer))
    }

    /// Read the entry points from the pyproject.toml and return them as a list.
    ///
    /// If the manifest is not a pyproject.toml file no entry-points are added.
    fn entry_points(&self) -> Vec<EntryPoint> {
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
    fn recipe(
        &self,
        host_platform: Platform,
        channel_config: &ChannelConfig,
        editable: bool,
        variant: &BTreeMap<NormalizedKey, String>,
    ) -> miette::Result<Recipe> {
        // Parse the package name and version from the manifest
        let name = PackageName::from_str(&self.project_model.name).into_diagnostic()?;
        let version = self.project_model.version.clone().ok_or_else(|| {
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
            // TODO: remove this as soon as we have profiles
            editable: std::env::var("BUILD_EDITABLE_PYTHON")
                .map(|val| val == "true")
                .unwrap_or(editable),
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
        variant: BTreeMap<NormalizedKey, String>,
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
        input_variant_configuration: Option<HashMap<String, Vec<String>>>,
        host_platform: Platform,
    ) -> miette::Result<Vec<BTreeMap<NormalizedKey, String>>> {
        // Create a variant config from the variant configuration in the parameters.
        let variant_config = VariantConfig {
            variants: input_variant_configuration
                .unwrap_or_default()
                .into_iter()
                .map(|(key, values)| (key.into(), values))
                .collect(),
            pin_run_as_build: None,
            zip_keys: None,
        };

        // Determine the variant keys that are used in the recipe.
        let used_variants = self
            .project_model
            .targets
            .iter()
            .flat_map(|target| target.resolve(Some(host_platform)))
            .flat_map(|dep| {
                dep.build_dependencies
                    .iter()
                    .flatten()
                    .chain(dep.run_dependencies.iter().flatten())
                    .chain(dep.host_dependencies.iter().flatten())
            })
            .filter(|(_, spec)| can_be_used_as_variant(spec))
            .map(|(name, _)| name.clone().into())
            .collect();

        // Determine the combinations of the used variants.
        variant_config
            .combinations(&used_variants, None)
            .into_diagnostic()
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

        let package_name = PackageName::from_str(&self.project_model.name)
            .into_diagnostic()
            .context("`{name}` is not a valid package name")?;

        let directories = Directories::setup(
            package_name.as_normalized(),
            &self.manifest_path,
            &params.work_directory,
            true,
            &Utc::now(),
        )
        .into_diagnostic()
        .context("failed to setup build directories")?;

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
            .project_model
            .targets
            .iter()
            .flat_map(|target| target.resolve(Some(host_platform)))
            .flat_map(|dep| {
                dep.build_dependencies
                    .iter()
                    .flatten()
                    .chain(dep.run_dependencies.iter().flatten())
                    .chain(dep.host_dependencies.iter().flatten())
            })
            .filter(|(_, spec)| can_be_used_as_variant(spec))
            .map(|(name, _)| name.clone().into())
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
                build_configuration: self.build_configuration(
                    &recipe,
                    channels.clone(),
                    params.build_platform.clone(),
                    params.host_platform.clone(),
                    variant,
                    directories.clone(),
                )?,
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

        let package_name = PackageName::from_str(&self.project_model.name)
            .into_diagnostic()
            .context("`{name}` is not a valid package name")?;

        let directories = Directories::setup(
            package_name.as_normalized(),
            &self.manifest_path,
            &params.work_directory,
            true,
            &Utc::now(),
        )
        .into_diagnostic()
        .context("failed to setup build directories")?;

        // Recompute all the variant combinations
        let variant_combinations =
            self.compute_variants(params.variant_configuration, host_platform)?;

        // Compute outputs for each variant
        let mut outputs = Vec::with_capacity(variant_combinations.len());
        for variant in variant_combinations {
            let recipe = self.recipe(host_platform, &channel_config, params.editable, &variant)?;
            let build_configuration = self.build_configuration(
                &recipe,
                channels.clone(),
                params.host_platform.clone(),
                Some(PlatformAndVirtualPackages {
                    platform: host_platform,
                    virtual_packages: params.build_platform_virtual_packages.clone(),
                }),
                variant,
                directories.clone(),
            )?;

            let mut output = Output {
                build_configuration,
                recipe,
                finalized_dependencies: None,
                finalized_cache_dependencies: None,
                finalized_cache_sources: None,
                finalized_sources: None,
                build_summary: Arc::default(),
                system_tools: Default::default(),
                extra_meta: None,
            };

            // Resolve the build string
            let selector_config = output.build_configuration.selector_config();
            let jinja = Jinja::new(selector_config.clone()).with_context(&output.recipe.context);
            let hash = HashInfo::from_variant(output.variant(), output.recipe.build().noarch());
            let build_string = output
                .recipe
                .build()
                .string()
                .resolve(&hash, output.recipe.build().number(), &jinja)
                .into_owned();
            output.recipe.build.string = BuildString::Resolved(build_string);

            outputs.push(output);
        }

        // Setup tool configuration
        let tool_config = Arc::new(
            Configuration::builder()
                .with_opt_cache_dir(self.cache_dir.clone())
                .with_logging_output_handler(self.logging_output_handler.clone())
                .with_channel_config(channel_config.clone())
                .with_testing(false)
                .with_keep_build(true)
                .finish(),
        );

        // Determine the outputs to build
        let selected_outputs = if let Some(output_identifiers) = params.outputs {
            output_identifiers
                .into_iter()
                .filter_map(|iden| {
                    let pos = outputs.iter().position(|output| {
                        let CondaOutputIdentifier {
                            name,
                            version,
                            build,
                            subdir,
                        } = &iden;
                        name.as_ref()
                            .map_or(true, |n| output.name().as_normalized() == n)
                            && version
                                .as_ref()
                                .map_or(true, |v| output.version().to_string() == *v)
                            && build
                                .as_ref()
                                .map_or(true, |b| output.build_string() == b.as_str())
                            && subdir
                                .as_ref()
                                .map_or(true, |s| output.target_platform().as_str() == s)
                    })?;
                    Some(outputs.remove(pos))
                })
                .collect()
        } else {
            outputs
        };

        let mut packages = Vec::with_capacity(selected_outputs.len());
        for output in selected_outputs {
            let temp_recipe = TemporaryRenderedRecipe::from_output(&output)?;
            let build_string = output
                .recipe
                .build
                .string
                .as_resolved()
                .expect("build string must have already been resolved")
                .to_string();
            let tool_config = tool_config.clone();
            let (output, package) = temp_recipe
                .within_context_async(move || async move { run_build(output, &tool_config).await })
                .await?;
            let built_package = CondaBuiltPackage {
                output_file: package,
                input_globs: input_globs(),
                name: output.name().as_normalized().to_string(),
                version: output.version().to_string(),
                build: build_string.to_string(),
                subdir: output.target_platform().to_string(),
            };
            packages.push(built_package);
        }

        Ok(CondaBuildResult { packages })
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
        let project_model = params
            .project_model
            .ok_or_else(|| miette::miette!("project model is required"))?;

        let project_model = project_model
            .into_v1()
            .ok_or_else(|| miette::miette!("project model v1 is required"))?;

        let config = if let Some(config) = params.configuration {
            serde_json::from_value(config)
                .into_diagnostic()
                .context("failed to parse configuration")?
        } else {
            PythonBackendConfig::default()
        };

        let instance = PythonBuildBackend::new(
            params.manifest_path,
            project_model,
            config,
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
        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
        let project_model = to_project_model_v1(&package, &channel_config).unwrap();

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

        let manifest = Manifest::from_str(&tmp_manifest, package_with_host_and_build_deps).unwrap();
        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
        let project_model =
            to_project_model_v1(&manifest.package.unwrap(), &channel_config).unwrap();
        let python_backend = PythonBuildBackend::new(
            manifest.path,
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
