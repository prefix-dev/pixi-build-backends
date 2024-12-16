use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use chrono::Utc;
use indexmap::IndexSet;
use itertools::Itertools;
use miette::IntoDiagnostic;
use pixi_build_types::procedures::conda_metadata::CondaMetadataParams;
use rattler_build::{
    hash::HashInfo,
    metadata::{
        BuildConfiguration, Directories, Output, PackageIdentifier, PackagingSettings,
        PlatformWithVirtualPackages,
    },
    recipe::{parser::find_outputs_from_src, ParsingError, Recipe},
    selectors::SelectorConfig,
    system_tools::SystemTools,
    variant_config::{DiscoveredOutput, ParseErrors, VariantConfig},
};
use rattler_conda_types::{package::ArchiveType, GenericVirtualPackage, Platform};
use rattler_package_streaming::write::CompressionLevel;
use rattler_virtual_packages::VirtualPackageOverrides;
use url::Url;

/// A `recipe.yaml` file might be accompanied by a `variants.toml` file from
/// which we can read variant configuration for that specific recipe..
pub const VARIANTS_CONFIG_FILE: &str = "variants.yaml";

/// A struct that contains all the configuration needed
/// for `rattler-build` in order to build a recipe.
/// The principal concepts is that all rattler-build concepts
/// should be hidden behind this struct and all pixi-build-backends
/// should only interact with this struct.
pub struct RattlerBuild {
    /// The raw recipe string.
    pub raw_recipe: String,
    /// The path to the recipe file.
    pub recipe_path: PathBuf,
    /// The selector configuration.
    pub selector_config: SelectorConfig,
    /// The directory where the build should happen.
    pub work_directory: PathBuf,
}

impl RattlerBuild {
    /// Create a new `RattlerBuild` instance.
    pub fn new(
        raw_recipe: String,
        recipe_path: PathBuf,
        selector_config: SelectorConfig,
        work_directory: PathBuf,
    ) -> Self {
        Self {
            raw_recipe,
            recipe_path,
            selector_config,
            work_directory,
        }
    }

    /// Create a `SelectorConfig` from the given `CondaMetadataParams`.
    pub fn selector_config_from(params: &CondaMetadataParams) -> SelectorConfig {
        SelectorConfig {
            target_platform: params
                .build_platform
                .as_ref()
                .map(|p| p.platform)
                .unwrap_or(Platform::current()),
            host_platform: params
                .host_platform
                .as_ref()
                .map(|p| p.platform)
                .unwrap_or(Platform::current()),
            build_platform: params
                .build_platform
                .as_ref()
                .map(|p| p.platform)
                .unwrap_or(Platform::current()),
            hash: None,
            variant: Default::default(),
            experimental: true,
            allow_undefined: false,
        }
    }

    /// Discover the outputs from the recipe.
    pub fn discover_outputs(
        &self,
        variant_config_input: &Option<HashMap<String, Vec<String>>>,
    ) -> miette::Result<IndexSet<DiscoveredOutput>> {
        // First find all outputs from the recipe
        let outputs = find_outputs_from_src(&self.raw_recipe)?;

        // Check if there is a `variants.yaml` file next to the recipe that we should
        // potentially use.
        let mut variant_configs = None;
        if let Some(variant_path) = self
            .recipe_path
            .parent()
            .map(|parent| parent.join(VARIANTS_CONFIG_FILE))
        {
            if variant_path.is_file() {
                variant_configs = Some(vec![variant_path]);
            }
        };

        let variant_configs = variant_configs.unwrap_or_default();

        let mut variant_config =
            VariantConfig::from_files(&variant_configs, &self.selector_config).into_diagnostic()?;

        if let Some(variant_config_input) = variant_config_input {
            for (k, v) in variant_config_input.iter() {
                variant_config.variants.insert(k.to_owned(), v.clone());
            }
        }

        variant_config
            .find_variants(&outputs, &self.raw_recipe, &self.selector_config)
            .into_diagnostic()
    }

