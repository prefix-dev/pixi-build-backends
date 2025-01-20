use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use fs_err as fs;
use miette::IntoDiagnostic;
use pixi_build_types::{BackendCapabilities, FrontendCapabilities};
use rattler_build::console_utils::LoggingOutputHandler;

pub struct RattlerBuildBackend {
    pub(crate) logging_output_handler: LoggingOutputHandler,
    /// In case of rattler-build, manifest is the raw recipe
    /// We need to apply later the selectors to get the final recipe
    pub(crate) raw_recipe: String,
    pub(crate) recipe_path: PathBuf,
    pub(crate) cache_dir: Option<PathBuf>,
}

impl RattlerBuildBackend {
    /// Returns a new instance of [`RattlerBuildBackend`] by reading the
    /// manifest at the given path.
    pub fn new(
        manifest_path: &Path,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
    ) -> miette::Result<Self> {
        // Locate the recipe
        let manifest_file_name = manifest_path.file_name().and_then(OsStr::to_str);
        let recipe_path = match manifest_file_name {
            Some("recipe.yaml") | Some("recipe.yml") => manifest_path.to_path_buf(),
            _ => {
                // The manifest is not a recipe, so we need to find the recipe.yaml file.
                let recipe_path = manifest_path.parent().and_then(|manifest_dir| {
                    [
                        "recipe.yaml",
                        "recipe.yml",
                        "recipe/recipe.yaml",
                        "recipe/recipe.yml",
                    ]
                    .into_iter()
                    .find_map(|relative_path| {
                        let recipe_path = manifest_dir.join(relative_path);
                        recipe_path.is_file().then_some(recipe_path)
                    })
                });

                recipe_path.ok_or_else(|| miette::miette!("Could not find a recipe.yaml in the source directory to use as the recipe manifest."))?
            }
        };

        // Load the manifest from the source directory
        let raw_recipe = fs::read_to_string(&recipe_path).into_diagnostic()?;

        Ok(Self {
            raw_recipe,
            recipe_path,
            logging_output_handler,
            cache_dir,
        })
    }

    /// Returns the capabilities of this backend based on the capabilities of
    /// the frontend.
    pub fn capabilities(_frontend_capabilities: &FrontendCapabilities) -> BackendCapabilities {
        BackendCapabilities {
            provides_conda_metadata: Some(true),
            provides_conda_build: Some(true),
            highest_supported_project_model: None,
        }
    }
}
