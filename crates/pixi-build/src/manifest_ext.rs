use std::{path::PathBuf, str::FromStr, sync::OnceLock};

use pixi_manifest::Manifests;
use rattler_conda_types::{ChannelConfig, ParseChannelError, Platform, Version};
use reqwest::Url;

pub trait ManifestExt {
    fn manifest(&self) -> &Manifests;

    /// Returns the path to the root directory that contains the manifest.
    fn manifest_root(&self) -> PathBuf {
        self.manifest().workspace.provenance.absolute_path()
    }

    /// Returns the resolved channels that are specified in the manifest
    /// `project` section.
    ///
    /// This function might return an error if the channel URL is invalid.
    fn resolved_project_channels(
        &self,
        channel_config: &ChannelConfig,
    ) -> Result<Vec<Url>, ParseChannelError> {
        self.manifest()
            .workspace
            .value
            .workspace
            .channels
            .iter()
            .map(|c| {
                c.channel
                    .clone()
                    .into_base_url(channel_config)
                    .map(|cl| cl.url().as_ref().clone())
            })
            .collect()
    }

    /// Returns `true` if the manifest is configured to use the specified
    /// platform.
    fn supports_target_platform(&self, platform: Platform) -> bool {
        self.manifest()
            .workspace
            .value
            .workspace
            .platforms
            .contains(&platform)
    }

    /// Returns the version as specified in the manifest.
    ///
    /// Note that this may be `None` because having a version is not required.
    /// Use [`Self::version_or_default`] to get a default version in that case.
    fn version(&self) -> Option<&Version> {
        self.manifest().workspace.value.workspace.version.as_ref()
    }

    /// Returns the version of the project or a default version if no version is
    /// specified in the manifest.
    fn version_or_default(&self) -> &Version {
        static DEFAULT_VERSION: OnceLock<Version> = OnceLock::new();
        self.version()
            .unwrap_or_else(|| DEFAULT_VERSION.get_or_init(|| Version::from_str("0.1.0").unwrap()))
    }
}

impl ManifestExt for Manifests {
    fn manifest(&self) -> &Manifests {
        self
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pixi_manifest::{ManifestKind, ManifestProvenance, Manifests, WithProvenance};

    #[test]
    fn test_manifest_root() {
        let raw_manifest = r#"
            [workspace]
            name = "basic"
            channels = ["conda-forge"]
            platforms = ["osx-arm64"]
            preview = ["pixi-build"]

            [package]
            authors = ["Tim de Jager <tim@prefix.dev>"]
            description = "Add a short description here"
            name = "basic"
            version = "0.1.0"

            [package.build]
            backend = { name = "pixi-build-python", version = "*" }
            "#;

        let manifest_path = Path::new("pixi.toml");
        let provenance = WithProvenance::new(
            raw_manifest,
            ManifestProvenance::new(manifest_path.to_path_buf(), ManifestKind::Pixi),
        );

        Manifests::from_workspace_source(provenance);
    }
}
