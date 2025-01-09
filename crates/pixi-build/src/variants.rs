use pixi_spec::{DetailedSpec, PixiSpec};
use rattler_conda_types::VersionSpec;

/// Returns true if the specified [`PixiSpec`] is a valid variant spec.
///
/// At the moment, a spec that allows any version is considered a variant spec.
pub fn can_be_used_as_variant(spec: &PixiSpec) -> bool {
    match spec {
        PixiSpec::Version(version)
        | PixiSpec::DetailedVersion(DetailedSpec {
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
