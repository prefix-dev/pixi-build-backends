use std::path::Path;

use miette::IntoDiagnostic;
use pixi_build_type_conversions::to_project_model_v1;
use pixi_build_types::VersionedProjectModel;
use rattler_conda_types::ChannelConfig;

/// Convert manifest to project model
pub fn to_project_model(
    manifest_path: &Path,
    channel_config: &ChannelConfig,
    highest_supported_project_model: Option<u32>,
) -> miette::Result<Option<VersionedProjectModel>> {
    // Load the manifest
    let manifest =
        pixi_manifest::Manifests::from_workspace_manifest_path(manifest_path.to_path_buf())?;
    let package = manifest.value.package.as_ref();

    // Determine which project model version to use
    let version_to_use = match highest_supported_project_model {
        // If a specific version is requested, use it (or fail if it's higher than what we support)
        Some(requested_version) => {
            let our_highest = VersionedProjectModel::highest_version();
            if requested_version > our_highest {
                miette::bail!(
                    "Requested project model version {} is higher than our highest supported version {}",
                    requested_version,
                    our_highest
                );
            }
            // Use the requested version
            requested_version
        }
        // If no specific version is requested, use our highest supported version
        None => VersionedProjectModel::highest_version(),
    };

    // This can be null in the rattler-build backend
    let versioned = package
        .map(|manifest| {
            let result = match version_to_use {
                1 => to_project_model_v1(&manifest.value, channel_config).into_diagnostic()?,
                _ => {
                    miette::bail!(
                        "Unsupported project model version: {}",
                        VersionedProjectModel::highest_version()
                    );
                }
            };
            Ok(VersionedProjectModel::from(result))
        })
        .transpose()?;

    Ok(versioned)
}
