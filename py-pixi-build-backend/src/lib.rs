use ::pixi_build_backend::generated_recipe::BackendConfig;
use pyo3::prelude::*;

mod error;
// mod backends;
mod types;

use error::PyPixiBuildError;

#[pyfunction]
fn check_config(x: impl BackendConfig) -> usize {
    x.debug_dir().map_or(0, |dir| dir.to_string_lossy().len())
}

#[pymodule]
fn pixi_build_backend(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // // Add exception types
    // m.add("PyPixiBuildError", _py.get_type::<PyPixiBuildError>())?;

    // Add core types
    m.add_class::<types::PyPlatform>()?;
    m.add_class::<types::PyProjectModelV1>()?;
    // m.add_class::<types::PyGeneratedRecipe>()?;
    m.add_class::<types::PyPythonParams>()?;
    m.add_class::<types::PyBackendConfig>()?;

    // Add backend
    // m.add_class::<backends::PyPythonBackend>()?;

    Ok(())
}
