//! Trait for adding build tools to dependencies

use std::collections::BTreeMap;

use rattler_build::{
    recipe::{parser::Requirements, variable::Variable},
    NormalizedKey,
};
use rattler_conda_types::{ChannelConfig, Platform};

use crate::dependencies::extract_dependencies;

use super::{project::new_spec, Dependencies, Targets};

/// Trait for adding build tools to dependencies
pub trait RequirementsProvider<P: crate::ProjectModel> {
    /// Returns the list of build tool names required by this backend
    fn build_tool_names(
        &self,
        dependencies: &Dependencies<<P::Targets as Targets>::Spec>,
    ) -> Vec<String>;

    /// Adds build tools to the dependencies
    fn add_build_tools<'a>(
        &'a self,
        dependencies: &mut Dependencies<'a, <P::Targets as Targets>::Spec>,
        empty_spec: &'a <P::Targets as Targets>::Spec,
        build_tools: &'a [String],
    );

    /// Return requirements for the given project model
    fn requirements(
        &self,
        project_model: &P,
        host_platform: Platform,
        channel_config: &ChannelConfig,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<Requirements> {
        let mut requirements = Requirements::default();

        // Get dependencies from project model
        let mut dependencies = project_model
            .targets()
            .map(|t| t.dependencies(Some(host_platform)))
            .unwrap_or_default();

        // Add build tools
        let build_tools = self.build_tool_names(&dependencies);
        let empty_spec = new_spec::<P>();
        self.add_build_tools(&mut dependencies, &empty_spec, &build_tools);

        // Extract dependencies into requirements
        requirements.build = extract_dependencies(channel_config, dependencies.build, variant)?;
        requirements.host = extract_dependencies(channel_config, dependencies.host, variant)?;
        requirements.run = extract_dependencies(channel_config, dependencies.run, variant)?;

        // Allow backend-specific handling after basic processing
        self.post_process_requirements(&mut requirements, host_platform);

        Ok(requirements)
    }

    /// Optional hook for backends to modify requirements after processing
    fn post_process_requirements(&self, requirements: &mut Requirements, host_platform: Platform);
}
