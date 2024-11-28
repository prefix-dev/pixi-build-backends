use std::path::{Path, PathBuf};
use std::str::FromStr;

use fs_err as fs;
use miette::IntoDiagnostic;
use pixi_build_backend::protocol::{Protocol, ProtocolFactory};
use pixi_build_backend::tools::RattlerBuild;
use pixi_build_backend::utils::TemporaryRenderedRecipe;
use pixi_build_types::procedures::conda_build::{
    CondaBuildParams, CondaBuildResult, CondaBuiltPackage,
};
use pixi_build_types::procedures::conda_metadata::{CondaMetadataParams, CondaMetadataResult};
use pixi_build_types::procedures::initialize::{InitializeParams, InitializeResult};
use pixi_build_types::{BackendCapabilities, CondaPackageMetadata, FrontendCapabilities};
use rattler_build::build::run_build;
use rattler_build::console_utils::LoggingOutputHandler;

use rattler_build::metadata::PlatformWithVirtualPackages;
use rattler_build::render::resolved_dependencies::DependencyInfo;
use rattler_build::selectors::SelectorConfig;
use rattler_build::tool_configuration::Configuration;
use rattler_conda_types::{ChannelConfig, MatchSpec, Platform};
use rattler_virtual_packages::VirtualPackageOverrides;
use reqwest::Url;

pub struct RattlerBuildBackend {
    logging_output_handler: LoggingOutputHandler,
    /// In case of rattler-build, manifest is the raw recipe
    /// We need to apply later the selectors to get the final recipe
    raw_recipe: String,
    recipe_path: PathBuf,
    cache_dir: Option<PathBuf>,
}

impl RattlerBuildBackend {
    /// Returns a new instance of [`RattlerBuildBackendFactory`].
    ///
    /// This type implements [`ProtocolFactory`] and can be used to initialize a
    /// new [`RattlerBuildBackend`].
    pub fn factory(logging_output_handler: LoggingOutputHandler) -> RattlerBuildBackendFactory {
        RattlerBuildBackendFactory {
            logging_output_handler,
        }
    }

    /// Returns a new instance of [`RattlerBuildBackend`] by reading the manifest
    /// at the given path.
    pub fn new(
        manifest_path: &Path,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
    ) -> miette::Result<Self> {
        // Load the manifest from the source directory
        let raw_recipe = fs::read_to_string(manifest_path).into_diagnostic()?;

        Ok(Self {
            raw_recipe,
            recipe_path: manifest_path.to_path_buf(),
            logging_output_handler,
            cache_dir,
        })
    }

    /// Returns the capabilities of this backend based on the capabilities of
    /// the frontend.
    pub fn capabilites(
        &self,
        _frontend_capabilities: &FrontendCapabilities,
    ) -> BackendCapabilities {
        BackendCapabilities {
            provides_conda_metadata: Some(true),
            provides_conda_build: Some(true),
        }
    }
}

