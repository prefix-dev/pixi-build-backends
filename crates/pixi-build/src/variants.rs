use pixi_build_types as pbt;
use rattler_conda_types::VersionSpec;

/// Returns true if the specified [`pbt::PackageSpecV1`] is a valid variant spec.
///
/// At the moment, a spec that allows any version is considered a variant spec.
pub fn can_be_used_as_variant(spec: &pbt::PackageSpecV1) -> bool {
    match spec {
        pbt::PackageSpecV1::Binary(pbt::BinaryPackageSpecV1 {
            version: Some(version),
            build: None,
            build_number: None,
            file_name: None,
            channel: None,
            subdir: None,
            md5: None,
            sha256: None,
        }) => version == &VersionSpec::Any,
        _ => false,
    }
}
