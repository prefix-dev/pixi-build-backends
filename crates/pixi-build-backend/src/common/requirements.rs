use std::collections::BTreeMap;

use rattler_build::{
    recipe::{parser::Requirements, variable::Variable},
    NormalizedKey,
};
use rattler_conda_types::ChannelConfig;

use crate::{dependencies::extract_dependencies, traits::Dependencies, ProjectModel, Targets};

/// Return requirements for the given project model
pub fn requirements<P: ProjectModel>(
    dependencies: Dependencies<<P::Targets as Targets>::Spec>,
    channel_config: &ChannelConfig,
    variant: &BTreeMap<NormalizedKey, Variable>,
) -> miette::Result<Requirements> {
    // Extract dependencies into requirements
    let requirements = Requirements {
        build: extract_dependencies(channel_config, dependencies.build, variant)?,
        host: extract_dependencies(channel_config, dependencies.host, variant)?,
        run: extract_dependencies(channel_config, dependencies.run, variant)?,
        ..Default::default()
    };
    Ok(requirements)
}
