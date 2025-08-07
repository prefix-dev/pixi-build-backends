use miette::IntoDiagnostic;
use pixi_build_backend::generated_recipe::{
    DefaultMetadataProvider, GenerateRecipe, GeneratedRecipe,
};
use pyo3::{
    PyErr, PyObject, PyResult, Python,
    exceptions::PyValueError,
    pyclass, pymethods,
    types::{PyAnyMethods, PyString},
};

use crate::{
    recipe_stage0::recipe::PyIntermediateRecipe,
    types::{PyBackendConfig, PyPlatform, PyProjectModelV1, PyPythonParams},
};

#[pyclass]
#[derive(Clone, Default)]
pub struct PyGeneratedRecipe {
    pub(crate) inner: pixi_build_backend::generated_recipe::GeneratedRecipe,
}

#[pymethods]
impl PyGeneratedRecipe {
    #[new]
    pub fn new() -> Self {
        PyGeneratedRecipe {
            inner: pixi_build_backend::generated_recipe::GeneratedRecipe::default(),
        }
    }

    #[staticmethod]
    pub fn from_model(model: PyProjectModelV1) -> PyResult<Self> {
        let recipe = GeneratedRecipe::from_model(model.inner.clone(), &mut DefaultMetadataProvider)
            .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;
        Ok(PyGeneratedRecipe { inner: recipe })
    }

    #[getter]
    pub fn recipe(&self) -> PyIntermediateRecipe {
        self.inner.recipe.clone().into()
    }

    #[getter]
    pub fn metadata_input_globs(&self) -> Vec<String> {
        self.inner.metadata_input_globs.iter().cloned().collect()
    }

    #[getter]
    pub fn build_input_globs(&self) -> Vec<String> {
        self.inner.build_input_globs.iter().cloned().collect()
    }
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

/// Trait part
#[pyclass]
#[derive(Clone)]
pub struct PyGenerateRecipe {
    model: PyObject,
}

#[pymethods]
impl PyGenerateRecipe {
    #[new]
    pub fn new(model: PyObject) -> Self {
        PyGenerateRecipe { model }
    }
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
        let recipe: GeneratedRecipe = Python::with_gil(|py| {
            let manifest_str = manifest_path.to_string_lossy().to_string();

            // we dont pass the wrapper but the python inner model directly
            let py_object = config.model.clone();

            // For other types, we try to wrap them into the Python class
            // So user can use the Python API
            let project_model_class = py
                .import("pixi_build_backend.types.project_model")
                .into_diagnostic()?
                .getattr("ProjectModelV1")
                .into_diagnostic()?;

            let project_model = project_model_class
                .call_method1("_from_py", (PyProjectModelV1::from(model),))
                .into_diagnostic()?;

            let platform_model_class = py
                .import("pixi_build_backend.types.platform")
                .into_diagnostic()?
                .getattr("Platform")
                .into_diagnostic()?;

            let platform_model = platform_model_class
                .call_method1("_from_py", (PyPlatform::from(host_platform),))
                .into_diagnostic()?;

            let python_params_class = py
                .import("pixi_build_backend.types.python_params")
                .into_diagnostic()?
                .getattr("PythonParams")
                .into_diagnostic()?;
            let python_params_model = python_params_class
                .call_method1(
                    "_from_py",
                    (PyPythonParams::from(python_params.unwrap_or_default()),),
                )
                .into_diagnostic()?;

            let generated_recipe_py = self
                .model
                .bind(py)
                .call_method(
                    "generate_recipe",
                    (
                        project_model,
                        py_object,
                        PyString::new(py, manifest_str.as_str()),
                        platform_model,
                        python_params_model,
                    ),
                    None,
                )
                .into_diagnostic()?;

            // To expose a nice API for the user, we extract the PyGeneratedRecipe
            // calling private _into_py method
            let generated_recipe: PyGeneratedRecipe = generated_recipe_py
                .call_method0("_into_py")
                .into_diagnostic()?
                .extract::<PyGeneratedRecipe>()
                .into_diagnostic()?;

            Ok::<_, miette::Report>(generated_recipe.into())
        })?;

        Ok(recipe)
    }
}
