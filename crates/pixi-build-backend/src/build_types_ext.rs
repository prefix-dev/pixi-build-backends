//! This module mimics some of the functions found in pixi that works with the data types
//! there but work with the project model types instead.
//!
//! This makes it easier when devoloping new backends that need to work with the project model.
use std::sync::Arc;

use itertools::Either;
use pixi_build_types as pbt;
use rattler_conda_types::{Channel, NamelessMatchSpec, Platform};

pub trait TargetSelectorExt {
    /// Does the target selector match the platform?
    fn matches(&self, platform: Platform) -> bool;
}

/// Extends the  type with additional functionality.
pub trait TargetExt<'a> {
    /// The selector, in pixi this is something like `[target.linux-64]
    type Selector: TargetSelectorExt + 'a;
    /// The target it is resolving to
    type Target: 'a;

    /// Returns the default target.
    fn default_target(&self) -> Option<&Self::Target>;

    /// Returns all targets
    fn targets(&'a self) -> impl Iterator<Item = (&'a Self::Selector, &'a Self::Target)>;

    /// Resolve the target for the given platform.
    fn resolve(&'a self, platform: Option<Platform>) -> impl Iterator<Item = &'a Self::Target> {
        if let Some(platform) = platform {
            let iter = self
                .default_target()
                .into_iter()
                .chain(self.targets().filter_map(move |(selector, target)| {
                    if selector.matches(platform) {
                        Some(target)
                    } else {
                        None
                    }
                }));
            Either::Right(iter)
        } else {
            Either::Left(self.default_target().into_iter())
        }
    }
}

/// Get the * version for the version type, that is currently being used
pub trait AnyVersion {
    fn any() -> Self;
}

pub trait BinarySpecExt {
    fn to_nameless(&self) -> NamelessMatchSpec;
}

impl BinarySpecExt for pbt::BinaryPackageSpecV1 {
    fn to_nameless(&self) -> NamelessMatchSpec {
        NamelessMatchSpec {
            version: self.version.clone(),
            build: self.build.clone(),
            build_number: self.build_number.clone(),
            file_name: self.file_name.clone(),
            channel: self
                .channel
                .as_ref()
                .map(|url| Arc::new(Channel::from_url(url.clone()))),
            subdir: self.subdir.clone(),
            md5: self.md5.as_ref().map(|m| m.0),
            sha256: self.sha256.as_ref().map(|s| s.0),
            namespace: None,
            url: None,
            extras: None,
        }
    }
}

// === Below here are the implementations for v1 ===
impl TargetSelectorExt for pbt::TargetSelectorV1 {
    fn matches(&self, platform: Platform) -> bool {
        match self {
            pbt::TargetSelectorV1::Platform(p) => p == &platform.to_string(),
            pbt::TargetSelectorV1::Linux => platform.is_linux(),
            pbt::TargetSelectorV1::Unix => platform.is_unix(),
            pbt::TargetSelectorV1::Win => platform.is_windows(),
            pbt::TargetSelectorV1::MacOs => platform.is_osx(),
        }
    }
}

impl<'a> TargetExt<'a> for pbt::TargetsV1 {
    type Selector = pbt::TargetSelectorV1;
    type Target = pbt::TargetV1;

    fn default_target(&self) -> Option<&pbt::TargetV1> {
        self.default_target.as_ref()
    }

    fn targets(&'a self) -> impl Iterator<Item = (&'a pbt::TargetSelectorV1, &'a pbt::TargetV1)> {
        self.targets.iter().flatten()
    }
}

impl AnyVersion for pbt::PackageSpecV1 {
    fn any() -> Self {
        pbt::PackageSpecV1::Binary(Box::new(rattler_conda_types::VersionSpec::Any.into()))
    }
}

// == end of v1 implementations ==
