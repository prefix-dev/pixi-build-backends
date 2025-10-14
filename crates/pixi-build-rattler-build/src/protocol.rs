use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use crate::{config::RattlerBuildBackendConfig, rattler_build::RattlerBuildBackend};
use fs_err::tokio as tokio_fs;
use itertools::Itertools;
use miette::{Context, IntoDiagnostic};
use pixi_build_backend::specs_conversion::from_build_v1_args_to_finalized_dependencies;
use pixi_build_backend::{
    dependencies::{convert_binary_dependencies, convert_dependencies},
    intermediate_backend::{conda_build_v1_directories, find_matching_output},
    protocol::{Protocol, ProtocolInstantiator},
    tools::{LoadedVariantConfig, RattlerBuild},
    utils::TemporaryRenderedRecipe,
};
use pixi_build_types::{
    BackendCapabilities, CondaPackageMetadata, PathSpecV1, SourcePackageSpecV1, TargetV1,
    procedures::{
        conda_build_v0::{
            CondaBuildParams, CondaBuildResult, CondaBuiltPackage, CondaOutputIdentifier,
        },
        conda_build_v1::{CondaBuildV1Params, CondaBuildV1Result},
        conda_metadata::{CondaMetadataParams, CondaMetadataResult},
        conda_outputs::{
            CondaOutput, CondaOutputDependencies, CondaOutputIgnoreRunExports, CondaOutputMetadata,
            CondaOutputRunExports, CondaOutputsParams, CondaOutputsResult,
        },
        initialize::{InitializeParams, InitializeResult},
        negotiate_capabilities::{NegotiateCapabilitiesParams, NegotiateCapabilitiesResult},
    },
};
use rattler_build::{
    build::{WorkingDirectoryBehavior, run_build},
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{
        BuildConfiguration, Debug, Output, PackageIdentifier, PackagingSettings,
        PlatformWithVirtualPackages,
    },
    recipe::{
        Jinja, ParsingError, Recipe,
        parser::{BuildString, find_outputs_from_src},
        variable::Variable,
    },
    render::resolved_dependencies::DependencyInfo,
    selectors::SelectorConfig,
    tool_configuration::{BaseClient, Configuration},
    variant_config::{ParseErrors, VariantConfig},
};
use rattler_conda_types::{
    ChannelConfig, MatchSpec, PackageName, Platform, compression_level::CompressionLevel,
    package::ArchiveType,
};
use rattler_virtual_packages::VirtualPackageOverrides;
use url::Url;
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

        for output in &outputs {
            let temp_recipe = TemporaryRenderedRecipe::from_output(output)?;
            let tool_config = &tool_config;
            let output = temp_recipe
                .within_context_async(move || async move {
                    output
                        .clone()
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

            let depends = finalized_deps.depends.iter().map(DependencyInfo::spec);

            let mut sources: HashMap<String, SourcePackageSpecV1> = outputs
                .iter()
                .cartesian_product(depends.clone())
                .filter_map(|(output, depend)| {
                    if Some(output.name()) == depend.name.as_ref() {
                        Some(output.name())
                    } else {
                        None
                    }
                })
                .map(|name| {
                    (
                        name.as_source().to_string(),
                        SourcePackageSpecV1::Path(pixi_build_types::PathSpecV1 {
                            // Our source dependency lives in the same recipe
                            path: ".".to_string(),
                        }),
                    )
                })
                .collect();

            // Add workspace dependencies to the sources
            sources.extend(self.workspace_dependencies.clone());

            let conda = CondaPackageMetadata {
                name: output.name().clone(),
                version: output.version().clone(),
                build: build_string.to_string(),
                build_number: output.recipe.build.number,
                subdir: output.build_configuration.target_platform,
                depends: depends.map(MatchSpec::to_string).collect(),
                constraints: finalized_deps
                    .constraints
                    .iter()
                    .map(DependencyInfo::spec)
                    .map(MatchSpec::to_string)
                    .collect(),
                license: output.recipe.about.license.map(|l| l.to_string()),
                license_family: output.recipe.about.license_family,
                noarch: output.recipe.build.noarch,
                sources,
            };
            solved_packages.push(conda);
        }

        let input_globs = Some(get_metadata_input_globs(
            &self.manifest_root,
            &self.recipe_source.path,
        )?);

        Ok(CondaMetadataResult {
            packages: solved_packages,
            input_globs,
        })
    }

    async fn conda_outputs(
        &self,
        params: CondaOutputsParams,
    ) -> miette::Result<CondaOutputsResult> {
        let build_platform = params.host_platform;

        // Determine the variant configuration to use. This loads the variant
        // configuration from disk as well as including the variants from the input
        // parameters.
        let selector_config_for_variants = SelectorConfig {
            target_platform: params.host_platform,
            host_platform: params.host_platform,
            build_platform,
            hash: None,
            variant: Default::default(),
            experimental: false,
            allow_undefined: false,
            recipe_path: Some(self.recipe_source.path.clone()),
        };
        let variant_config = LoadedVariantConfig::from_recipe_path(
            &self.source_dir,
            &self.recipe_source.path,
            &selector_config_for_variants,
        )?
        .extend_with_input_variants(&params.variant_configuration.unwrap_or_default());

        // Find all outputs from the recipe
        let output_nodes = find_outputs_from_src(self.recipe_source.clone())?;
        let discovered_outputs = variant_config.variant_config.find_variants(
            &output_nodes,
            self.recipe_source.clone(),
            &selector_config_for_variants,
        )?;

        // Construct a mapping that for packages that we want from source.
        //
        // By default, this includes all the outputs in the recipe. These should all be
        // build from source, in particular from the current source.
        let mut local_source_packages: HashMap<String, SourcePackageSpecV1> = discovered_outputs
            .iter()
            .map(|output| {
                (
                    output.name.clone(),
                    SourcePackageSpecV1::Path(PathSpecV1 { path: ".".into() }),
                )
            })
            .collect();

        // Add workspace dependencies to the source packages mapping.
        // This allows the recipe to reference workspace packages by name (e.g., "my-lib")
        // and have them automatically resolved to source dependencies with the correct path.
        local_source_packages.extend(self.workspace_dependencies.clone());

        let mut subpackages = HashMap::new();
        let mut outputs = Vec::new();
        for discovered_output in discovered_outputs {
            let variant = discovered_output.used_vars;
            let hash = HashInfo::from_variant(&variant, &discovered_output.noarch_type);

            // Construct the selector config for this particular output. We base this on the
            // selector config that was used to determine the variants.
            let selector_config = SelectorConfig {
                variant: variant.clone(),
                hash: Some(hash.clone()),
                target_platform: discovered_output.target_platform,
                ..selector_config_for_variants.clone()
            };

            // Convert this discovered output into a recipe.
            let recipe = Recipe::from_node(&discovered_output.node, selector_config.clone())
                .map_err(|err| {
                    let errs: ParseErrors<_> = err
                        .into_iter()
                        .map(|err| ParsingError::from_partial(self.recipe_source.clone(), err))
                        .collect::<Vec<_>>()
                        .into();
                    errs
                })?;

            // Skip this output if the recipe is marked as skipped
            if recipe.build().skip() {
                continue;
            }

            let jinja = Jinja::new(selector_config);
            let build_number = recipe.build().number;
            let build_string = recipe.build().string().resolve(&hash, build_number, &jinja);

            subpackages.insert(
                recipe.package().name().clone(),
                PackageIdentifier {
                    name: recipe.package().name().clone(),
                    version: recipe.package().version().version().clone().into(),
                    build_string: build_string.to_string(),
                },
            );

            outputs.push(CondaOutput {
                metadata: CondaOutputMetadata {
                    name: recipe.package().name().clone(),
                    version: recipe.package.version().clone(),
                    build: build_string.to_string(),
                    build_number,
                    subdir: discovered_output.target_platform,
                    license: recipe.about.license.map(|l| l.to_string()),
                    license_family: recipe.about.license_family,
                    noarch: recipe.build.noarch,
                    purls: None,
                    python_site_packages_path: None,
                    variant: variant
                        .iter()
                        .map(|(k, v)| (k.0.clone(), v.to_string()))
                        .collect(),
                },
                build_dependencies: Some(CondaOutputDependencies {
                    depends: convert_dependencies(
                        recipe.requirements.build,
                        &variant,
                        &subpackages,
                        &local_source_packages,
                    )?,
                    constraints: Vec::new(),
                }),
                host_dependencies: Some(CondaOutputDependencies {
                    depends: convert_dependencies(
                        recipe.requirements.host,
                        &variant,
                        &subpackages,
                        &local_source_packages,
                    )?,
                    constraints: Vec::new(),
                }),
                run_dependencies: CondaOutputDependencies {
                    depends: convert_dependencies(
                        recipe.requirements.run,
                        &BTreeMap::default(), // Variants are not applied to run dependencies
                        &subpackages,
                        &local_source_packages,
                    )?,
                    constraints: convert_binary_dependencies(
                        recipe.requirements.run_constraints,
                        &BTreeMap::default(), // Variants are not applied to run constraints
                        &subpackages,
                    )?,
                },
                ignore_run_exports: CondaOutputIgnoreRunExports {
                    by_name: recipe
                        .requirements
                        .ignore_run_exports
                        .by_name
                        .into_iter()
                        .collect(),
                    from_package: recipe
                        .requirements
                        .ignore_run_exports
                        .from_package
                        .into_iter()
                        .collect(),
                },
                run_exports: CondaOutputRunExports {
                    weak: convert_dependencies(
                        recipe.requirements.run_exports.weak,
                        &variant,
                        &subpackages,
                        &local_source_packages,
                    )?,
                    strong: convert_dependencies(
                        recipe.requirements.run_exports.strong,
                        &variant,
                        &subpackages,
                        &local_source_packages,
                    )?,
                    noarch: convert_dependencies(
                        recipe.requirements.run_exports.noarch,
                        &variant,
                        &subpackages,
                        &local_source_packages,
                    )?,
                    weak_constrains: convert_binary_dependencies(
                        recipe.requirements.run_exports.weak_constraints,
                        &variant,
                        &subpackages,
                    )?,
                    strong_constrains: convert_binary_dependencies(
                        recipe.requirements.run_exports.strong_constraints,
                        &variant,
                        &subpackages,
                    )?,
                },

                // The input globs are the same for all outputs
                input_globs: None,
                // TODO: Implement caching
            });
        }

        let mut input_globs = variant_config.input_globs;
        input_globs.extend(get_metadata_input_globs(
            &self.manifest_root,
            &self.recipe_source.path,
        )?);

        Ok(CondaOutputsResult {
            outputs,
            input_globs,
        })
    }

    async fn conda_build_v0(&self, params: CondaBuildParams) -> miette::Result<CondaBuildResult> {
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
            recipe_path: Some(self.recipe_source.path.clone()),
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

        // Discover and filter the outputs.
        let mut discovered_outputs =
            rattler_build_tool.discover_outputs(&params.variant_configuration)?;
        if let Some(outputs) = &params.outputs {
            discovered_outputs.retain(|output| {
                let name = PackageName::from_str(&output.name)
                    .map_or_else(|_| output.name.clone(), |n| n.as_normalized().to_string());
                let id = CondaOutputIdentifier {
                    name: Some(name),
                    version: Some(output.version.clone()),
                    build: output.recipe.build.string.clone().into(),
                    subdir: Some(output.target_platform.to_string()),
                };
                outputs.contains(&id)
            });
        }

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
                    run_build(
                        output_with_build_string,
                        tool_config,
                        // rattler-build requires a clean work dir
                        WorkingDirectoryBehavior::Cleanup,
                    )
                    .await
                })
                .await?;

            built.push(CondaBuiltPackage {
                output_file: build_path,
                input_globs: build_input_globs(
                    &self.manifest_root,
                    &self.recipe_source.path,
                    extract_mutable_package_sources(&output),
                    self.config.extra_input_globs.clone(),
                )?,
                name: output.name().clone(),
                version: output.version().to_string(),
                build: build_string.to_string(),
                subdir: output.target_platform().to_string(),
            });
        }
        Ok(CondaBuildResult { packages: built })
    }

    async fn conda_build_v1(
        &self,
        params: CondaBuildV1Params,
    ) -> miette::Result<CondaBuildV1Result> {
        let host_platform = params
            .host_prefix
            .as_ref()
            .map_or_else(Platform::current, |prefix| prefix.platform);
        let build_platform = params
            .build_prefix
            .as_ref()
            .map_or_else(Platform::current, |prefix| prefix.platform);

        // Construct a `VariantConfig` based on the input parameters. We only
        // have a single variant here so we can just use the variant from the
        // parameters.
        let variant_config = VariantConfig {
            variants: params
                .output
                .variant
                .iter()
                .map(|(k, v)| (k.as_str().into(), vec![Variable::from_string(v)]))
                .collect(),
            pin_run_as_build: None,
            zip_keys: None,
        };

        // Determine the variant configuration to use. This loads the variant
        // configuration from disk as well as including the variants from the input
        // parameters.
        let selector_config_for_variants = SelectorConfig {
            target_platform: host_platform,
            host_platform,
            build_platform,
            hash: None,
            variant: Default::default(),
            experimental: false,
            allow_undefined: false,
            recipe_path: Some(self.recipe_source.path.clone()),
        };
        let outputs = find_outputs_from_src(self.recipe_source.clone())?;
        let discovered_outputs = variant_config.find_variants(
            &outputs,
            self.recipe_source.clone(),
            &selector_config_for_variants,
        )?;
        let discovered_output = find_matching_output(&params.output, discovered_outputs)?;

        // Set up the proper directories for the build.
        let directories = conda_build_v1_directories(
            params.host_prefix.as_ref().map(|p| p.prefix.as_path()),
            params.build_prefix.as_ref().map(|p| p.prefix.as_path()),
            params.work_directory,
            self.cache_dir.as_deref(),
            params.output_directory.as_deref(),
            self.recipe_source.path.clone(),
        );

        let tool_config = Configuration::builder()
            .with_opt_cache_dir(self.cache_dir.clone())
            .with_logging_output_handler(self.logging_output_handler.clone())
            .with_testing(false)
            // Pixi is incremental so keep the build
            .with_keep_build(true)
            // This indicates that the environments are externally managed, e.g. they are already
            // prepared.
            .with_environments_externally_managed(true)
            .finish();

        let output = Output {
            recipe: discovered_output.recipe,
            build_configuration: BuildConfiguration {
                target_platform: discovered_output.target_platform,
                host_platform: PlatformWithVirtualPackages {
                    platform: host_platform,
                    virtual_packages: vec![],
                },
                build_platform: PlatformWithVirtualPackages {
                    platform: build_platform,
                    virtual_packages: vec![],
                },
                hash: discovered_output.hash,
                variant: discovered_output.used_vars.clone(),
                directories,
                channels: vec![],
                channel_priority: Default::default(),
                solve_strategy: Default::default(),
                timestamp: chrono::Utc::now(),
                subpackages: BTreeMap::new(),
                packaging_settings: PackagingSettings::from_args(
                    ArchiveType::Conda,
                    CompressionLevel::default(),
                ),
                store_recipe: false,
                force_colors: true,
                sandbox_config: None,
                debug: Debug::new(false),
                exclude_newer: None,
            },
            finalized_dependencies: Some(from_build_v1_args_to_finalized_dependencies(
                params.build_prefix,
                params.host_prefix,
                params.run_dependencies,
                params.run_constraints,
                params.run_exports,
            )),
            finalized_sources: None,
            finalized_cache_dependencies: None,
            finalized_cache_sources: None,
            build_summary: Arc::default(),
            system_tools: Default::default(),
            extra_meta: None,
        };

        let (output, output_path) =
            // rattler-build requires a clean work dir
            run_build(output, &tool_config, WorkingDirectoryBehavior::Cleanup).await?;

        Ok(CondaBuildV1Result {
            output_file: output_path,
            input_globs: build_input_globs(
                &self.manifest_root,
                &self.recipe_source.path,
                extract_mutable_package_sources(&output),
                self.config.extra_input_globs.clone(),
            )?,
            name: output.name().as_normalized().to_string(),
            version: output.version().clone(),
            build: output.build_string().into_owned(),
            subdir: *output.target_platform(),
        })
    }
}

