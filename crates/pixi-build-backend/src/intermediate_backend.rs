use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use indexmap::IndexMap;
use itertools::Itertools;
use miette::{Context, IntoDiagnostic};
use pixi_build_types::{
    BackendCapabilities, CondaPackageMetadata, ProjectModelV1,
    procedures::{
        conda_build::{
            CondaBuildParams, CondaBuildResult, CondaBuiltPackage, CondaOutputIdentifier,
        },
        conda_metadata::{CondaMetadataParams, CondaMetadataResult},
        initialize::{InitializeParams, InitializeResult},
        negotiate_capabilities::{NegotiateCapabilitiesParams, NegotiateCapabilitiesResult},
    },
};
use rattler_build::{
    build::run_build,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{
        BuildConfiguration, Debug, Output, PackageIdentifier, PackagingSettings,
        PlatformWithVirtualPackages,
    },
    recipe::{
        ParsingError, Recipe,
        parser::{BuildString, find_outputs_from_src},
        variable::Variable,
    },
    render::resolved_dependencies::DependencyInfo,
    selectors::SelectorConfig,
    source_code::Source,
    system_tools::SystemTools,
    tool_configuration::Configuration,
    variant_config::{ParseErrors, VariantConfig},
};
use rattler_conda_types::{ChannelConfig, MatchSpec, Platform, package::ArchiveType};
use rattler_package_streaming::write::CompressionLevel;
use recipe_stage0::matchspec::{PackageDependency, SerializableMatchSpec};
use serde::Deserialize;

