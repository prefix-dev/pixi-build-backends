use std::collections::HashMap;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use fs_err::tokio as tokio_fs;
use miette::{Context, IntoDiagnostic};
use pixi_build_backend::{
    protocol::{Protocol, ProtocolInstantiator},
    tools::RattlerBuild,
    utils::TemporaryRenderedRecipe,
};
use pixi_build_types::{
    BackendCapabilities, CondaPackageMetadata,
    procedures::{
        conda_build::{CondaBuildParams, CondaBuildResult, CondaBuiltPackage},
        conda_metadata::{CondaMetadataParams, CondaMetadataResult},
        initialize::{InitializeParams, InitializeResult},
        negotiate_capabilities::{NegotiateCapabilitiesParams, NegotiateCapabilitiesResult},
    },
};
use rattler_build::{
    build::run_build,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::PlatformWithVirtualPackages,
    recipe::{Jinja, parser::BuildString},
    render::resolved_dependencies::DependencyInfo,
    selectors::SelectorConfig,
    tool_configuration::{BaseClient, Configuration},
};
use rattler_conda_types::{ChannelConfig, MatchSpec, Platform};
use rattler_virtual_packages::VirtualPackageOverrides;
use url::Url;

use crate::{config::RattlerBuildBackendConfig, rattler_build::RattlerBuildBackend};
pub struct RattlerBuildBackendInstantiator {
    logging_output_handler: LoggingOutputHandler,
}

impl RattlerBuildBackendInstantiator {
    /// This type implements [`ProtocolInstantiator`] and can be used to
    /// initialize a new [`RattlerBuildBackend`].
    pub fn new(logging_output_handler: LoggingOutputHandler) -> RattlerBuildBackendInstantiator {
        RattlerBuildBackendInstantiator {
            logging_output_handler,
        }
    }
}

#[async_trait::async_trait]
impl Protocol for RattlerBuildBackend {
    fn debug_dir(&self) -> Option<&Path> {
        self.config.debug_dir.as_deref()
    }

    async fn conda_get_metadata(
        &self,
        params: CondaMetadataParams,
    ) -> miette::Result<CondaMetadataResult> {
        // Create the work directory if it does not exist
        tokio_fs::create_dir_all(&params.work_directory)
            .await
            .into_diagnostic()?;

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
            self.recipe_source.clone(),
            selector_config,
            params.work_directory.clone(),
        );

