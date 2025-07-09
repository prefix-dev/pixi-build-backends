use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use pixi_build_types::ProjectModelV1;
use rattler_build::{NormalizedKey, recipe::variable::Variable};
use rattler_conda_types::Platform;
use recipe_stage0::recipe::{ConditionalList, IntermediateRecipe, Item, Package, Source, Value};
use serde::de::DeserializeOwned;

use crate::specs_conversion::from_targets_v1_to_conditional_requirements;

#[derive(Debug, Clone, Default)]
pub struct PythonParams {
    // Returns whetever the build is editable or not.
    // Default to false
    pub editable: bool,
}

/// The trait is responsible of converting a certain [`ProjectModelV1`] (or others in the future)
/// into an [`IntermediateRecipe`].
/// By implementing this trait, you can create a new backend for `pixi-build`.
///
/// It also uses a [`BackendConfig`] to provide additional configuration
/// options.
///
///
/// An instance of this trait is used by the [`IntermediateBackend`]
/// in order to generate the recipe.
pub trait GenerateRecipe {
    type Config: BackendConfig;

    /// Generates an [`IntermediateRecipe`] from a [`ProjectModelV1`].
    fn generate_recipe(
        &self,
        model: &ProjectModelV1,
        config: &Self::Config,
        manifest_path: PathBuf,
        // The host_platform will be removed in the future.
        // Right now it is used to determine if certain dependencies are present
        // for the host platform.
        // Instead, we should rely on recipe selectors and offload all the
        // evaluation logic to the rattler-build.
        host_platform: Platform,
        // Note: It is used only by python backend right now and may
        // be removed when profiles will be implemented.
        python_params: Option<PythonParams>,
    ) -> miette::Result<GeneratedRecipe>;

    /// Returns a list of globs that should be used to find the input files
    /// for the build process.
    /// For example, this could be a list of source files or configuration files
    /// used by Cmake.
    fn extract_input_globs_from_build(
        _config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> Vec<String> {
        vec![]
    }

    /// Returns "default" variants for the given host platform. This allows
    /// backends to set some default variant configuration that can be
    /// completely overwritten by the user.
    ///
    /// This can be useful to change the default behavior of rattler-build with
    /// regard to compilers. But it also allows setting up default build
    /// matrices.
    fn default_variants(&self, _host_platform: Platform) -> BTreeMap<NormalizedKey, Vec<Variable>> {
        BTreeMap::new()
    }
}

/// At least debug dir should be provided by the backend config
pub trait BackendConfig: DeserializeOwned + Default {
    fn debug_dir(&self) -> Option<&Path>;
}

#[derive(Default)]
pub struct GeneratedRecipe {
    pub recipe: IntermediateRecipe,
    pub metadata_input_globs: Vec<String>,
    pub build_input_globs: Vec<String>,
}

impl GeneratedRecipe {
    /// Creates a new [`GeneratedRecipe`] from a [`ProjectModelV1`].
    /// A default implementation that doesn't take into account the
    /// build scripts or other fields.
    pub fn from_model(model: ProjectModelV1, manifest_root: PathBuf) -> Self {
        let package = Package {
            name: Value::Concrete(model.name),
            version: Value::Concrete(
                model.version
                  .expect("`version` is required at the moment. In the future we will read this from `Cargo.toml`.")
                  .to_string(),
            ),
        };

        let source = ConditionalList::from([Item::Value(Value::Concrete(Source::path(
            manifest_root.display().to_string(),
        )))]);

        let requirements =
            from_targets_v1_to_conditional_requirements(&model.targets.unwrap_or_default());

        let ir = IntermediateRecipe {
            package,
            source,
            requirements,
            ..Default::default()
        };

        GeneratedRecipe {
            recipe: ir,
            ..Default::default()
        }
    }
}
