use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    str::FromStr,
};

use fs_err as fs;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    protocol::{Protocol, ProtocolFactory},
    tools::RattlerBuild,
    utils::TemporaryRenderedRecipe,
};
use pixi_build_types::{
    procedures::{
        conda_build::{CondaBuildParams, CondaBuildResult, CondaBuiltPackage},
        conda_metadata::{CondaMetadataParams, CondaMetadataResult},
        initialize::{InitializeParams, InitializeResult},
    },
    BackendCapabilities, CondaPackageMetadata, FrontendCapabilities,
};
use rattler_build::{
    build::run_build,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::PlatformWithVirtualPackages,
    recipe::{parser::BuildString, Jinja},
    render::resolved_dependencies::DependencyInfo,
    selectors::SelectorConfig,
    tool_configuration::Configuration,
};
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

    /// Returns a new instance of [`RattlerBuildBackend`] by reading the
    /// manifest at the given path.
    pub fn new(
        manifest_path: &Path,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
    ) -> miette::Result<Self> {
        // Locate the recipe
        let manifest_file_name = manifest_path.file_name().and_then(OsStr::to_str);
        let recipe_path = match manifest_file_name {
            Some("recipe.yaml") | Some("recipe.yml") => manifest_path.to_path_buf(),
            _ => {
                // The manifest is not a recipe, so we need to find the recipe.yaml file.
                let recipe_path = manifest_path.parent().and_then(|manifest_dir| {
                    [
                        "recipe.yaml",
                        "recipe.yml",
                        "recipe/recipe.yaml",
                        "recipe/recipe.yml",
                    ]
                    .into_iter()
                    .find_map(|relative_path| {
                        let recipe_path = manifest_dir.join(relative_path);
                        recipe_path.is_file().then_some(recipe_path)
                    })
                });

                recipe_path.ok_or_else(|| miette::miette!("Could not find a recipe.yaml in the source directory to use as the recipe manifest."))?
            }
        };

        // Load the manifest from the source directory
        let raw_recipe = fs::read_to_string(&recipe_path).into_diagnostic()?;

        Ok(Self {
            raw_recipe,
            recipe_path,
            logging_output_handler,
            cache_dir,
        })
    }

    /// Returns the capabilities of this backend based on the capabilities of
    /// the frontend.
    pub fn capabilities(
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

        let discovered_outputs =
            rattler_build_tool.discover_outputs(&params.variant_configuration)?;

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

        eprintln!("before outputs ");

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

            let selector_config = output.build_configuration.selector_config();

            let jinja = Jinja::new(selector_config.clone()).with_context(&output.recipe.context);

            let hash = HashInfo::from_variant(output.variant(), output.recipe.build().noarch());
            let build_string = output.recipe.build().string().resolve(
                &hash,
                output.recipe.build().number(),
                &jinja,
            );

            let conda = CondaPackageMetadata {
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

        let discovered_outputs =
            rattler_build_tool.discover_outputs(&params.variant_configuration)?;

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

        eprintln!("before outputs ");
        for output in outputs {
            let temp_recipe = TemporaryRenderedRecipe::from_output(&output)?;

            let tool_config = &tool_config;

            let mut output_with_build_string = output.clone();

            let selector_config = output.build_configuration.selector_config();

            let jinja = Jinja::new(selector_config.clone()).with_context(&output.recipe.context);

            let hash = HashInfo::from_variant(output.variant(), output.recipe.build().noarch());
            let build_string = output.recipe.build().string().resolve(
                &hash,
                output.recipe.build().number(),
                &jinja,
            );
            output_with_build_string.recipe.build.string =
                BuildString::Resolved(build_string.to_string());

            let (output, build_path) = temp_recipe
                .within_context_async(move || async move {
                    run_build(output_with_build_string, tool_config).await
                })
                .await?;

            built.push(CondaBuiltPackage {
                output_file: build_path,
                input_globs: Vec::from([self.recipe_path.to_string_lossy().to_string()]),
                name: output.name().as_normalized().to_string(),
                version: output.version().to_string(),
                build: build_string.to_string(),
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

        let capabilities = instance.capabilities(&params.capabilities);
        Ok((instance, InitializeResult { capabilities }))
    }
}

#[cfg(test)]
mod tests {
    use pixi_build_backend::protocol::{Protocol, ProtocolFactory};
    use pixi_build_types::{
        procedures::{
            conda_build::CondaBuildParams, conda_metadata::CondaMetadataParams,
            initialize::InitializeParams,
        },
        ChannelConfiguration, FrontendCapabilities,
    };
    use rattler_build::console_utils::LoggingOutputHandler;
    use serde_json::Value;
    use std::path::Path;
    use std::{path::PathBuf, str::FromStr};
    use tempfile::tempdir;
    use url::Url;

    use crate::rattler_build::RattlerBuildBackend;

    #[tokio::test]
    async fn test_get_conda_metadata() {
        // get cargo manifest dir
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = manifest_dir.join("../../recipe/recipe.yaml");

        let factory = RattlerBuildBackend::factory(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                manifest_path: recipe,
                configuration: Value::Null,
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
                variant_configuration: None,
            })
            .await
            .unwrap();

        assert_eq!(result.packages.len(), 3);
    }

    #[tokio::test]
    async fn test_conda_build() {
        // get cargo manifest dir
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = manifest_dir.join("../../tests/recipe/boltons/recipe.yaml");

        let factory = RattlerBuildBackend::factory(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                manifest_path: recipe,
                configuration: Value::Null,
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
                variant_configuration: None,
                editable: false,
            })
            .await
            .unwrap();

        assert_eq!(result.packages[0].name, "boltons-with-extra");
    }

    const FAKE_RECIPE: &str = r#"
    package:
      name: foobar
      version: 0.1.0
    "#;

    async fn try_initialize(
        manifest_path: impl AsRef<Path>,
    ) -> miette::Result<RattlerBuildBackend> {
        RattlerBuildBackend::factory(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                manifest_path: manifest_path.as_ref().to_path_buf(),
                configuration: Value::Null,
                capabilities: FrontendCapabilities {},
                cache_directory: None,
            })
            .await
            .map(|e| e.0)
    }

    #[tokio::test]
    async fn test_recipe_discovery() {
        let tmp = tempdir().unwrap();
        let recipe = tmp.path().join("recipe.yaml");
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_path,
            recipe
        );
        assert_eq!(try_initialize(&recipe).await.unwrap().recipe_path, recipe);

        let tmp = tempdir().unwrap();
        let recipe = tmp.path().join("recipe.yml");
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_path,
            recipe
        );
        assert_eq!(try_initialize(&recipe).await.unwrap().recipe_path, recipe);

        let tmp = tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        let recipe = recipe_dir.join("recipe.yaml");
        std::fs::create_dir(recipe_dir).unwrap();
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_path,
            recipe
        );

        let tmp = tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        let recipe = recipe_dir.join("recipe.yml");
        std::fs::create_dir(recipe_dir).unwrap();
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_path,
            recipe
        );
    }
}
