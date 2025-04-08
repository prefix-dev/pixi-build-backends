use std::collections::{BTreeMap, HashMap};

use rattler_build::{
    recipe::{parser::Requirements, variable::Variable},
    NormalizedKey,
};
use serde::Serialize;
use crate::{
    dependencies::extract_dependencies, traits::Dependencies, PackageSpec, ProjectModel, Targets,
};

pub struct PackageRequirements<P: ProjectModel> {
    /// Requirements for rattler-build
    pub requirements: Requirements,

    /// The source requirements
    pub source: SourceRequirements<P>,
}

#[derive(Debug, Serialize)]
#[serde(bound(serialize = "<<P::Targets as Targets>::Spec as PackageSpec>::SourceSpec: Serialize"))]
pub struct SourceRequirements<P: ProjectModel> {
    /// Source package specification for build dependencies
    pub build: HashMap<String, <<P::Targets as Targets>::Spec as PackageSpec>::SourceSpec>,

    /// Source package specification for host dependencies
    pub host: HashMap<String, <<P::Targets as Targets>::Spec as PackageSpec>::SourceSpec>,

    /// Source package specification for runtime dependencies
    pub run: HashMap<String, <<P::Targets as Targets>::Spec as PackageSpec>::SourceSpec>,
}

/// Return requirements for the given project model
pub fn requirements<P: ProjectModel>(
    dependencies: Dependencies<<P::Targets as Targets>::Spec>,
    variant: &BTreeMap<NormalizedKey, Variable>,
) -> miette::Result<PackageRequirements<P>> {
    let (build, build_source_dependencies) =
        extract_dependencies(dependencies.build, variant)?;
    let (host, host_source_dependencies) =
        extract_dependencies(dependencies.host, variant)?;
    let (run, run_source_dependencies) =
        extract_dependencies(dependencies.run, variant)?;

    Ok(PackageRequirements {
        requirements: Requirements {
            build,
            host,
            run,
            ..Default::default()
        },
        source: SourceRequirements {
            build: build_source_dependencies,
            host: host_source_dependencies,
            run: run_source_dependencies,
        },
    })
}