#[async_trait::async_trait]
impl Protocol for RattlerBuildBackend {
    async fn get_conda_metadata(
        &self,
        params: CondaMetadataParams,
    ) -> miette::Result<CondaMetadataResult> {
        // Create the work directory if it does not exist
        fs::create_dir_all(&params.work_directory).into_diagnostic()?;

        let host_platform = params
            .host_platform
            .as_ref()
            .map(|p| p.platform)
            .unwrap_or(Platform::current());

        let build_platform = params
            .build_platform
            .as_ref()
            .map(|p| p.platform)
            .unwrap_or(Platform::current());

        let selector_config = RattlerBuild::selector_config_from(&params);

        let rattler_build_tool = RattlerBuild::new(
            self.raw_recipe.clone(),
            self.recipe_path.clone(),
            selector_config,
            params.work_directory.clone(),
        );

        let channel_config = ChannelConfig {
            channel_alias: params.channel_configuration.base_url,
            root_dir: self
                .recipe_path
                .parent()
                .expect("should have parent")
                .to_path_buf(),
        };

        let channels = params
            .channel_base_urls
            .unwrap_or_else(|| vec![Url::from_str("https://prefix.dev/conda-forge").unwrap()]);

        let discovered_outputs = rattler_build_tool.discover_outputs()?;

        let host_vpkgs = params
            .host_platform
            .as_ref()
            .map(|p| p.virtual_packages.clone())
            .unwrap_or_default();

        let host_vpkgs = RattlerBuild::detect_virtual_packages(host_vpkgs)?;

        let build_vpkgs = params
            .build_platform
            .as_ref()
            .map(|p| p.virtual_packages.clone())
            .unwrap_or_default();

        let build_vpkgs = RattlerBuild::detect_virtual_packages(build_vpkgs)?;

        let outputs = rattler_build_tool.get_outputs(
            &discovered_outputs,
            channels,
            build_vpkgs,
            host_vpkgs,
            host_platform,
            build_platform,
        )?;

        let tool_config = Configuration::builder()
            .with_opt_cache_dir(self.cache_dir.clone())
            .with_logging_output_handler(self.logging_output_handler.clone())
            .with_channel_config(channel_config.clone())
            .with_testing(false)
            .with_keep_build(true)
            .finish();

        let mut solved_packages = vec![];

        for output in outputs {
            let temp_recipe = TemporaryRenderedRecipe::from_output(&output)?;
            let tool_config = &tool_config;
            let output = temp_recipe
                .within_context_async(move || async move {
                    output
                        .resolve_dependencies(tool_config)
                        .await
                        .into_diagnostic()
                })
                .await?;

            let finalized_deps = &output
                .finalized_dependencies
                .as_ref()
                .expect("dependencies should be resolved at this point")
                .run;

            let conda = CondaPackageMetadata {
                name: output.name().clone(),
                version: output.version().clone().into(),
                build: output.build_string().into_owned(),
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
            };
            solved_packages.push(conda);
        }

        Ok(CondaMetadataResult {
            packages: solved_packages,
            input_globs: None,
        })
    }

    async fn build_conda(&self, params: CondaBuildParams) -> miette::Result<CondaBuildResult> {
        // Create the work directory if it does not exist
        fs::create_dir_all(&params.work_directory).into_diagnostic()?;

        let host_platform = params
            .host_platform
            .as_ref()
            .map(|p| p.platform)
            .unwrap_or(Platform::current());

        let build_platform = Platform::current();

        let selector_config = SelectorConfig {
            target_platform: build_platform,
            host_platform,
            build_platform,
            hash: None,
            variant: Default::default(),
            experimental: true,
            allow_undefined: false,
        };

        let host_vpkgs = params
            .host_platform
            .as_ref()
            .map(|p| p.virtual_packages.clone())
            .unwrap_or_default();

        let host_vpkgs = match host_vpkgs {
            Some(vpkgs) => vpkgs,
            None => {
                PlatformWithVirtualPackages::detect(&VirtualPackageOverrides::from_env())
                    .into_diagnostic()?
                    .virtual_packages
            }
        };

        let build_vpkgs = params
            .build_platform_virtual_packages
            .clone()
            .unwrap_or_default();

        let channel_config = ChannelConfig {
            channel_alias: params.channel_configuration.base_url,
            root_dir: self
                .recipe_path
                .parent()
                .expect("should have parent")
                .to_path_buf(),
        };

        let channels = params
            .channel_base_urls
            .unwrap_or_else(|| vec![Url::from_str("https://prefix.dev/conda-forge").unwrap()]);

        let rattler_build_tool = RattlerBuild::new(
            self.raw_recipe.clone(),
            self.recipe_path.clone(),
            selector_config,
            params.work_directory.clone(),
        );

        let discovered_outputs = rattler_build_tool.discover_outputs()?;

        let outputs = rattler_build_tool.get_outputs(
            &discovered_outputs,
            channels,
            build_vpkgs,
            host_vpkgs,
            host_platform,
            build_platform,
        )?;

        let mut built = vec![];

        let tool_config = Configuration::builder()
            .with_opt_cache_dir(self.cache_dir.clone())
            .with_logging_output_handler(self.logging_output_handler.clone())
            .with_channel_config(channel_config.clone())
            .with_testing(false)
            .with_keep_build(true)
            .finish();

        for output in outputs {
            let temp_recipe = TemporaryRenderedRecipe::from_output(&output)?;

            let tool_config = &tool_config;
            let (output, build_path) = temp_recipe
                .within_context_async(move || async move { run_build(output, tool_config).await })
                .await?;

            built.push(CondaBuiltPackage {
                output_file: build_path,
                input_globs: Vec::from([self.recipe_path.to_string_lossy().to_string()]),
                name: output.name().as_normalized().to_string(),
                version: output.version().to_string(),
                build: output.build_string().into_owned(),
                subdir: output.target_platform().to_string(),
            });
        }

        Ok(CondaBuildResult { packages: built })
    }
}
pub struct RattlerBuildBackendFactory {
    logging_output_handler: LoggingOutputHandler,
}

