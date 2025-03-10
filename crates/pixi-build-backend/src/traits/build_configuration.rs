//! Trait for building build configuration

use std::collections::BTreeMap;

use miette::IntoDiagnostic;
use pixi_build_types::PlatformAndVirtualPackages;
use rattler_build::{
    metadata::{BuildConfiguration, Directories, PlatformWithVirtualPackages},
    recipe::{variable::Variable, Recipe},
    NormalizedKey,
};
use rattler_conda_types::ChannelUrl;
use rattler_virtual_packages::VirtualPackageOverrides;
use url::Url;

/// The trait to provide build configuration for a recipe
pub trait BuildConfigurationProvider<P: crate::ProjectModel> {
    /// Returns the build configuration for a recipe
    fn build_configuration(
        &self,
        recipe: &Recipe,
        channels: Vec<Url>,
        build_platform: Option<PlatformAndVirtualPackages>,
        host_platform: Option<PlatformAndVirtualPackages>,
        variant: BTreeMap<NormalizedKey, Variable>,
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

        let configuration = self.construct_configuration(
            recipe,
            channels,
            build_platform,
            host_platform,
            variant,
            directories,
        );

        Ok(configuration)
    }

    /// Constructs the build configuration for a recipe
    fn construct_configuration(
        &self,
        recipe: &Recipe,
        channels: Vec<ChannelUrl>,
        build_platform: PlatformWithVirtualPackages,
        host_platform: PlatformWithVirtualPackages,
        variant: BTreeMap<NormalizedKey, Variable>,
        directories: Directories,
    ) -> BuildConfiguration;
}
