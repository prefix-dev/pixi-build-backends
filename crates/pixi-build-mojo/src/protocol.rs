use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use miette::{Context, IntoDiagnostic};
use pixi_build_backend::{
    PackageSourceSpec, ProjectModel,
    common::{build_configuration, compute_variants},
    protocol::{Protocol, ProtocolInstantiator},
    utils::TemporaryRenderedRecipe,
};
use pixi_build_types::{
    BackendCapabilities, CondaPackageMetadata, PlatformAndVirtualPackages,
    procedures::{
        conda_build::{
            CondaBuildParams, CondaBuildResult, CondaBuiltPackage, CondaOutputIdentifier,
        },
        conda_metadata::{CondaMetadataParams, CondaMetadataResult},
        initialize::{InitializeParams, InitializeResult},
        negotiate_capabilities::{NegotiateCapabilitiesParams, NegotiateCapabilitiesResult},
    },
};
// use pixi_build_types as pbt;
use rattler_build::{
    build::run_build,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{Directories, Output},
    recipe::{Jinja, parser::BuildString, variable::Variable},
    render::resolved_dependencies::DependencyInfo,
    tool_configuration::Configuration,
    variant_config::VariantConfig,
};
use rattler_conda_types::{ChannelConfig, MatchSpec, PackageName, Platform};

use crate::{
    config::MojoBackendConfig,
    rust::{MojoBuildBackend, construct_configuration},
};

#[async_trait::async_trait]
impl<P: ProjectModel + Sync> Protocol for MojoBuildBackend<P> {
    fn debug_dir(&self) -> Option<&Path> {
        self.config.debug_dir.as_deref()
    }

    async fn conda_get_metadata(
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

        let package_name = PackageName::from_str(self.project_model.name())
            .into_diagnostic()
            .context("`{name}` is not a valid package name")?;

        let directories = Directories::setup(
            package_name.as_normalized(),
            &self.manifest_path,
            &params.work_directory,
            true,
            &chrono::Utc::now(),
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
        let variant_config = VariantConfig {
            variants,
            pin_run_as_build: None,
            zip_keys: None,
        };

        // Determine the variant keys that are used in the recipe.
        let used_variants = self.project_model.used_variants(Some(host_platform));

        // Determine the combinations of the used variants.
        let combinations = variant_config
            .combinations(&used_variants, None)
            .into_diagnostic()?;

        // Construct the different outputs
        let mut packages = Vec::new();
        for variant in combinations {
            // TODO: Determine how and if we can determine this from the manifest.
            let (recipe, source_requirements) = self.recipe(host_platform, &variant)?;
            let build_configuration_params = build_configuration(
                channels.clone(),
                params.build_platform.clone(),
                params.host_platform.clone(),
                variant.clone(),
                directories.clone(),
            )?;

            let build_configuration = construct_configuration(&recipe, build_configuration_params);

            let output = Output {
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
                sources: source_requirements
                    .run
                    .into_iter()
                    .map(|(k, spec)| (k, PackageSourceSpec::to_v1(spec)))
                    .collect(),
            });
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
        let channels = params.channel_base_urls.unwrap_or_default();
        let host_platform = params
            .host_platform
            .as_ref()
            .map(|p| p.platform)
            .unwrap_or_else(Platform::current);

        let package_name = PackageName::from_str(self.project_model.name())
            .into_diagnostic()
            .context("`{name}` is not a valid package name")?;

        let directories = Directories::setup(
            package_name.as_normalized(),
            &self.manifest_path,
            &params.work_directory,
            true,
            &chrono::Utc::now(),
        )
        .into_diagnostic()
        .context("failed to setup build directories")?;

        // Recompute all the variant combinations
        let input_variant_configuration = params.variant_configuration.map(|v| {
            v.into_iter()
                .map(|(k, v)| {
                    (
                        k.into(),
                        v.into_iter().map(|v| Variable::from_string(&v)).collect(),
                    )
                })
                .collect()
        });
        let variant_combinations = compute_variants(
            &self.project_model,
            input_variant_configuration,
            host_platform,
        )?;

        // Compute outputs for each variant
        let mut outputs = Vec::with_capacity(variant_combinations.len());
        for variant in variant_combinations {
            let (recipe, _source_requirements) = self.recipe(host_platform, &variant)?;
            let build_configuration_params = build_configuration(
                channels.clone(),
                Some(PlatformAndVirtualPackages {
                    platform: Platform::current(),
                    virtual_packages: params.build_platform_virtual_packages.clone(),
                }),
                Some(PlatformAndVirtualPackages {
                    platform: host_platform,
                    virtual_packages: params
                        .host_platform
                        .clone()
                        .and_then(|p| p.virtual_packages),
                }),
                variant.clone(),
                directories.clone(),
            )?;

            let build_configuration = construct_configuration(&recipe, build_configuration_params);

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

/// Determines the build input globs for given python package
/// even this will be probably backend specific, e.g setuptools
/// has a different way of determining the input globs than hatch etc.
///
/// However, lets take everything in the directory as input for now
fn input_globs() -> Vec<String> {
    [
        "**/*.mojo",
        // pixi configuration files
        // TODO: do I need these?
        "pixi.toml",
        "pixi.lock",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub struct MojoBackendInstantiator {
    logging_output_handler: LoggingOutputHandler,
}

impl MojoBackendInstantiator {
    pub fn new(logging_output_handler: LoggingOutputHandler) -> Self {
        Self {
            logging_output_handler,
        }
    }
}

#[async_trait::async_trait]
impl ProtocolInstantiator for MojoBackendInstantiator {
    fn debug_dir(configuration: Option<serde_json::Value>) -> Option<PathBuf> {
        configuration
            .and_then(|config| serde_json::from_value::<MojoBackendConfig>(config.clone()).ok())
            .and_then(|config| config.debug_dir)
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
            serde_json::from_value(config)
                .into_diagnostic()
                .context("failed to parse configuration")?
        } else {
            MojoBackendConfig::default()
        };

        let instance = MojoBuildBackend::new(
            params.manifest_path,
            project_model,
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

fn default_capabilities() -> BackendCapabilities {
    BackendCapabilities {
        provides_conda_metadata: Some(true),
        provides_conda_build: Some(true),
        highest_supported_project_model: Some(
            pixi_build_types::VersionedProjectModel::highest_version(),
        ),
    }
}
