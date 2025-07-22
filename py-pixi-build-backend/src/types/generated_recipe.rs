use std::path::PathBuf;

use pixi_build_backend::generated_recipe::{GenerateRecipe, GeneratedRecipe};
use pyo3::{
    PyObject, Python, pyclass, pymethods,
    types::{PyAnyMethods, PyString},
};

use crate::types::{
    PyBackendConfig, PyPlatform, PyProjectModelV1, PyPythonParams, recipe::PyIntermediateRecipe,
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
    pub fn from_model(model: PyProjectModelV1, manifest_root: PathBuf) -> Self {
        let recipe = GeneratedRecipe::from_model(model.inner.clone(), manifest_root);
        PyGeneratedRecipe { inner: recipe }
    }

    #[getter]
    pub fn recipe(&self) -> PyIntermediateRecipe {
        self.inner.recipe.clone().into()
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

    fn generate_recipe(
        &self,
        model: &PyProjectModelV1,
        config: &PyBackendConfig,
        manifest_path: std::path::PathBuf,
        host_platform: PyPlatform,
        python_params: Option<PyPythonParams>,
    ) -> PyGeneratedRecipe {
        let result = GenerateRecipe::generate_recipe(
            self,
            &model.inner,
            config,
            manifest_path,
            host_platform.inner,
            python_params.map(|p| p.inner),
        )
        .unwrap();

        PyGeneratedRecipe::from(result)
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
                .unwrap()
                .getattr("ProjectModelV1")
                .unwrap();
            let project_model = project_model_class
                .call_method1("_from_py", (PyProjectModelV1::from(model),))
                .unwrap();

            let platform_model_class = py
                .import("pixi_build_backend.types.platform")
                .unwrap()
                .getattr("Platform")
                .unwrap();
            let platform_model = platform_model_class
                .call_method1("_from_py", (PyPlatform::from(host_platform),))
                .unwrap();

            let python_params_class = py
                .import("pixi_build_backend.types.python_params")
                .unwrap()
                .getattr("PythonParams")
                .unwrap();
            let python_params_model = python_params_class
                .call_method1(
                    "_from_py",
                    (PyPythonParams::from(python_params.unwrap_or_default()),),
                )
                .unwrap();

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
                .unwrap();

            // To expose a nice API for the user, we extract the PyGeneratedRecipe
            // calling private _into_py method
            let generated_recipe: PyGeneratedRecipe = generated_recipe_py
                .call_method0("_into_py")
                .unwrap()
                .extract::<PyGeneratedRecipe>()
                .unwrap();

            generated_recipe.into()
        });

        Ok(recipe)
    }
}
