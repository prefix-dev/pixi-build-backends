use std::collections::HashMap;

use crate::{traits::Dependencies, ProjectModel, Targets};

pub fn sccache_tools() -> Vec<String> {
    vec!["sccache".to_string()]
}

pub fn enable_sccache(env: HashMap<String, String>) -> bool {
    env.keys().any(|k| k.to_lowercase().starts_with("SCCACHE"))
}

pub fn add_sccache<'a, P: ProjectModel>(
    dependencies: &mut Dependencies<'a, <P::Targets as Targets>::Spec>,
    sccache_tools: &'a [String],
    empty_spec: &'a <<P as ProjectModel>::Targets as Targets>::Spec,
) {
    for cache_tool in sccache_tools {
        if !dependencies.build.contains_key(&cache_tool) {
            dependencies.build.insert(cache_tool, empty_spec);
        }
    }
}