use crate::{
    generated_recipe::{BackendConfig, GenerateRecipe, PythonParams},
    protocol::{Protocol, ProtocolInstantiator},
    specs_conversion::from_source_matchspec_into_package_spec,
    tools::{OneOrMultipleOutputs, output_directory},
    utils::TemporaryRenderedRecipe,
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IntermediateBackendConfig {
    /// Environment Variables
    #[serde(default)]
    pub env: IndexMap<String, String>,
    /// If set, internal state will be logged as files in that directory
    pub debug_dir: Option<PathBuf>,
}

pub struct IntermediateBackendInstantiator<T: GenerateRecipe> {
    logging_output_handler: LoggingOutputHandler,

    generator: T,
}

impl<T: GenerateRecipe + Default + Clone> IntermediateBackendInstantiator<T> {
    pub fn new(logging_output_handler: LoggingOutputHandler) -> Self {
        Self {
            logging_output_handler,
            generator: T::default(),
        }
    }
}

pub struct IntermediateBackend<T: GenerateRecipe + Clone> {
    pub(crate) logging_output_handler: LoggingOutputHandler,
    pub(crate) source_dir: PathBuf,
    /// The path to the manifest file relative to the source directory.
    pub(crate) manifest_rel_path: PathBuf,
    pub(crate) project_model: ProjectModelV1,
    pub(crate) generate_recipe: T,
    pub(crate) config: T::Config,
    pub(crate) cache_dir: Option<PathBuf>,
}
impl<T: GenerateRecipe + Clone> IntermediateBackend<T> {
    pub fn new(
        manifest_path: PathBuf,
        project_model: ProjectModelV1,
        generate_recipe: T,
        config: serde_json::Value,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
    ) -> miette::Result<Self> {
        // Determine the root directory of the manifest
        let manifest_root = manifest_path
            .parent()
            .ok_or_else(|| miette::miette!("the project manifest must reside in a directory"))?
            .to_path_buf();

        let config = serde_json::from_value::<T::Config>(config)
            .into_diagnostic()
            .context("failed to parse configuration")?;

        Ok(Self {
            manifest_rel_path: pathdiff::diff_paths(manifest_path, &manifest_root)
                .expect("must be relative"),
            source_dir: manifest_root,
            project_model,
            generate_recipe,
            config,
            logging_output_handler,
            cache_dir,
        })
    }
}

#[async_trait::async_trait]
impl<T> ProtocolInstantiator for IntermediateBackendInstantiator<T>
where
    T: GenerateRecipe + Clone + Send + Sync + 'static,
    T::Config: BackendConfig + Send + Sync + 'static,
{
    fn debug_dir(configuration: Option<serde_json::Value>) -> Option<PathBuf> {
        let config = configuration
            .and_then(|config| serde_json::from_value::<T::Config>(config.clone()).ok())
            .and_then(|config| config.debug_dir().map(|d| d.to_path_buf()));

        config
    }

    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> miette::Result<(Box<dyn Protocol + Send + Sync + 'static>, InitializeResult)> {
        let project_model = params
            .project_model
            .ok_or_else(|| miette::miette!("project model is required"))?;

        let project_model = project_model
            .into_v1()
            .ok_or_else(|| miette::miette!("project model v1 is required"))?;

        let config = if let Some(config) = params.configuration {
            config
        } else {
            serde_json::Value::Object(Default::default())
        };

        let instance = IntermediateBackend::<T>::new(
            params.manifest_path,
            project_model,
            self.generator.clone(),
            config,
            self.logging_output_handler.clone(),
            params.cache_directory,
        )?;

        Ok((Box::new(instance), InitializeResult {}))
    }

    async fn negotiate_capabilities(
        _params: NegotiateCapabilitiesParams,
    ) -> miette::Result<NegotiateCapabilitiesResult> {
        // Returns the capabilities of this backend based on the capabilities of
        // the frontend.
        Ok(NegotiateCapabilitiesResult {
            capabilities: default_capabilities(),
        })
    }
}

#[async_trait::async_trait]
impl<T> Protocol for IntermediateBackend<T>
where
    T: GenerateRecipe + Clone + Send + Sync + 'static,
    T::Config: BackendConfig + Send + Sync + 'static,
{
    fn debug_dir(&self) -> Option<&Path> {
        self.config.debug_dir()
    }

    async fn conda_get_metadata(
        &self,
        params: CondaMetadataParams,
    ) -> miette::Result<CondaMetadataResult> {
        let channel_config = ChannelConfig {
            channel_alias: params.channel_configuration.base_url,
            root_dir: self.source_dir.to_path_buf(),
        };

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

        // Construct the intermediate recipe
        let generated_recipe = self.generate_recipe.generate_recipe(
            &self.project_model,
            &self.config,
            self.source_dir.clone(),
            host_platform,
            Some(PythonParams { editable: false }),
        )?;

        // Convert the recipe to source code.
        // TODO(baszalmstra): In the future it would be great if we could just
        // immediately use the intermediate recipe for some of this rattler-build
        // functions.
        let recipe_path = self.source_dir.join(&self.manifest_rel_path);
        let named_source = Source {
            name: self.manifest_rel_path.display().to_string(),
            code: Arc::from(
                generated_recipe
                    .recipe
                    .to_yaml_pretty()
                    .into_diagnostic()?
                    .as_str(),
            ),
            path: recipe_path.clone(),
        };

        // Construct a `VariantConfig` based on the input parameters.
        //
        // rattler-build recipes would also load variant.yaml (or
        // conda-build-config.yaml) files here, but we only respect the variant
        // configuration passed in.
        //
        // Determine the variant configuration to use. This is a combination of defaults
        // from the generator and the user supplied parameters. The parameters
        // from the user take precedence over the default variants.
        let recipe_variants = self.generate_recipe.default_variants(host_platform);
        let mut param_variant_configuration = params
            .variant_configuration
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| {
                (
                    k.into(),
                    v.into_iter().map(|v| Variable::from_string(&v)).collect(),
                )
            })
            .collect();
        let mut variants = recipe_variants;
        variants.append(&mut param_variant_configuration);
        let variant_config = VariantConfig {
            variants,
            pin_run_as_build: None,
            zip_keys: None,
        };

        // Determine the different outputs that are supported by the recipe by expanding
        // all the different variant combinations.
        //
        // TODO(baszalmstra): The selector config we pass in here doesnt have all values
        // filled in. This is on prupose because at this point we dont yet know all
        // values like the variant. We should introduce a new type of selector config
        // for this particular case.
        let selector_config_for_variants = SelectorConfig {
            target_platform: host_platform,
            host_platform,
            build_platform,
            hash: None,
            variant: Default::default(),
            experimental: false,
            allow_undefined: false,
        };
        let outputs = find_outputs_from_src(named_source.clone())?;
        let discovered_outputs = variant_config.find_variants(
            &outputs,
            named_source.clone(),
            &selector_config_for_variants,
        )?;

        // Build the tool configuration
        let tool_config = Arc::new(
            Configuration::builder()
                .with_opt_cache_dir(self.cache_dir.clone())
                .with_logging_output_handler(self.logging_output_handler.clone())
                .with_channel_config(channel_config)
                .with_testing(false)
                .with_keep_build(true)
                .finish(),
        );

        let timestamp = chrono::Utc::now();
        let mut subpackages = BTreeMap::new();
        let mut packages = Vec::new();
        let number_of_outputs = discovered_outputs.len();
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
                        .map(|err| ParsingError::from_partial(named_source.clone(), err))
                        .collect::<Vec<_>>()
                        .into();
                    errs
                })?;

            // Skip this output if the recipe is marked as skipped
            if recipe.build().skip() {
                continue;
            }

            subpackages.insert(
                recipe.package().name().clone(),
                PackageIdentifier {
                    name: recipe.package().name().clone(),
                    version: recipe.package().version().clone(),
                    build_string: discovered_output.build_string.clone(),
                },
            );

            let mut output = Output {
                recipe,
                build_configuration: BuildConfiguration {
                    target_platform: discovered_output.target_platform,
                    host_platform: PlatformWithVirtualPackages {
                        platform: selector_config.host_platform,
                        virtual_packages: params
                            .host_platform
                            .as_ref()
                            .map(|p| p.virtual_packages.clone().unwrap_or_default())
                            .unwrap_or_default(),
                    },
                    build_platform: PlatformWithVirtualPackages {
                        platform: selector_config.build_platform,
                        virtual_packages: params
                            .build_platform
                            .as_ref()
                            .map(|p| p.virtual_packages.clone().unwrap_or_default())
                            .unwrap_or_default(),
                    },
                    hash: discovered_output.hash.clone(),
                    variant,
                    directories: output_directory(
                        if number_of_outputs == 1 {
                            OneOrMultipleOutputs::Single(discovered_output.name.clone())
                        } else {
                            OneOrMultipleOutputs::OneOfMany(discovered_output.name.clone())
                        },
                        params.work_directory.clone(),
                        &named_source.path,
                    ),
                    channels: params
                        .channel_base_urls
                        .iter()
                        .flatten()
                        .cloned()
                        .map(Into::into)
                        .collect(),
                    channel_priority: tool_config.channel_priority,
                    timestamp,
                    subpackages: subpackages.clone(),
                    packaging_settings: PackagingSettings::from_args(
                        ArchiveType::Conda,
                        CompressionLevel::default(),
                    ),
                    store_recipe: false,
                    force_colors: false,
                    sandbox_config: None,
                    debug: Debug::default(),
                    solve_strategy: Default::default(),
                },
                finalized_dependencies: None,
                finalized_sources: None,
                finalized_cache_dependencies: None,
                finalized_cache_sources: None,
                system_tools: SystemTools::default(),
                build_summary: Arc::default(),
                extra_meta: None,
            };

            output.recipe.build.string = BuildString::Resolved(discovered_output.build_string);

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

            let finalized_run_deps = &output
                .finalized_dependencies
                .as_ref()
                .expect("dependencies should be resolved at this point")
                .run
                .depends
                .iter()
                .cloned()
                .map(|dep| {
                    let spec = dep.spec().clone();
                    let ser_matchspec = SerializableMatchSpec(spec);

                    PackageDependency::from(ser_matchspec)
                })
                .collect_vec();

            let source_dependencies = finalized_run_deps
                .iter()
                .filter_map(|dep| dep.as_source().cloned())
                .collect_vec();

            let source_spec_v1 = source_dependencies
                .iter()
                .map(|dep| {
                    let name = dep
                        .spec
                        .name
                        .as_ref()
                        .ok_or_else(|| {
                            miette::miette!("source dependency {} does not have a name", dep.spec)
                        })?
                        .as_normalized()
                        .to_string();
                    Ok((name, from_source_matchspec_into_package_spec(dep.clone())?))
                })
                .collect::<miette::Result<HashMap<_, _>>>()?;

            packages.push(CondaPackageMetadata {
                name: output.name().clone(),
                version: output.version().clone(),
                build: output.build_string().into_owned(),
                build_number: output.recipe.build.number,
                subdir: output.build_configuration.target_platform,
                depends: finalized_run_deps
                    .iter()
                    .sorted_by_key(|dep| dep.package_name())
                    .map(|package_dependency| {
                        SerializableMatchSpec::from(package_dependency.clone())
                            .0
                            .clone()
                    })
                    .map(|mut arg| {
                        // reset the URL for source dependencies
                        arg.url = None;
                        arg.to_string()
                    })
                    .collect(),
                constraints: finalized_deps
                    .constraints
                    .iter()
                    .map(DependencyInfo::spec)
                    .map(MatchSpec::to_string)
                    .collect(),
                license: output.recipe.about.license.as_ref().map(|l| l.to_string()),
                license_family: output.recipe.about.license_family.clone(),
                noarch: output.recipe.build.noarch,
                sources: source_spec_v1,
            });
        }

        Ok(CondaMetadataResult {
            packages,
            input_globs: Some(generated_recipe.metadata_input_globs),
        })
    }

    async fn conda_build(&self, params: CondaBuildParams) -> miette::Result<CondaBuildResult> {
        let channel_config = ChannelConfig {
            channel_alias: params.channel_configuration.base_url,
            root_dir: self.source_dir.to_path_buf(),
        };

        let host_platform = params
            .host_platform
            .as_ref()
            .map(|p| p.platform)
            .unwrap_or(Platform::current());

        let build_platform = Platform::current();

        // Construct the intermediate recipe
        let generated_recipe = self.generate_recipe.generate_recipe(
            &self.project_model,
            &self.config,
            self.source_dir.clone(),
            host_platform,
            Some(PythonParams {
                editable: params.editable,
            }),
        )?;

        // Convert the recipe to source code.
        // TODO(baszalmstra): In the future it would be great if we could just
        // immediately use the intermediate recipe for some of this rattler-build
        // functions.
        let recipe_path = self.source_dir.join(&self.manifest_rel_path);
        let named_source = Source {
            name: self.manifest_rel_path.display().to_string(),
            code: Arc::from(
                generated_recipe
                    .recipe
                    .to_yaml_pretty()
                    .into_diagnostic()?
                    .as_str(),
            ),
            path: recipe_path.clone(),
        };

        // Construct a `VariantConfig` based on the input parameters.
        //
        // rattler-build recipes would also load variant.yaml (or
        // conda-build-config.yaml) files here, but we only respect the variant
        // configuration passed in.
        //
        // Determine the variant configuration to use. This is a combination of defaults
        // from the generator and the user supplied parameters. The parameters
        // from the user take precedence over the default variants.
        let recipe_variants = self.generate_recipe.default_variants(host_platform);
        let mut param_variant_configuration = params
            .variant_configuration
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| {
                (
                    k.into(),
                    v.into_iter().map(|v| Variable::from_string(&v)).collect(),
                )
            })
            .collect();
        let mut variants = recipe_variants;
        variants.append(&mut param_variant_configuration);
        let variant_config = VariantConfig {
            variants,
            pin_run_as_build: None,
            zip_keys: None,
        };

        // Determine the different outputs that are supported by the recipe by expanding
        // all the different variant combinations.
        //
        // TODO(baszalmstra): The selector config we pass in here doesnt have all values
        // filled in. This is on prupose because at this point we dont yet know all
        // values like the variant. We should introduce a new type of selector config
        // for this particular case.
        let selector_config_for_variants = SelectorConfig {
            target_platform: host_platform,
            host_platform,
            build_platform,
            hash: None,
            variant: Default::default(),
            experimental: false,
            allow_undefined: false,
        };
        let outputs = find_outputs_from_src(named_source.clone())?;
        let mut discovered_outputs = variant_config.find_variants(
            &outputs,
            named_source.clone(),
            &selector_config_for_variants,
        )?;

        // Build the tool configuration
        let tool_config = Arc::new(
            Configuration::builder()
                .with_opt_cache_dir(self.cache_dir.clone())
                .with_logging_output_handler(self.logging_output_handler.clone())
                .with_channel_config(channel_config)
                .with_testing(false)
                .with_keep_build(true)
                .finish(),
        );

        // Filter on only the outputs that the user requested
        // Determine the outputs to build
        let selected_outputs = if let Some(output_identifiers) = params.outputs.clone() {
            output_identifiers
                .into_iter()
                .filter_map(|iden| {
                    let pos = discovered_outputs.iter().position(|output| {
                        let CondaOutputIdentifier {
                            name,
                            version,
                            build,
                            subdir,
                        } = &iden;
                        name.as_ref().is_none_or(|n| output.name == *n)
                            && version.as_ref().is_none_or(|v| output.version == *v)
                            && build
                                .as_ref()
                                .is_none_or(|b| output.build_string == b.as_str())
                            && subdir
                                .as_ref()
                                .is_none_or(|s| output.target_platform.as_str() == s)
                    })?;
                    discovered_outputs.swap_remove_index(pos)
                })
                .collect()
        } else {
            discovered_outputs
        };

        let timestamp = chrono::Utc::now();
        let mut subpackages = BTreeMap::new();

        let mut packages = Vec::new();
        let number_of_outputs = selected_outputs.len();
        for discovered_output in selected_outputs {
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
                        .map(|err| ParsingError::from_partial(named_source.clone(), err))
                        .collect::<Vec<_>>()
                        .into();
                    errs
                })?;

            // Skip this output if the recipe is marked as skipped
            if recipe.build().skip() {
                continue;
            }

            subpackages.insert(
                recipe.package().name().clone(),
                PackageIdentifier {
                    name: recipe.package().name().clone(),
                    version: recipe.package().version().clone(),
                    build_string: discovered_output.build_string.clone(),
                },
            );

            let mut output = Output {
                recipe,
                build_configuration: BuildConfiguration {
                    target_platform: discovered_output.target_platform,
                    host_platform: PlatformWithVirtualPackages {
                        platform: selector_config.host_platform,
                        virtual_packages: params
                            .host_platform
                            .as_ref()
                            .map(|p| p.virtual_packages.clone().unwrap_or_default())
                            .unwrap_or_default(),
                    },
                    build_platform: PlatformWithVirtualPackages {
                        platform: selector_config.build_platform,
                        virtual_packages: params
                            .build_platform_virtual_packages
                            .clone()
                            .unwrap_or_default(),
                    },
                    hash: discovered_output.hash.clone(),
                    variant,
                    directories: output_directory(
                        if number_of_outputs == 1 {
                            OneOrMultipleOutputs::Single(discovered_output.name.clone())
                        } else {
                            OneOrMultipleOutputs::OneOfMany(discovered_output.name.clone())
                        },
                        params.work_directory.clone(),
                        &named_source.path,
                    ),
                    channels: params
                        .channel_base_urls
                        .iter()
                        .flatten()
                        .cloned()
                        .map(Into::into)
                        .collect(),
                    channel_priority: tool_config.channel_priority,
                    timestamp,
                    subpackages: subpackages.clone(),
                    packaging_settings: PackagingSettings::from_args(
                        ArchiveType::Conda,
                        CompressionLevel::default(),
                    ),
                    store_recipe: false,
                    force_colors: false,
                    sandbox_config: None,
                    debug: Debug::default(),
                    solve_strategy: Default::default(),
                },
                finalized_dependencies: None,
                finalized_sources: None,
                finalized_cache_dependencies: None,
                finalized_cache_sources: None,
                system_tools: SystemTools::default(),
                build_summary: Arc::default(),
                extra_meta: None,
            };

            output.recipe.build.string = BuildString::Resolved(discovered_output.build_string);

            let temp_recipe = TemporaryRenderedRecipe::from_output(&output)?;
            let tool_config = tool_config.clone();
            let (output, package) = temp_recipe
                .within_context_async(move || async move { run_build(output, &tool_config).await })
                .await?;

            let input_globs = T::extract_input_globs_from_build(
                &self.config,
                &params.work_directory,
                params.editable,
            );

            // join it with the files that were read during the recipe generation
            let input_globs = input_globs
                .into_iter()
                .chain(
                    generated_recipe
                        .build_input_globs
                        .iter()
                        .map(|f| f.to_string()),
                )
                .collect::<Vec<_>>();

            let built_package = CondaBuiltPackage {
                output_file: package,
                // TODO: we should handle input globs properly
                input_globs,
                name: output.name().as_normalized().to_string(),
                version: output.version().to_string(),
                build: output.build_string().into_owned(),
                subdir: output.target_platform().to_string(),
            };
            packages.push(built_package);
        }

        Ok(CondaBuildResult { packages })
    }
}

/// Returns the capabilities for this backend
fn default_capabilities() -> BackendCapabilities {
    BackendCapabilities {
        provides_conda_metadata: Some(true),
        provides_conda_build: Some(true),
        highest_supported_project_model: Some(
            pixi_build_types::VersionedProjectModel::highest_version(),
        ),
    }
}