/// Extracts the package sources from an `Output` object that are mutable and
/// should be watched for changes.
fn extract_mutable_package_sources(output: &Output) -> Option<Vec<PathBuf>> {
    output.finalized_sources.as_ref().map(|package_sources| {
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
    })
}

/// Returns the relative path from `base` to `input`, joined by "/".
fn build_relative_glob(base: &std::path::Path, input: &std::path::Path) -> miette::Result<String> {
    // Get the difference between paths
    let rel = pathdiff::diff_paths(input, base).ok_or_else(|| {
        miette::miette!(
            "could not compute relative path from '{:?}' to '{:?}'",
            input,
            base
        )
    })?;

    // Normalize the path
    let joined = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");

    if input.is_dir() {
        // This means the base is the same as the input
        // just use `**` in that case that matches everything
        if joined.is_empty() {
            Ok("**".to_string())
        } else {
            Ok(format!("{joined}/**"))
        }
    } else {
        // This is a file so lets just use that
        Ok(joined)
    }
}

fn build_input_globs(
    manifest_root: &Path,
    source: &Path,
    package_sources: Option<Vec<PathBuf>>,
    extra_globs: Vec<String>,
) -> miette::Result<BTreeSet<String>> {
    // Get parent directory path
    let src_parent = if source.is_file() {
        // use the parent path as glob
        source.parent().unwrap_or(source).to_path_buf()
    } else {
        // use the source path as glob
        source.to_path_buf()
    };

    // Always add the current directory of the package to the globs
    let mut input_globs = BTreeSet::from([build_relative_glob(manifest_root, &src_parent)?]);

    // If there are sources add them to the globs as well
    if let Some(package_sources) = package_sources {
        for source in package_sources {
            let source = if source.is_absolute() {
                source
            } else {
                src_parent.join(source)
            };
            input_globs.insert(build_relative_glob(manifest_root, &source)?);
        }
    }

    // Extend with extra input globs
    input_globs.extend(extra_globs);

    Ok(input_globs)
}

