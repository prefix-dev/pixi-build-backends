use itertools::Itertools;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use indexmap::IndexMap;
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
    NormalizedKey,
    build::run_build,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    recipe::{Jinja, parser::BuildString, variable::Variable},
    render::resolved_dependencies::DependencyInfo,
    selectors::SelectorConfig,
    tool_configuration::Configuration,
    variant_config::VariantConfig,
};
use rattler_conda_types::{ChannelConfig, MatchSpec, Platform};
use recipe_stage0::matchspec::{PackageDependency, SerializableMatchSpec};
use serde::Deserialize;
use tempfile::tempdir;

use crate::{
    generated_recipe::{BackendConfig, GenerateRecipe},
    protocol::{Protocol, ProtocolInstantiator},
    rattler_build_integration,
    specs_conversion::from_source_matchspec_into_package_spec,
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
    pub(crate) manifest_root: PathBuf,
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
            manifest_root,
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
            root_dir: self.manifest_root.to_path_buf(),
        };

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
        // Create a variant config from the variant configuration in the parameters.
        let variants: BTreeMap<NormalizedKey, Vec<Variable>> = params
            .variant_configuration
            .map(|v| {
                v.into_iter()
                    .map(|(k, v)| {
                        (
                            k.into(),
                            v.into_iter().map(|v| Variable::from_string(&v)).collect(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let host_platform = params
            .host_platform
            .as_ref()
            .map(|p| p.platform)
            .unwrap_or(Platform::current());

        let variant_config = VariantConfig {
            variants,
            pin_run_as_build: None,
            zip_keys: None,
        };

        let generated_recipe = self.generate_recipe.generate_recipe(
            &self.project_model,
            &self.config,
            self.manifest_root.clone(),
            host_platform,
        )?;

        // Determine the variant keys that are used in the recipe.
        let resolved_dependencies = generated_recipe
            .recipe
            .requirements
            .resolve(Some(&host_platform));

        let used_variants = resolved_dependencies.used_variants();

        // Determine the combinations of the used variants.
        let combinations = variant_config
            .combinations(&used_variants, None)
            .into_diagnostic()?;

        let mut packages = Vec::new();

        let target_platform = params
            .build_platform
            .as_ref()
            .map(|bp| bp.platform)
            .unwrap_or_else(Platform::current);

        let host_platform = params
            .host_platform
            .as_ref()
            .map(|hp| hp.platform)
            .unwrap_or_else(Platform::current);

        for input_variant in combinations {
            let selector_config = SelectorConfig {
                // We ignore noarch here
                target_platform,
                host_platform,
                hash: None,
                build_platform: target_platform,
                variant: input_variant,
                experimental: false,
                // allow undefined while finding the variants
                allow_undefined: true,
            };

            let host_virtual_packages = params
                .host_platform
                .as_ref()
                .and_then(|p| p.virtual_packages.clone());

            let build_virtual_packages = params
                .build_platform
                .as_ref()
                .and_then(|p| p.virtual_packages.clone());

            let tmp_dir = tempdir().into_diagnostic()?;

            let tmp_dir_path = tmp_dir.path().to_path_buf();

            let outputs = rattler_build_integration::get_build_output(
                &generated_recipe,
                tool_config.clone(),
                selector_config,
                host_virtual_packages,
                build_virtual_packages,
                params.channel_base_urls.clone(),
                tmp_dir_path.clone(),
                tmp_dir_path.clone(),
            )
            .await?;

            for output in outputs {
                let selector_config = output.build_configuration.selector_config();

                let jinja =
                    Jinja::new(selector_config.clone()).with_context(&output.recipe.context);

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
                                miette::miette!(
                                    "source dependency {} does not have a name",
                                    dep.spec
                                )
                            })?
                            .as_normalized()
                            .to_string();
                        Ok((name, from_source_matchspec_into_package_spec(dep.clone())?))
                    })
                    .collect::<miette::Result<HashMap<_, _>>>()?;

                packages.push(CondaPackageMetadata {
                    name: output.name().clone(),
                    version: output.version().clone(),
                    build: build_string.to_string(),
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
        }

        Ok(CondaMetadataResult {
            packages,
            input_globs: None,
        })
    }

    async fn conda_build(&self, params: CondaBuildParams) -> miette::Result<CondaBuildResult> {
        let channel_config = ChannelConfig {
            channel_alias: params.channel_configuration.base_url,
            root_dir: self.manifest_root.to_path_buf(),
        };

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

        let build_platform = Platform::current();

        let target_platform = params
            .host_platform
            .as_ref()
            .map(|hp| hp.platform)
            .unwrap_or_else(Platform::current);

        // Recompute all the variant combinations
        let variants = params
            .variant_configuration
            .map(|v| {
                v.into_iter()
                    .map(|(k, v)| {
                        (
                            k.into(),
                            v.into_iter().map(|v| Variable::from_string(&v)).collect(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let host_platform = params
            .host_platform
            .as_ref()
            .map(|p| p.platform)
            .unwrap_or(Platform::current());

        let variant_config = VariantConfig {
            variants,
            pin_run_as_build: None,
            zip_keys: None,
        };

        let recipe = self.generate_recipe.generate_recipe(
            &self.project_model,
            &self.config,
            self.manifest_root.clone(),
            host_platform,
        )?;

        // Determine the variant keys that are used in the recipe.
        let resolved_dependencies = recipe.recipe.requirements.resolve(Some(&host_platform));

        let used_variants = resolved_dependencies.used_variants();

        // Determine the combinations of the used variants.
        let combinations = variant_config
            .combinations(&used_variants, None)
            .into_diagnostic()?;

        let mut packages = Vec::new();

        for input_variant in combinations {
            let selector_config = SelectorConfig {
                // We ignore noarch here
                target_platform,
                host_platform: target_platform,
                hash: None,
                build_platform,
                variant: input_variant,
                experimental: false,
                // allow undefined while finding the variants
                allow_undefined: true,
            };

            let host_virtual_packages = params
                .host_platform
                .as_ref()
                .and_then(|p| p.virtual_packages.clone());

            let build_virtual_packages = params.build_platform_virtual_packages.clone();

            let outputs = rattler_build_integration::get_build_output(
                &recipe,
                tool_config.clone(),
                selector_config,
                host_virtual_packages,
                build_virtual_packages,
                params.channel_base_urls.clone(),
                params.work_directory.clone(),
                params.work_directory.clone(),
            )
            .await?;

            let mut modified_outputs = Vec::with_capacity(outputs.len());
            for mut output in outputs {
                let selector_config = output.build_configuration.selector_config();
                let jinja =
                    Jinja::new(selector_config.clone()).with_context(&output.recipe.context);
                let hash = HashInfo::from_variant(output.variant(), output.recipe.build().noarch());
                let build_string = output
                    .recipe
                    .build()
                    .string()
                    .resolve(&hash, output.recipe.build().number(), &jinja)
                    .into_owned();
                output.recipe.build.string = BuildString::Resolved(build_string);
                modified_outputs.push(output);
            }

            // Determine the outputs to build
            let selected_outputs = if let Some(output_identifiers) = params.outputs.clone() {
                output_identifiers
                    .into_iter()
                    .filter_map(|iden| {
                        let pos = modified_outputs.iter().position(|output| {
                            let CondaOutputIdentifier {
                                name,
                                version,
                                build,
                                subdir,
                            } = &iden;
                            name.as_ref()
                                .is_none_or(|n| output.name().as_normalized() == n)
                                && version
                                    .as_ref()
                                    .is_none_or(|v| output.version().to_string() == *v)
                                && build
                                    .as_ref()
                                    .is_none_or(|b| output.build_string() == b.as_str())
                                && subdir
                                    .as_ref()
                                    .is_none_or(|s| output.target_platform().as_str() == s)
                        })?;
                        Some(modified_outputs.remove(pos))
                    })
                    .collect()
            } else {
                modified_outputs
            };

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
                    .within_context_async(
                        move || async move { run_build(output, &tool_config).await },
                    )
                    .await?;

                let input_globs = T::build_input_globs(&self.config, &params.work_directory);

                let built_package = CondaBuiltPackage {
                    output_file: package,
                    // TODO: we should handle input globs properly
                    input_globs,
                    name: output.name().as_normalized().to_string(),
                    version: output.version().to_string(),
                    build: build_string.to_string(),
                    subdir: output.target_platform().to_string(),
                };
                packages.push(built_package);
            }
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