#[async_trait::async_trait]
impl ProtocolFactory for RattlerBuildBackendFactory {
    type Protocol = RattlerBuildBackend;

    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> miette::Result<(Self::Protocol, InitializeResult)> {
        let instance = RattlerBuildBackend::new(
            params.manifest_path.as_path(),
            self.logging_output_handler.clone(),
            params.cache_directory,
        )?;

        let capabilities = instance.capabilites(&params.capabilities);
        Ok((instance, InitializeResult { capabilities }))
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use pixi_build_backend::protocol::{Protocol, ProtocolFactory};
    use pixi_build_types::{
        procedures::{
            conda_build::CondaBuildParams, conda_metadata::CondaMetadataParams,
            initialize::InitializeParams,
        },
        ChannelConfiguration, FrontendCapabilities,
    };
    use rattler_build::console_utils::LoggingOutputHandler;
    use tempfile::tempdir;

    use crate::rattler_build::RattlerBuildBackend;

    use url::Url;

    #[tokio::test]
    async fn test_get_conda_metadata() {
        // get cargo manifest dir
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = manifest_dir.join("../../recipe/recipe.yaml");

        let factory = RattlerBuildBackend::factory(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                manifest_path: recipe,
                capabilities: FrontendCapabilities {},
                cache_directory: None,
            })
            .await
            .unwrap();

        let current_dir = std::env::current_dir().unwrap();

        let result = factory
            .0
            .get_conda_metadata(CondaMetadataParams {
                host_platform: None,
                build_platform: None,
                channel_configuration: ChannelConfiguration {
                    base_url: Url::from_str("https://prefix.dev").unwrap(),
                },
                channel_base_urls: None,
                work_directory: current_dir,
            })
            .await
            .unwrap();

        assert_eq!(result.packages.len(), 3);
    }

    #[tokio::test]
    async fn test_conda_build() {
        // get cargo manifest dir
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = manifest_dir.join("../../tests/recipe/boltons_recipe.yaml");

        let factory = RattlerBuildBackend::factory(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                manifest_path: recipe,
                capabilities: FrontendCapabilities {},
                cache_directory: None,
            })
            .await
            .unwrap();

        let current_dir = tempdir().unwrap();

        let result = factory
            .0
            .build_conda(CondaBuildParams {
                build_platform_virtual_packages: None,
                host_platform: None,
                channel_base_urls: None,
                channel_configuration: ChannelConfiguration {
                    base_url: Url::from_str("https://prefix.dev").unwrap(),
                },
                outputs: None,
                work_directory: current_dir.into_path(),
            })
            .await
            .unwrap();

        assert_eq!(result.packages[0].name, "boltons-with-extra");
    }
}
