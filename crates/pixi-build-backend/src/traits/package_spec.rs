//! Package specification traits
//!
//! # Key components
//!
//! * [`PackageSpec`] - Core trait for package specification behavior
//! * [`AnyVersion`] - Trait for creating wildcard version specifications that
//!   can match any version
//! * [`BinarySpecExt`] - Extension for converting binary specs to nameless
//!   match specs

use std::fmt::Debug;
use std::sync::Arc;

use miette::IntoDiagnostic;
use pixi_build_types::{self as pbt};
use rattler_conda_types::{Channel, MatchSpec, NamelessMatchSpec, PackageName};

/// Get the * version for the version type, that is currently being used
pub trait AnyVersion {
    /// Get the * version for the version type, that is currently being used
    fn any() -> Self;
}

/// Convert a binary spec to a nameless match spec
pub trait BinarySpecExt {
    /// Return a NamelessMatchSpec from the binary spec
    fn to_nameless(&self) -> NamelessMatchSpec;
}

/// A trait that define the package spec interface
pub trait PackageSpec: Send {
    /// Source representation of a package
    type SourceSpec: PackageSourceSpec;

    /// Returns true if the specified [`PackageSpec`] is a valid variant spec.
    fn can_be_used_as_variant(&self) -> bool;

    /// Converts the package spec to a match spec.
    fn to_match_spec(
        &self,
        name: PackageName,
    ) -> miette::Result<(MatchSpec, Option<Self::SourceSpec>)>;
}

/// A trait that defines the package source spec interface
pub trait PackageSourceSpec: Debug + Send {
    /// Convert this instance into a v1 instance.
    fn to_v1(self) -> pbt::SourcePackageSpecV1;
}

impl PackageSpec for pbt::PackageSpecV1 {
    type SourceSpec = pbt::SourcePackageSpecV1;

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
    ) -> miette::Result<(MatchSpec, Option<Self::SourceSpec>)> {
        match self {
            pbt::PackageSpecV1::Binary(binary_spec) => {
                let match_spec = if binary_spec.version == Some("*".parse().unwrap()) {
                    // Skip dependencies with wildcard versions.
                    name.as_normalized()
                        .to_string()
                        .parse::<MatchSpec>()
                        .into_diagnostic()?
                } else {
                    MatchSpec::from_nameless(binary_spec.to_nameless(), Some(name))
                };
                Ok((match_spec, None))
            }
            pbt::PackageSpecV1::Source(source_spec) => Ok((
                MatchSpec {
                    name: Some(name),
                    ..MatchSpec::default()
                },
                Some(source_spec.clone()),
            )),
        }
    }
}

impl AnyVersion for pbt::PackageSpecV1 {
    fn any() -> Self {
        pbt::PackageSpecV1::Binary(Box::new(rattler_conda_types::VersionSpec::Any.into()))
    }
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
            license: None,
        }
    }
}

impl PackageSourceSpec for pbt::SourcePackageSpecV1 {
    fn to_v1(self) -> pbt::SourcePackageSpecV1 {
        self
    }
}
