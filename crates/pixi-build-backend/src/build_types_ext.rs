//! This module mimics some of the functions found in pixi that works with the data types
//! there but work with the project model types instead.
//!
//! This makes it easier when devoloping new backends that need to work with the project model.
use std::{collections::HashSet, ops::Deref, path::Path, sync::Arc};

use indexmap::IndexMap;
use itertools::{Either, Itertools};
use miette::IntoDiagnostic;
use pixi_build_types::{self as pbt, PackageSpecV1, SourcePackageName};
use rattler_build::NormalizedKey;
use rattler_conda_types::{Channel, MatchSpec, NamelessMatchSpec, PackageName, Platform, Version};

use crate::dependencies::resolve_path;

pub trait TargetSelectorExt {
    /// Does the target selector match the platform?
    fn matches(&self, platform: Platform) -> bool;
}

/// Extends the type with additional functionality.
pub trait TargetsExt<'a> {
    /// The selector, in pixi this is something like `[target.linux-64]
    type Selector: TargetSelectorExt + 'a;
    /// The target it is resolving to
    type Target: 'a;

    /// The Spec type that is used in the package spec
    type Spec: PackageSpecExt + 'a;

    /// Returns the default target.
    fn default_target(&self) -> Option<&Self::Target>;

    /// Returns all targets
    fn targets(&'a self) -> impl Iterator<Item = (&'a Self::Selector, &'a Self::Target)>;

    fn run_dependencies(
        &'a self,
        platform: Option<Platform>,
    ) -> IndexMap<&'a SourcePackageName, &'a Self::Spec>;

    fn host_dependencies(
        &'a self,
        platform: Option<Platform>,
    ) -> IndexMap<&'a SourcePackageName, &'a Self::Spec>;

    fn build_dependencies(
        &'a self,
        platform: Option<Platform>,
    ) -> IndexMap<&'a SourcePackageName, &'a Self::Spec>;

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

impl<'a> TargetsExt<'a> for pbt::TargetsV1 {
    type Selector = pbt::TargetSelectorV1;
    type Target = pbt::TargetV1;

    type Spec = pbt::PackageSpecV1;

    fn default_target(&self) -> Option<&pbt::TargetV1> {
        self.default_target.as_ref()
    }

    fn targets(&'a self) -> impl Iterator<Item = (&'a pbt::TargetSelectorV1, &'a pbt::TargetV1)> {
        self.targets.iter().flatten()
    }

    /// Return the run dependencies for the given platform
    fn run_dependencies(
        &'a self,
        platform: Option<Platform>,
    ) -> IndexMap<&'a SourcePackageName, &'a PackageSpecV1> {
        let targets = self.resolve(platform).collect_vec();

        targets
            .iter()
            .flat_map(|t| t.run_dependencies.iter())
            .flatten()
            .collect::<IndexMap<&pbt::SourcePackageName, &pbt::PackageSpecV1>>()
    }

    /// Return the run dependencies for the given platform
    fn host_dependencies(
        &'a self,
        platform: Option<Platform>,
    ) -> IndexMap<&'a SourcePackageName, &'a PackageSpecV1> {
        let targets = self.resolve(platform).collect_vec();

        targets
            .iter()
            .flat_map(|t| t.host_dependencies.iter())
            .flatten()
            .collect::<IndexMap<&pbt::SourcePackageName, &pbt::PackageSpecV1>>()
    }

    /// Return the run dependencies for the given platform
    fn build_dependencies(
        &'a self,
        platform: Option<Platform>,
    ) -> IndexMap<&'a SourcePackageName, &'a PackageSpecV1> {
        let targets = self.resolve(platform).collect_vec();

        targets
            .iter()
            .flat_map(|t| t.build_dependencies.iter())
            .flatten()
            .collect::<IndexMap<&pbt::SourcePackageName, &pbt::PackageSpecV1>>()
    }
}

impl AnyVersion for pbt::PackageSpecV1 {
    fn any() -> Self {
        pbt::PackageSpecV1::Binary(Box::new(rattler_conda_types::VersionSpec::Any.into()))
    }
}

// == end of v1 implementations ==

pub trait PackageSpecExt {
    /// Returns true if the specified [`PackageSpec`] is a valid variant spec.
    fn can_be_used_as_variant(&self) -> bool;

    fn to_match_spec(
        &self,
        name: PackageName,
        root_dir: &Path,
        ignore_self: bool,
    ) -> miette::Result<MatchSpec>;
}

impl PackageSpecExt for pbt::PackageSpecV1 {
    fn can_be_used_as_variant(&self) -> bool {
        match self {
            pbt::PackageSpecV1::Binary(boxed_spec) => {
                let pbt::BinaryPackageSpecV1 {
                    version,
                    build,
                    build_number,
                    file_name,
                    channel,
                    subdir,
                    md5,
                    sha256,
                } = &**boxed_spec;

                version == &Some(rattler_conda_types::VersionSpec::Any)
                    && build.is_none()
                    && build_number.is_none()
                    && file_name.is_none()
                    && channel.is_none()
                    && subdir.is_none()
                    && md5.is_none()
                    && sha256.is_none()
            }
            _ => false,
        }
    }

    fn to_match_spec(
        &self,
        name: PackageName,
        root_dir: &Path,
        ignore_self: bool,
    ) -> miette::Result<MatchSpec> {
        match self {
            pbt::PackageSpecV1::Binary(binary_spec) => {
                if binary_spec.version == Some("*".parse().unwrap()) {
                    // Skip dependencies with wildcard versions.
                    name.as_normalized()
                        .to_string()
                        .parse::<MatchSpec>()
                        .into_diagnostic()
                } else {
                    Ok(MatchSpec::from_nameless(
                        binary_spec.to_nameless(),
                        Some(name),
                    ))
                }
            }
            pbt::PackageSpecV1::Source(source_spec) => match source_spec {
                pbt::SourcePackageSpecV1::Path(path) => {
                    let path = resolve_path(Path::new(&path.path), root_dir).ok_or_else(|| {
                        miette::miette!("failed to resolve home dir for: {}", path.path)
                    })?;

                    if ignore_self && path.as_path() == root_dir {
                        // Skip source dependencies that point to the root directory.
                        return Err(miette::miette!("Skipping self-referencing dependency"));
                    } else {
                        // All other source dependencies are not yet supported.
                        return Err(miette::miette!(
                            "recursive source dependencies are not yet supported"
                        ));
                    }
                }
                _ => {
                    return Err(miette::miette!(
                        "recursive source dependencies are not yet supported"
                    ));
                }
            },
        }
    }
}

/// Represent a ProjectModel that can be used to resolve targets.
pub trait ProjectModelExt<'a> {
    type Targets: TargetsExt<'a> + 'a;

    /// Return the targets of the project model
    fn targets(&'a self) -> Option<&'a Self::Targets>;

    fn used_variants(&'a self, host_platform: Option<Platform>) -> HashSet<NormalizedKey>;

    fn name(&'a self) -> &'a str;
    fn version(&'a self) -> &'a Option<Version>;
}

// impl Target for pbt::TargetsV1 {}

impl<'a> ProjectModelExt<'a> for pbt::ProjectModelV1 {
    type Targets = pbt::TargetsV1;

    fn targets(&self) -> Option<&Self::Targets> {
        self.targets.as_ref()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &Option<Version> {
        &self.version
    }

    fn used_variants(&'a self, host_platform: Option<Platform>) -> HashSet<NormalizedKey> {
        let build_dependencies = self
            .targets()
            .iter()
            .flat_map(|target| target.build_dependencies(host_platform))
            .collect_vec();

        let host_dependencies = self
            .targets()
            .iter()
            .flat_map(|target| target.host_dependencies(host_platform))
            .collect_vec();

        let run_dependencies = self
            .targets()
            .iter()
            .flat_map(|target| target.run_dependencies(host_platform))
            .collect_vec();

        let used_variants = build_dependencies
            .iter()
            .chain(host_dependencies.iter())
            .chain(run_dependencies.iter())
            .filter(|(_, spec)| spec.can_be_used_as_variant())
            .map(|(name, _)| name.deref().clone().into())
            .collect();

        used_variants
    }
}
