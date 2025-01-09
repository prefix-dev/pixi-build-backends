use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use chrono::Utc;
use itertools::Itertools;
use miette::{Context, IntoDiagnostic};
use pixi_build_backend::{
    dependencies::extract_dependencies,
    protocol::{Protocol, ProtocolFactory},
    utils::TemporaryRenderedRecipe,
    variants::can_be_used_as_variant,
};
use pixi_build_types::{
    procedures::{
        conda_build::{CondaBuildParams, CondaBuildResult, CondaBuiltPackage},
        conda_metadata::{CondaMetadataParams, CondaMetadataResult},
        initialize::{InitializeParams, InitializeResult},
        negotiate_capabilities::{NegotiateCapabilitiesParams, NegotiateCapabilitiesResult},
    },
    BackendCapabilities, CondaPackageMetadata, FrontendCapabilities, PlatformAndVirtualPackages,
};
use pixi_manifest::{Dependencies, Manifest, PackageManifest, SpecType};
use pixi_spec::PixiSpec;
use rattler_build::{
    build::run_build,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{
        BuildConfiguration, Directories, Output, PackagingSettings, PlatformWithVirtualPackages,
    },
    recipe::{
        parser::{Build, BuildString, Dependency, Package, Requirements, ScriptContent},
        Jinja, Recipe,
    },
    render::resolved_dependencies::DependencyInfo,
    tool_configuration::Configuration,
    variant_config::VariantConfig,
    NormalizedKey,
};
use rattler_conda_types::{
    package::ArchiveType, ChannelConfig, MatchSpec, NoArchType, PackageName, Platform,
};
use rattler_package_streaming::write::CompressionLevel;
use rattler_virtual_packages::VirtualPackageOverrides;
use reqwest::Url;

use crate::{
    build_script::{BuildPlatform, BuildScriptContext},
    config::CMakeBackendConfig,
    stub::default_compiler,
};

pub struct CMakeBuildBackend {
    logging_output_handler: LoggingOutputHandler,
    manifest_path: PathBuf,
    manifest_root: PathBuf,
    package_manifest: PackageManifest,
    _config: CMakeBackendConfig,
    cache_dir: Option<PathBuf>,
}

impl CMakeBuildBackend {
    /// Returns a new instance of [`CMakeBuildBackendFactory`].
    ///
    /// This type implements [`ProtocolFactory`] and can be used to initialize a
    /// new [`CMakeBuildBackend`].
    pub fn factory(logging_output_handler: LoggingOutputHandler) -> CMakeBuildBackendFactory {
        CMakeBuildBackendFactory {
            logging_output_handler,
        }
    }

    /// Returns a new instance of [`CMakeBuildBackend`] by reading the manifest
    /// at the given path.
    pub fn new(
        manifest_path: &Path,
        config: Option<CMakeBackendConfig>,
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

        let Some(package_manifest) = manifest.package else {
            return Err(miette::miette!("no package manifest found in the manifest"));
        };

        // Read config from the manifest itself if its not provided
        // TODO: I guess this should also be passed over the protocol.
        let config = match config {
            Some(config) => config,
            None => CMakeBackendConfig::from_path(manifest_path)?,
        };

        Ok(Self {
            manifest_path: manifest.path,
            manifest_root,
            package_manifest,
            _config: config,
            logging_output_handler,
            cache_dir,
        })
    }

    /// Returns the capabilities of this backend based on the capabilities of
    /// the frontend.
    pub fn capabilities(_frontend_capabilities: &FrontendCapabilities) -> BackendCapabilities {
        BackendCapabilities {
            provides_conda_metadata: Some(true),
            provides_conda_build: Some(true),
        }
    }

