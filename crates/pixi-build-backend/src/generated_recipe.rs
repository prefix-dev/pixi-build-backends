use miette::Diagnostic;
use pixi_build_types::ProjectModelV1;
use rattler_build::{NormalizedKey, recipe::variable::Variable};
use rattler_conda_types::{Platform, Version};
use recipe_stage0::recipe::{About, IntermediateRecipe, Package, Value};
use serde::de::DeserializeOwned;
use std::convert::Infallible;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    path::{Path, PathBuf},
};
use thiserror::Error;

use crate::specs_conversion::from_targets_v1_to_conditional_requirements;

#[derive(Debug, Clone, Default)]
pub struct PythonParams {
    // Returns whetever the build is editable or not.
    // Default to false
    pub editable: bool,
}

/// The trait is responsible of converting a certain [`ProjectModelV1`] (or
/// others in the future) into an [`IntermediateRecipe`].
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
    ) -> BTreeSet<String> {
        BTreeSet::new()
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

pub trait BackendConfig: DeserializeOwned + Clone {
    /// At least debug dir should be provided by the backend config
    fn debug_dir(&self) -> Option<&Path>;

    /// Merge this configuration with a target-specific configuration.
    /// Target-specific values typically override base values.
    fn merge_with_target_config(&self, target_config: &Self) -> miette::Result<Self>;
}

#[derive(Debug, Error, Diagnostic)]
pub enum GenerateRecipeError<MetadataProviderError: Diagnostic + 'static> {
    #[error("There was no name defined for the recipe")]
    NoNameDefined,
    #[error("There was no version defined for the recipe")]
    NoVersionDefined,
    #[error("An error occurred while querying the {0}")]
    MetadataProviderError(String, #[source] MetadataProviderError),
}

#[derive(Default, Clone)]
pub struct GeneratedRecipe {
    pub recipe: IntermediateRecipe,
    pub metadata_input_globs: BTreeSet<String>,
    pub build_input_globs: BTreeSet<String>,
}

impl GeneratedRecipe {
    /// Creates a new [`GeneratedRecipe`] from a [`ProjectModelV1`].
    /// A default implementation that doesn't take into account the
    /// build scripts or other fields.
    pub fn from_model<M: MetadataProvider>(
        model: ProjectModelV1,
        provider: &mut M,
    ) -> Result<Self, GenerateRecipeError<M::Error>> {
        // If the name is not defined in the model, we try to get it from the provider.
        // If the provider cannot provide a name, we return an error.
        let name = if model.name.is_empty() {
            provider
                .name()
                .map_err(|e| GenerateRecipeError::MetadataProviderError(String::from("name"), e))?
                .ok_or(GenerateRecipeError::NoNameDefined)?
        } else {
            model.name
        };

        // If the version is not defined in the model, we try to get it from the
        // provider. If the provider cannot provide a version, we return an
        // error.
        let version = match model.version {
            Some(v) => v,
            None => provider
                .version()
                .map_err(|e| {
                    GenerateRecipeError::MetadataProviderError(String::from("version"), e)
                })?
                .ok_or(GenerateRecipeError::NoVersionDefined)?,
        };

        let package = Package {
            name: Value::Concrete(name),
            version: Value::Concrete(version.to_string()),
        };

        let requirements =
            from_targets_v1_to_conditional_requirements(&model.targets.unwrap_or_default());

        macro_rules! derive_value {
            ($ident:ident) => {
                match model.$ident {
                    Some(v) => Some(v.to_string()),
                    None => provider.$ident().map_err(|e| {
                        GenerateRecipeError::MetadataProviderError(
                            String::from(stringify!($ident)),
                            e,
                        )
                    })?,
                }
            };
        }

        let about = About {
            homepage: derive_value!(homepage).map(Value::Concrete),
            license: derive_value!(license).map(Value::Concrete),
            description: derive_value!(description).map(Value::Concrete),
            documentation: derive_value!(documentation).map(Value::Concrete),
            repository: derive_value!(repository).map(Value::Concrete),
            license_file: match model.license_file {
                Some(v) => Some(Value::Concrete(v.display().to_string())),
                None => provider
                    .license_file()
                    .map_err(|e| {
                        GenerateRecipeError::MetadataProviderError(String::from("license-file"), e)
                    })?
                    .map(Value::Concrete),
            },
            summary: provider
                .summary()
                .map_err(|e| {
                    GenerateRecipeError::MetadataProviderError(String::from("summary"), e)
                })?
                .map(Value::Concrete),
        };

        let ir = IntermediateRecipe {
            package,
            requirements,
            about: Some(about),
            ..Default::default()
        };

        Ok(GeneratedRecipe {
            recipe: ir,
            ..Default::default()
        })
    }
}

#[derive(Debug, Error, Diagnostic)]
pub enum MetadataProviderError {
    #[error("The metadata provider cannot provide an about section for the recipe")]
    CannotParseVersion(#[from] rattler_conda_types::ParseVersionError),
}

pub trait MetadataProvider {
    type Error: Diagnostic;

    /// Returns the name of the package or `None` if the provider does not
    /// provide a name.
    fn name(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }

    /// Returns the version of the package or `None` if the provider does not
    /// provide a version.
    fn version(&mut self) -> Result<Option<Version>, Self::Error> {
        Ok(None)
    }

    fn homepage(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }
    fn license(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }
    fn license_file(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }
    fn summary(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }
    fn description(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }
    fn documentation(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }
    fn repository(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }
}

pub struct DefaultMetadataProvider;

impl MetadataProvider for DefaultMetadataProvider {
    type Error = Infallible;
}
