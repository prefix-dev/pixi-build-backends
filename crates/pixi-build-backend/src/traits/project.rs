use std::collections::HashSet;

use itertools::Itertools;
use pixi_build_types::{self as pbt};
use rattler_build::NormalizedKey;
use rattler_conda_types::{Platform, Version};

use super::{targets::Targets, PackageSpec};

/// A trait that defines the project model interface
pub trait ProjectModel {
    type Targets: Targets;

    /// Return the targets of the project model
    fn targets(&self) -> Option<&Self::Targets>;

    /// Return a spec that matches any version
    fn new_spec(&self) -> <<Self as ProjectModel>::Targets as Targets>::Spec;

    /// Return the used variants of the project model
    fn used_variants(&self, platform: Option<Platform>) -> HashSet<NormalizedKey>;

    /// Return the name of the project model
    fn name(&self) -> &str;

    /// Return the version of the project model
    fn version(&self) -> &Option<Version>;
}

impl ProjectModel for pbt::ProjectModelV1 {
    type Targets = pbt::TargetsV1;

    fn targets(&self) -> Option<&Self::Targets> {
        self.targets.as_ref()
    }

    fn new_spec(&self) -> <<Self as ProjectModel>::Targets as Targets>::Spec {
        pbt::TargetsV1::empty_spec()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &Option<Version> {
        &self.version
    }

    fn used_variants(&self, platform: Option<Platform>) -> HashSet<NormalizedKey> {
        let build_dependencies = self
            .targets()
            .iter()
            .flat_map(|target| target.build_dependencies(platform))
            .collect_vec();

        let host_dependencies = self
            .targets()
            .iter()
            .flat_map(|target| target.host_dependencies(platform))
            .collect_vec();

        let run_dependencies = self
            .targets()
            .iter()
            .flat_map(|target| target.run_dependencies(platform))
            .collect_vec();

        let used_variants = build_dependencies
            .iter()
            .chain(host_dependencies.iter())
            .chain(run_dependencies.iter())
            .filter(|(_, spec)| spec.can_be_used_as_variant())
            .map(|(name, _)| name.as_str().into())
            .collect();

        used_variants
    }
}
