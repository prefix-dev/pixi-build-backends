mod build_script;
mod config;

use build_script::BuildScriptContext;
use config::PixiBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    generated_recipe::{DefaultMetadataProvider, GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
};
use rattler_build::{NormalizedKey, recipe::variable::Variable};
use rattler_conda_types::{PackageName, Platform};
use recipe_stage0::recipe::{ConditionalRequirements, Script};
use std::collections::HashSet;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

#[derive(Default, Clone)]
pub struct PixiGenerator {}

impl GenerateRecipe for PixiGenerator {
    type Config = PixiBackendConfig;

    fn generate_recipe(
        &self,
        model: &pixi_build_types::ProjectModelV1,
        config: &Self::Config,
        manifest_root: std::path::PathBuf,
        host_platform: rattler_conda_types::Platform,
        _python_params: Option<PythonParams>,
        _variants: &HashSet<NormalizedKey>,
    ) -> miette::Result<GeneratedRecipe> {
        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), &mut DefaultMetadataProvider)
                .into_diagnostic()?;

        let requirements = &mut generated_recipe.recipe.requirements;

        let resolved_requirements = ConditionalRequirements::resolve(
            requirements.build.as_ref(),
            requirements.host.as_ref(),
            requirements.run.as_ref(),
            requirements.run_constraints.as_ref(),
            Some(host_platform),
        );

        // Add pixi as a build dependency
        let pixi_name = PackageName::new_unchecked("pixi");
        if !resolved_requirements.build.contains_key(&pixi_name) {
            requirements.build.push("pixi".parse().into_diagnostic()?);
        }

        let build_script = BuildScriptContext {
            build_task: config.build_task.clone(),
            manifest_root,
        }
        .render();

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config.env.clone(),
            ..Default::default()
        };

        Ok(generated_recipe)
    }

    fn extract_input_globs_from_build(
        &self,
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> miette::Result<BTreeSet<String>> {
        Ok(["pixi.toml", "pixi.lock"]
            .iter()
            .map(|s: &&str| s.to_string())
            .chain(config.extra_input_globs.clone())
            .collect())
    }

    fn default_variants(
        &self,
        _host_platform: Platform,
    ) -> miette::Result<BTreeMap<NormalizedKey, Vec<Variable>>> {
        let variants = BTreeMap::new();

        // No default variants needed for pixi builds
        Ok(variants)
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(|log| {
        IntermediateBackendInstantiator::<PixiGenerator>::new(log, Arc::default())
    })
    .await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
