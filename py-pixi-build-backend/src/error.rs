use thiserror::Error;

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum PixiBuildBackendError {
    #[error(transparent)]
    GenerateRecipe(#[from] miette::Error),
}