    /// Returns the requirements of the project that should be used for a
    /// recipe.
    fn requirements(
        &self,
        host_platform: Platform,
        channel_config: &ChannelConfig,
        variant: &BTreeMap<NormalizedKey, String>,
    ) -> miette::Result<Requirements> {
        let mut requirements = Requirements::default();

        let targets = self
            .package_manifest
            .targets
            .resolve(Some(host_platform))
            .collect_vec();

        let run_dependencies = Dependencies::from(
            targets
                .iter()
                .filter_map(|f| f.dependencies(SpecType::Run).cloned().map(Cow::Owned)),
        );

        let mut build_dependencies = Dependencies::from(
            targets
                .iter()
                .filter_map(|f| f.dependencies(SpecType::Build).cloned().map(Cow::Owned)),
        );

        let host_dependencies = Dependencies::from(
            targets
                .iter()
                .filter_map(|f| f.dependencies(SpecType::Host).cloned().map(Cow::Owned)),
        );

        // Ensure build tools are available in the build dependencies section.
        for pkg_name in ["cmake", "ninja"] {
            if build_dependencies.contains_key(pkg_name) {
                // If the host dependencies already contain the package, we don't need to add it
                // again.
                continue;
            }

            build_dependencies.insert(
                PackageName::from_str(pkg_name).unwrap(),
                PixiSpec::default(),
            );
        }

        requirements.build = extract_dependencies(channel_config, build_dependencies, variant)?;
        requirements.host = extract_dependencies(channel_config, host_dependencies, variant)?;
        requirements.run = extract_dependencies(channel_config, run_dependencies, variant)?;

        // Add compilers to the dependencies.
        requirements.build.extend(
            self.compiler_packages(host_platform)
                .into_iter()
                .map(Dependency::Spec),
        );

        Ok(requirements)
    }

    /// Returns the matchspecs for the compiler packages. That should be
    /// included in the build section of the recipe.
    fn compiler_packages(&self, target_platform: Platform) -> Vec<MatchSpec> {
        let mut compilers = vec![];

        for lang in self.languages() {
            if let Some(name) = default_compiler(target_platform, &lang) {
                // TODO: Read this from variants
                // TODO: Read the version specification from variants
                let compiler_package =
                    PackageName::new_unchecked(format!("{name}_{target_platform}"));
                compilers.push(MatchSpec::from(compiler_package));
            }

            // TODO: stdlib??
        }

        compilers
    }

    /// Returns the languages that are used in the cmake project. These define
    /// which compilers are required to build the project.
    fn languages(&self) -> Vec<String> {
        // TODO: Can we figure this out from looking at the CMake?
        vec!["cxx".to_string()]
    }