/// Returns the input globs for conda_get_metadata, as used in the
/// CondaMetadataResult.
fn get_metadata_input_globs(
    manifest_root: &Path,
    recipe_source_path: &Path,
) -> miette::Result<BTreeSet<String>> {
    match build_relative_glob(manifest_root, recipe_source_path) {
        Ok(rel) if !rel.is_empty() => Ok(BTreeSet::from_iter([rel])),
        Ok(_) => Ok(Default::default()),
        Err(e) => Err(e),
    }
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

        let mut workspace_dependencies = HashMap::new();

        if let Some(target) = params
            .project_model
            .and_then(|m| m.into_v1())
            .and_then(|m| m.targets)
        {
            fn extract_workspace_deps(
                target: TargetV1,
                workspace_deps: &mut HashMap<String, SourcePackageSpecV1>,
            ) -> miette::Result<()> {
                for dep_list in [
                    target.build_dependencies,
                    target.host_dependencies,
                    target.run_dependencies,
                ] {
                    let Some(deps) = dep_list else {
                        continue;
                    };

                    for (name, spec) in deps {
                        match spec {
                            pixi_build_types::PackageSpecV1::Source(source_spec) => {
                                // Source dependencies are allowed - they represent workspace packages
                                workspace_deps.insert(name, source_spec);
                            }
                            pixi_build_types::PackageSpecV1::Binary(_) => {
                                // Binary dependencies must be specified in the recipe, not here
                                return Err(miette::miette!(
                                    "Binary dependency '{}' is not allowed in pixi-build-rattler-build. Please specify all binary dependencies in the recipe.",
                                    name
                                ));
                            }
                        }
                    }
                }
                Ok(())
            }

            if let Some(default_target) = target.default_target {
                extract_workspace_deps(default_target, &mut workspace_dependencies)?;
            }

            if let Some(targets) = target.targets {
                for (_, target) in targets {
                    extract_workspace_deps(target, &mut workspace_dependencies)?;
                }
            }
        }

        let mut instance = RattlerBuildBackend::new(
            params.source_dir,
            params.manifest_path.as_path(),
            self.logging_output_handler.clone(),
            params.cache_directory,
            config,
        )?;

        // Set the workspace dependencies
        instance.workspace_dependencies = workspace_dependencies;

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
        provides_conda_outputs: Some(true),
        provides_conda_build_v1: Some(true),
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

    use pixi_build_backend::utils::test::conda_outputs_snapshot;
    use pixi_build_types::{
        ChannelConfiguration,
        procedures::{
            conda_build_v0::CondaBuildParams, conda_metadata::CondaMetadataParams,
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
                workspace_root: None,
                source_dir: None,
                manifest_path: recipe,
                project_model: None,
                configuration: None,
                target_configuration: None,
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

    #[test]
    fn test_conda_outputs() {
        insta::glob!("../../../tests/recipe", "*/recipe.yaml", |recipe_path| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let factory = RattlerBuildBackendInstantiator::new(LoggingOutputHandler::default())
                    .initialize(InitializeParams {
                        workspace_root: None,
                        source_dir: None,
                        manifest_path: recipe_path.to_path_buf(),
                        project_model: None,
                        configuration: None,
                        target_configuration: None,
                        cache_directory: None,
                    })
                    .await
                    .unwrap();

                let current_dir = std::env::current_dir().unwrap();

                let result = factory
                    .0
                    .conda_outputs(CondaOutputsParams {
                        channels: vec![],
                        host_platform: Platform::Linux64,
                        build_platform: Platform::Linux64,
                        variant_configuration: None,
                        work_directory: current_dir,
                    })
                    .await
                    .unwrap();

                insta::assert_snapshot!(conda_outputs_snapshot(result));
            });
        });
    }

    #[tokio::test]
    async fn test_conda_build() {
        // get cargo manifest dir
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = manifest_dir.join("../../tests/recipe/boltons/recipe.yaml");

        let factory = RattlerBuildBackendInstantiator::new(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                workspace_root: None,
                source_dir: None,
                manifest_path: recipe,
                project_model: None,
                configuration: None,
                target_configuration: None,
                cache_directory: None,
            })
            .await
            .unwrap();

        let current_dir = tempdir().unwrap();

        let result = factory
            .0
            .conda_build_v0(CondaBuildParams {
                build_platform_virtual_packages: None,
                host_platform: None,
                channel_base_urls: None,
                channel_configuration: ChannelConfiguration {
                    base_url: Url::from_str("https://prefix.dev").unwrap(),
                },
                outputs: None,
                work_directory: current_dir.keep(),
                variant_configuration: None,
                editable: false,
            })
            .await
            .unwrap();

        assert_eq!(
            result.packages[0].name.as_normalized(),
            "boltons-with-extra"
        );
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
            None,
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
            super::build_relative_glob(base, input).unwrap(),
            "baz/qux.txt"
        );
        // Same path
        let base = Path::new("/foo/bar");
        let input = Path::new("/foo/bar");
        assert_eq!(super::build_relative_glob(base, input).unwrap(), "");
        // Input not under base
        let base = Path::new("/foo/bar");
        let input = Path::new("/foo/other");
        assert_eq!(super::build_relative_glob(base, input).unwrap(), "../other");
        // Relative paths
        let base = Path::new("foo/bar");
        let input = Path::new("foo/bar/baz");
        assert_eq!(super::build_relative_glob(base, input).unwrap(), "baz");
    }

    #[test]
    #[cfg(windows)]
    fn test_relative_path_joined_windows() {
        use std::path::Path;
        let base = Path::new(r"C:\foo\bar");
        let input = Path::new(r"C:\foo\bar\baz\qux.txt");
        assert_eq!(
            super::build_relative_glob(base, input).unwrap(),
            "baz/qux.txt"
        );
        let base = Path::new(r"C:\foo\bar");
        let input = Path::new(r"C:\foo\bar");
        assert_eq!(super::build_relative_glob(base, input).unwrap(), "");
        let base = Path::new(r"C:\foo\bar");
        let input = Path::new(r"C:\foo\other");
        assert_eq!(super::build_relative_glob(base, input).unwrap(), "../other");
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
        let globs = super::build_input_globs(base_path, &recipe_path, None, Vec::new()).unwrap();
        assert_eq!(globs, BTreeSet::from([String::from("**")]));

        // Case 2: source is a directory, with a file and a dir as package sources
        let pkg_dir = base_path.join("pkg");
        let pkg_file = pkg_dir.join("file.txt");
        let pkg_subdir = pkg_dir.join("dir");
        fs::create_dir_all(&pkg_subdir).unwrap();
        fs::write(&pkg_file, "fake").unwrap();
        let globs = super::build_input_globs(
            base_path,
            base_path,
            Some(vec![pkg_file.clone(), pkg_subdir.clone()]),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            globs,
            BTreeSet::from([
                String::from("**"),
                String::from("pkg/file.txt"),
                String::from("pkg/dir/**")
            ])
        );
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

        // Call build_input_globs with source_dir as source, and package_source_dir as
        // package source
        let globs = super::build_input_globs(
            temp_path,
            &source_dir,
            Some(vec![package_source_dir.clone()]),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            globs,
            BTreeSet::from([String::from("source/**"), String::from("pkgsrc/**")])
        );
    }

    #[test]
    fn test_build_input_globs_relative_source() {
        use std::{fs, path::PathBuf};

        use tempfile::tempdir;

        // Create a temp directory to act as the base
        let base_dir = tempdir().unwrap();
        let base_path = base_dir.path();

        // Case: source is a directory, package_sources contains a relative path
        let rel_dir = PathBuf::from("rel_folder");
        let abs_rel_dir = base_path.join(&rel_dir);
        fs::create_dir_all(&abs_rel_dir).unwrap();

        // Call build_input_globs with base_path as source, and rel_dir as package
        // source (relative)
        let globs = super::build_input_globs(
            base_path,
            base_path,
            Some(vec![rel_dir.clone()]),
            Vec::new(),
        )
        .unwrap();
        // The relative path from base_path to rel_dir should be "rel_folder/**"
        assert_eq!(
            globs,
            BTreeSet::from_iter(["**", "rel_folder/**"].into_iter().map(ToString::to_string))
        );
    }

    #[test]
    fn test_get_metadata_input_globs() {
        use std::path::PathBuf;
        // Case: file with name
        let manifest_root = PathBuf::from("/foo/bar");
        let path = PathBuf::from("/foo/bar/recipe.yaml");
        let globs = super::get_metadata_input_globs(&manifest_root, &path).unwrap();
        assert_eq!(globs, BTreeSet::from([String::from("recipe.yaml")]));
        // Case: file with no name (root)
        let manifest_root = PathBuf::from("/");
        let path = PathBuf::from("/");
        let globs = super::get_metadata_input_globs(&manifest_root, &path).unwrap();
        assert_eq!(globs, BTreeSet::from([String::from("**")]));
        // Case: file with .yml extension
        let manifest_root = PathBuf::from("/foo/bar");
        let path = PathBuf::from("/foo/bar/recipe.yml");
        let globs = super::get_metadata_input_globs(&manifest_root, &path).unwrap();
        assert_eq!(globs, BTreeSet::from([String::from("recipe.yml")]));
        // Case: file in subdir
        let manifest_root = PathBuf::from("/foo");
        let path = PathBuf::from("/foo/bar/recipe.yaml");
        let globs = super::get_metadata_input_globs(&manifest_root, &path).unwrap();
        assert_eq!(globs, BTreeSet::from([String::from("bar/recipe.yaml")]));
    }

    #[test]
    fn test_build_input_globs_includes_extra_globs() {
        use std::fs;

        use tempfile::tempdir;

        // Create a temp directory to act as the base
        let base_dir = tempdir().unwrap();
        let base_path = base_dir.path();

        // Create a recipe file
        let recipe_path = base_path.join("recipe.yaml");
        fs::write(&recipe_path, "fake").unwrap();

        // Test with extra globs
        let extra_globs = vec!["custom/*.txt".to_string(), "extra/**/*.py".to_string()];
        let globs =
            super::build_input_globs(base_path, &recipe_path, None, extra_globs.clone()).unwrap();

        // Verify that all extra globs are included in the result
        for extra_glob in &extra_globs {
            assert!(
                globs.contains(extra_glob),
                "Result should contain extra glob: {extra_glob}"
            );
        }

        // Verify that the basic manifest glob is still present
        assert!(globs.contains("**"));
    }
}