    /// Get the outputs from the recipe.
    pub fn get_outputs(
        &self,
        discovered_outputs: &IndexSet<DiscoveredOutput>,
        channels: Vec<Url>,
        build_vpkgs: Vec<GenericVirtualPackage>,
        host_vpkgs: Vec<GenericVirtualPackage>,
        host_platform: Platform,
        build_platform: Platform,
    ) -> miette::Result<Vec<Output>> {
        let mut outputs = Vec::new();

        let mut subpackages = BTreeMap::new();

        let channels = channels.into_iter().map(Into::into).collect_vec();
        for discovered_output in discovered_outputs {
            let hash = HashInfo::from_variant(
                &discovered_output.used_vars,
                &discovered_output.noarch_type,
            );

            let selector_config = SelectorConfig {
                variant: discovered_output.used_vars.clone(),
                hash: Some(hash.clone()),
                target_platform: self.selector_config.target_platform,
                host_platform: self.selector_config.host_platform,
                build_platform: self.selector_config.build_platform,
                experimental: true,
                allow_undefined: false,
            };

            let recipe = Recipe::from_node(&discovered_output.node, selector_config.clone())
                .map_err(|err| {
                    let errs: ParseErrors = err
                        .into_iter()
                        .map(|err| ParsingError::from_partial(&self.raw_recipe, err))
                        .collect::<Vec<ParsingError>>()
                        .into();
                    errs
                })?;

            if recipe.build().skip() {
                eprintln!(
                    "Skipping build for variant: {:#?}",
                    discovered_output.used_vars
                );
                continue;
            }

            subpackages.insert(
                recipe.package().name().clone(),
                PackageIdentifier {
                    name: recipe.package().name().clone(),
                    version: recipe.package().version().version().clone(),
                    build_string: recipe
                        .build()
                        .string()
                        .resolve(&hash, recipe.build().number())
                        .into_owned(),
                },
            );

            let name = recipe.package().name().clone();

            outputs.push(Output {
                recipe,
                build_configuration: BuildConfiguration {
                    target_platform: discovered_output.target_platform,
                    host_platform: PlatformWithVirtualPackages {
                        platform: host_platform,
                        virtual_packages: host_vpkgs.clone(),
                    },
                    build_platform: PlatformWithVirtualPackages {
                        platform: build_platform,
                        virtual_packages: build_vpkgs.clone(),
                    },
                    hash,
                    variant: discovered_output.used_vars.clone(),
                    directories: Directories::setup(
                        name.as_normalized(),
                        &self.recipe_path,
                        &self.work_directory,
                        true,
                        &Utc::now(),
                    )
                    .into_diagnostic()?,
                    channels: channels.clone(),
                    channel_priority: Default::default(),
                    solve_strategy: Default::default(),
                    timestamp: chrono::Utc::now(),
                    subpackages: subpackages.clone(),
                    packaging_settings: PackagingSettings::from_args(
                        ArchiveType::Conda,
                        CompressionLevel::default(),
                    ),
                    store_recipe: false,
                    force_colors: true,
                },
                finalized_dependencies: None,
                finalized_cache_dependencies: None,
                finalized_cache_sources: None,
                finalized_sources: None,
                system_tools: SystemTools::new(),
                build_summary: Default::default(),
                extra_meta: None,
            });
        }

        Ok(outputs)
    }

    /// Detect the virtual packages.
    pub fn detect_virtual_packages(
        vpkgs: Option<Vec<GenericVirtualPackage>>,
    ) -> miette::Result<Vec<GenericVirtualPackage>> {
        let vpkgs = match vpkgs {
            Some(vpkgs) => vpkgs,
            None => {
                PlatformWithVirtualPackages::detect(&VirtualPackageOverrides::from_env())
                    .into_diagnostic()?
                    .virtual_packages
            }
        };
        Ok(vpkgs)
    }
}