        let channel_config = ChannelConfig {
            channel_alias: params.channel_configuration.base_url,
            root_dir: self
                .recipe_source
                .path
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

        let base_client =
            BaseClient::new(None, None, HashMap::default(), HashMap::default()).unwrap();

        let tool_config = Configuration::builder()
            .with_opt_cache_dir(self.cache_dir.clone())
            .with_logging_output_handler(self.logging_output_handler.clone())
            .with_channel_config(channel_config.clone())
            .with_testing(false)
            .with_keep_build(true)
            .with_reqwest_client(base_client)
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
                version: output.version().clone(),
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
                sources: HashMap::new(),
            };
            solved_packages.push(conda);
        }

        let input_globs = Some(Vec::from(["recipe.yaml".to_string()]));

        Ok(CondaMetadataResult {
            packages: solved_packages,
            input_globs,
        })
    }

    async fn conda_build(&self, params: CondaBuildParams) -> miette::Result<CondaBuildResult> {
        // Create the work directory if it does not exist
        tokio_fs::create_dir_all(&params.work_directory)
            .await
            .into_diagnostic()?;

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
                .recipe_source
                .path
                .parent()
                .expect("should have parent")
                .to_path_buf(),
        };

        let channels = params
            .channel_base_urls
            .unwrap_or_else(|| vec![Url::from_str("https://prefix.dev/conda-forge").unwrap()]);

        let rattler_build_tool = RattlerBuild::new(
            self.recipe_source.clone(),
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

        let base_client =
            BaseClient::new(None, None, HashMap::default(), HashMap::default()).unwrap();

        let tool_config = Configuration::builder()
            .with_opt_cache_dir(self.cache_dir.clone())
            .with_logging_output_handler(self.logging_output_handler.clone())
            .with_channel_config(channel_config.clone())
            .with_testing(false)
            .with_keep_build(true)
            .with_reqwest_client(base_client)
            .finish();

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

            let package_sources = output.finalized_sources.as_ref().map(|package_sources| {
                package_sources
                    .iter()
                    .filter_map(|source| {
                        if let rattler_build::recipe::parser::Source::Path(path_source) = source {
                            Some(path_source.path.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            });

            built.push(CondaBuiltPackage {
                output_file: build_path,
                input_globs: build_input_globs(&self.recipe_source.path, package_sources)?,
                name: output.name().as_normalized().to_string(),
                version: output.version().to_string(),
                build: build_string.to_string(),
                subdir: output.target_platform().to_string(),
            });
        }
        Ok(CondaBuildResult { packages: built })
    }
}

#[allow(dead_code)]
/// Returns the relative path from `base` to `input`, joined by "/".
fn relative_path_joined(base: &std::path::Path, input: &std::path::Path) -> miette::Result<String> {
    let rel = pathdiff::diff_paths(input, base).ok_or_else(|| {
        miette::miette!(
            "could not compute relative path from '{:?}' to '{:?}'",
            input,
            base
        )
    })?;
    let joined = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Ok(joined)
}

fn build_input_globs(
    source: &Path,
    package_sources: Option<Vec<PathBuf>>,
) -> miette::Result<Vec<String>> {
    // Always add the current directory of the package to the globs
    let mut input_globs = vec!["*/**".to_string()];

    // TODO: Remove this condition when working on https://github.com/prefix-dev/pixi/issues/3785
    if !source.is_absolute() {
        return Ok(input_globs);
    }

    // Get parent directory path
    let parent = if source.is_file() {
        // use the parent path as glob
        source.parent().unwrap_or(source).to_path_buf()
    } else {
        // use the source path as glob
        source.to_path_buf()
    };
    // If there are sources add them to the globs as well
    if let Some(package_sources) = package_sources {
        for source in package_sources {
            let source_glob = relative_path_joined(&parent, &source)?;
            if source.is_dir() {
                input_globs.push(format!("{}/**", source_glob));
            } else {
                input_globs.push(source_glob);
            }
        }
    }

    Ok(input_globs)
}

#[async_trait::async_trait]
impl ProtocolInstantiator for RattlerBuildBackendInstantiator {
    fn debug_dir(configuration: Option<serde_json::Value>) -> Option<PathBuf> {
        configuration
            .and_then(|config| {
                serde_json::from_value::<RattlerBuildBackendConfig>(config.clone()).ok()
            })
            .and_then(|config| config.debug_dir)
    }
    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> miette::Result<(Box<dyn Protocol + Send + Sync + 'static>, InitializeResult)> {
        let config = if let Some(config) = params.configuration {
            serde_json::from_value(config)
                .into_diagnostic()
                .context("failed to parse configuration")?
        } else {
            RattlerBuildBackendConfig::default()
        };

        let instance = RattlerBuildBackend::new(
            params.manifest_path.as_path(),
            self.logging_output_handler.clone(),
            params.cache_directory,
            config,
        )?;

        Ok((Box::new(instance), InitializeResult {}))
    }

    async fn negotiate_capabilities(
        _params: NegotiateCapabilitiesParams,
    ) -> miette::Result<NegotiateCapabilitiesResult> {
        Ok(NegotiateCapabilitiesResult {
            capabilities: default_capabilities(),
        })
    }
}

pub(crate) fn default_capabilities() -> BackendCapabilities {
    BackendCapabilities {
        provides_conda_metadata: Some(true),
        provides_conda_build: Some(true),
        highest_supported_project_model: Some(
            pixi_build_types::VersionedProjectModel::highest_version(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        str::FromStr,
    };

    use pixi_build_types::{
        ChannelConfiguration,
        procedures::{
            conda_build::CondaBuildParams, conda_metadata::CondaMetadataParams,
            initialize::InitializeParams,
        },
    };
    use rattler_build::console_utils::LoggingOutputHandler;
    use tempfile::tempdir;
    use url::Url;

    use super::*;

    #[tokio::test]
    async fn test_conda_get_metadata() {
        // get cargo manifest dir
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = manifest_dir.join("../../tests/recipe/boltons/recipe.yaml");

        let factory = RattlerBuildBackendInstantiator::new(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                manifest_path: recipe,
                project_model: None,
                configuration: None,
                cache_directory: None,
            })
            .await
            .unwrap();

        let current_dir = std::env::current_dir().unwrap();

        let result = factory
            .0
            .conda_get_metadata(CondaMetadataParams {
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

        assert_eq!(result.packages.len(), 1);
    }

    #[tokio::test]
    async fn test_conda_build() {
        // get cargo manifest dir
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = manifest_dir.join("../../tests/recipe/boltons/recipe.yaml");

        let factory = RattlerBuildBackendInstantiator::new(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                manifest_path: recipe,
                project_model: None,
                configuration: None,
                cache_directory: None,
            })
            .await
            .unwrap();

        let current_dir = tempdir().unwrap();

        let result = factory
            .0
            .conda_build(CondaBuildParams {
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
        RattlerBuildBackend::new(
            manifest_path.as_ref(),
            LoggingOutputHandler::default(),
            None,
            RattlerBuildBackendConfig::default(),
        )
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
                .recipe_source
                .path,
            recipe
        );
        assert_eq!(
            try_initialize(&recipe).await.unwrap().recipe_source.path,
            recipe
        );

        let tmp = tempdir().unwrap();
        let recipe = tmp.path().join("recipe.yml");
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_source
                .path,
            recipe
        );
        assert_eq!(
            try_initialize(&recipe).await.unwrap().recipe_source.path,
            recipe
        );

        let tmp = tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        let recipe = recipe_dir.join("recipe.yaml");
        std::fs::create_dir(recipe_dir).unwrap();
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_source
                .path,
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
                .recipe_source
                .path,
            recipe
        );
    }

    #[test]
    fn test_relative_path_joined() {
        use std::path::Path;
        // Simple case
        let base = Path::new("/foo/bar");
        let input = Path::new("/foo/bar/baz/qux.txt");
        assert_eq!(
            super::relative_path_joined(base, input).unwrap(),
            "baz/qux.txt"
        );
        // Same path
        let base = Path::new("/foo/bar");
        let input = Path::new("/foo/bar");
        assert_eq!(super::relative_path_joined(base, input).unwrap(), "");
        // Input not under base
        let base = Path::new("/foo/bar");
        let input = Path::new("/foo/other");
        assert_eq!(
            super::relative_path_joined(base, input).unwrap(),
            "../other"
        );
        // Relative paths
        let base = Path::new("foo/bar");
        let input = Path::new("foo/bar/baz");
        assert_eq!(super::relative_path_joined(base, input).unwrap(), "baz");
    }

    #[test]
    #[cfg(windows)]
    fn test_relative_path_joined_windows() {
        use std::path::Path;
        let base = Path::new(r"C:\foo\bar");
        let input = Path::new(r"C:\foo\bar\baz\qux.txt");
        assert_eq!(
            super::relative_path_joined(base, input).unwrap(),
            "baz/qux.txt"
        );
        let base = Path::new(r"C:\foo\bar");
        let input = Path::new(r"C:\foo\bar");
        assert_eq!(super::relative_path_joined(base, input).unwrap(), "");
        let base = Path::new(r"C:\foo\bar");
        let input = Path::new(r"C:\foo\other");
        assert_eq!(
            super::relative_path_joined(base, input).unwrap(),
            "../other"
        );
    }

    #[test]
    fn test_build_input_globs_with_tempdirs() {
        use std::fs;
        use tempfile::tempdir;

        // Create a temp directory to act as the base
        let base_dir = tempdir().unwrap();
        let base_path = base_dir.path();

        // Case 1: source is a file in the base dir
        let recipe_path = base_path.join("recipe.yaml");
        fs::write(&recipe_path, "fake").unwrap();
        let globs = super::build_input_globs(&recipe_path, None).unwrap();
        assert_eq!(globs, vec!["*/**"]);

        // Case 2: source is a directory, with a file and a dir as package sources
        let pkg_dir = base_path.join("pkg");
        let pkg_file = pkg_dir.join("file.txt");
        let pkg_subdir = pkg_dir.join("dir");
        fs::create_dir_all(&pkg_subdir).unwrap();
        fs::write(&pkg_file, "fake").unwrap();
        let globs =
            super::build_input_globs(base_path, Some(vec![pkg_file.clone(), pkg_subdir.clone()]))
                .unwrap();
        assert_eq!(globs, vec!["*/**", "pkg/file.txt", "pkg/dir/**"]);
    }

    #[test]
    fn test_build_input_globs_two_folders_in_tempdir() {
        use std::fs;
        use tempfile::tempdir;

        // Create a temp directory
        let temp = tempdir().unwrap();
        let temp_path = temp.path();

        // Create two folders: source_dir and package_source_dir
        let source_dir = temp_path.join("source");
        let package_source_dir = temp_path.join("pkgsrc");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&package_source_dir).unwrap();

        // Call build_input_globs with source_dir as source, and package_source_dir as package source
        let globs =
            super::build_input_globs(&source_dir, Some(vec![package_source_dir.clone()])).unwrap();
        // The relative path from source_dir to package_source_dir should be "../pkgsrc/**"
        assert_eq!(globs, vec!["*/**", "../pkgsrc/**"]);
    }
}
