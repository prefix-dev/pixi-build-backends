use std::{path::PathBuf, str::FromStr};

use pixi_build_types::ProjectModelV1;
use rattler_conda_types::Version;
use recipe_stage0::recipe::{
    Build, ConditionalList, IntermediateRecipe, Item, Package, Source, Value,
};

use crate::specs_conversion::from_targets_v1_to_conditional_requirements;

pub trait GenerateRecipe {
    /// Generates an IntermediateRecipe from a ProjectModelV1.
    fn generate_recipe(&self, model: &ProjectModelV1) -> miette::Result<GeneratedRecipe>;
}

pub struct GeneratedRecipe {
    pub recipe: IntermediateRecipe,
}

impl GeneratedRecipe {
    /// Creates a new GeneratedRecipe from a ProjectModelV1.
    /// A default implementation that don't take into account the
    /// build scripts or other fields.
    pub fn from_model(model: ProjectModelV1, manifest_root: PathBuf) -> Self {
        let package = Package {
            name: Value::Concrete(model.name),
            version: Value::Concrete(
                model
                    .version
                    .unwrap_or_else(|| {
                        Version::from_str("0.1.0").expect("Default version should be valid")
                    })
                    .to_string(),
            ),
        };

        let source = ConditionalList::from(vec![Item::Value(Value::Concrete(Source::path(
            manifest_root.display().to_string(),
        )))]);

        let requirements =
            from_targets_v1_to_conditional_requirements(&model.targets.unwrap_or_default());

        let ir = IntermediateRecipe {
            context: Default::default(),
            package,
            source,
            build: Build::default(),
            requirements,
            tests: Default::default(),
            about: None,
            extra: None,
        };

        GeneratedRecipe { recipe: ir }
    }
}