    /// Constructs a [`Recipe`] from the current manifest.
    fn recipe(
        &self,
        host_platform: Platform,
        channel_config: &ChannelConfig,
        variant: &BTreeMap<NormalizedKey, String>,
    ) -> miette::Result<Recipe> {
        // Parse the package name from the manifest
        let package = &self.package_manifest.package;
        let name = PackageName::from_str(&package.name).into_diagnostic()?;

        let noarch_type = NoArchType::none();

        let requirements = self.requirements(host_platform, channel_config, variant)?;
        let build_platform = Platform::current();
        let build_number = 0;

        let build_script = BuildScriptContext {
            build_platform: if build_platform.is_windows() {
                BuildPlatform::Windows
            } else {
                BuildPlatform::Unix
            },
            source_dir: self.manifest_root.display().to_string(),
        }
        .render();

        Ok(Recipe {
            schema_version: 1,
            context: Default::default(),
            package: Package {
                version: package.version.clone().into(),
                name,
            },
            cache: None,
            // source: vec![Source::Path(PathSource {
            //     // TODO: How can we use a git source?
            //     path: manifest_root.to_path_buf(),
            //     sha256: None,
            //     md5: None,
            //     patches: vec![],
            //     target_directory: None,
            //     file_name: None,
            //     use_gitignore: true,
            // })],
            // We hack the source location
            source: vec![],
            build: Build {
                number: build_number,
                string: Default::default(),

                // skip: Default::default(),
                script: ScriptContent::Commands(build_script).into(),
                noarch: noarch_type,

                // TODO: Python is not exposed properly
                //python: Default::default(),
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
        variant: BTreeMap<NormalizedKey, String>,
    ) -> miette::Result<BuildConfiguration> {
        // Parse the package name from the manifest
        let name = self.package_manifest.package.name.clone();
        let name = PackageName::from_str(&name).into_diagnostic()?;
        // TODO: Setup defaults
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
                    PlatformWithVirtualPackages::detect(&VirtualPackageOverrides::from_env())
                        .into_diagnostic()?;
                (
                    build_platform.unwrap_or_else(|| current_platform.clone()),
                    host_platform.unwrap_or(current_platform),
                )
            }
        };

        let channels = channels.into_iter().map(Into::into).collect_vec();

        Ok(BuildConfiguration {
            target_platform: host_platform.platform,
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

fn input_globs() -> Vec<String> {
    [
        // Source files
        "**/*.{c,cc,cxx,cpp,h,hpp,hxx}",
        // CMake files
        "**/*.{cmake,cmake.in}",
        "**/CMakeFiles.txt",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[async_trait::async_trait]
impl Protocol for CMakeBuildBackend {
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
            let recipe = self.recipe(host_platform, &channel_config, &variant)?;
            let output = Output {
                build_configuration: self
                    .build_configuration(
                        &recipe,
                        channels.clone(),
                        params.build_platform.clone(),
                        params.host_platform.clone(),
                        &params.work_directory,
                        variant,
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

            let selector_config = output.build_configuration.selector_config();

            let jinja = Jinja::new(selector_config.clone()).with_context(&output.recipe.context);

            let hash = HashInfo::from_variant(output.variant(), output.recipe.build().noarch());
            let build_string = output.recipe.build().string().resolve(
                &hash,
                output.recipe.build().number(),
                &jinja,
            );

            let finalized_deps = &output
                .finalized_dependencies
                .as_ref()
                .expect("dependencies should be resolved at this point")
                .run;

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

        let recipe = self.recipe(host_platform, &channel_config, &variant)?;
        let output = Output {
            build_configuration: self
                .build_configuration(
                    &recipe,
                    channels,
                    params.host_platform.clone(),
                    Some(PlatformAndVirtualPackages {
                        platform: host_platform,
                        virtual_packages: params.build_platform_virtual_packages,
                    }),
                    &params.work_directory,
                    variant,
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
        let tool_config = Configuration::builder()
            .with_opt_cache_dir(self.cache_dir.clone())
            .with_logging_output_handler(self.logging_output_handler.clone())
            .with_channel_config(channel_config.clone())
            .with_testing(false)
            .with_keep_build(true)
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

        let tool_config = Arc::new(tool_config);
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

pub struct CMakeBuildBackendFactory {
    logging_output_handler: LoggingOutputHandler,
}

#[async_trait::async_trait]
impl ProtocolFactory for CMakeBuildBackendFactory {
    type Protocol = CMakeBuildBackend;

    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> miette::Result<(Self::Protocol, InitializeResult)> {
        let instance = CMakeBuildBackend::new(
            params.manifest_path.as_path(),
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

    use pixi_manifest::Manifest;
    use rattler_build::console_utils::LoggingOutputHandler;
    use rattler_conda_types::{ChannelConfig, Platform};
    use tempfile::tempdir;

    use crate::{cmake::CMakeBuildBackend, config::CMakeBackendConfig};

    #[tokio::test]
    async fn test_setting_host_and_build_requirements() {
        // get cargo manifest dir

        let package_with_host_and_build_deps = r#"
        [workspace]
        name = "test-reqs"
        channels = ["conda-forge"]
        platforms = ["osx-arm64"]
        preview = ["pixi-build"]

        [package]
        name = "test-reqs"
        version = "1.0"

        [package.host-dependencies]
        hatchling = "*"

        [package.build-dependencies]
        boltons = "*"

        [package.run-dependencies]
        foobar = "==3.2.1"

        [package.build]
        backend = { name = "pixi-build-python", version = "*" }
        "#;

        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");

        // write the raw string into the file
        std::fs::write(&tmp_manifest, package_with_host_and_build_deps).unwrap();

        let manifest = Manifest::from_str(&tmp_manifest, package_with_host_and_build_deps).unwrap();

        let cmake_backend = CMakeBuildBackend::new(
            &manifest.path,
            Some(CMakeBackendConfig::default()),
            LoggingOutputHandler::default(),
            None,
        )
        .unwrap();

        let channel_config = ChannelConfig::default_with_root_dir(PathBuf::new());

        let host_platform = Platform::current();

        let recipe = cmake_backend.recipe(host_platform, &channel_config, &BTreeMap::new());
        insta::with_settings!({
            filters => vec![
                ("(vs2017|vs2019|gxx|clang).*", "\"[ ... compiler ... ]\""),
            ]
        }, {
            insta::assert_yaml_snapshot!(recipe.unwrap(), {
               ".build.script" => "[ ... script ... ]",
            });
        });
    }

    #[tokio::test]
    async fn test_parsing_subdirectory() {
        // a manifest with subdir

        let package_with_git_and_subdir = r#"
        [workspace]
        name = "test-reqs"
        channels = ["conda-forge"]
        platforms = ["osx-arm64"]
        preview = ["pixi-build"]

        [package]
        name = "test-reqs"
        version = "1.0"

        [package.build]
        backend = { name = "pixi-build-python", version = "*" }

        [package.host-dependencies]
        hatchling = { git = "git+https://github.com/hatchling/hatchling.git", subdirectory = "src" }
        "#;

        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");

        // write the raw string into the file
        std::fs::write(&tmp_manifest, package_with_git_and_subdir).unwrap();

        Manifest::from_str(&tmp_manifest, package_with_git_and_subdir).unwrap();
    }
}
