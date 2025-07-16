use pixi_build_backend::generated_recipe::{GenerateRecipe, GeneratedRecipe};
use pyo3::{
    FromPyObject, PyObject, Python, pyclass,
    types::{PyAnyMethods, PyString},
};

use crate::types::{PyBackendConfig, PyPlatform, PyProjectModelV1, PyPythonParams};

#[pyclass]
#[derive(FromPyObject)]
pub struct PyGeneratedRecipe {
    pub(crate) inner: pixi_build_backend::generated_recipe::GeneratedRecipe,
}

impl From<GeneratedRecipe> for PyGeneratedRecipe {
    fn from(recipe: GeneratedRecipe) -> Self {
        PyGeneratedRecipe { inner: recipe }
    }
}

impl From<PyGeneratedRecipe> for GeneratedRecipe {
    fn from(py_recipe: PyGeneratedRecipe) -> Self {
        py_recipe.inner
    }
}

#[pyclass]
pub struct PyGenerateRecipe {
    model: PyObject,
}

impl GenerateRecipe for PyGenerateRecipe {
    type Config = PyBackendConfig;

    fn generate_recipe(
        &self,
        model: &pixi_build_types::ProjectModelV1,
        config: &Self::Config,
        manifest_path: std::path::PathBuf,
        host_platform: rattler_conda_types::Platform,
        python_params: Option<pixi_build_backend::generated_recipe::PythonParams>,
    ) -> miette::Result<pixi_build_backend::generated_recipe::GeneratedRecipe> {
        let manifest_str = manifest_path.to_string_lossy().to_string();

        let recipe = Python::with_gil(|py| {
            self.model
                .bind(py)
                .call_method(
                    "generate_recipe",
                    (
                        PyProjectModelV1::from(model),
                        config.clone(),
                        PyString::new(py, manifest_str.as_str()),
                        PyPlatform::from(host_platform),
                        PyPythonParams::from(python_params.unwrap_or_default()),
                    ),
                    None,
                )
                .unwrap()
        })
        .extract::<PyGeneratedRecipe>()
        .unwrap();
        Ok(recipe.into())
    }
}
