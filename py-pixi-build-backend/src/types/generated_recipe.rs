use std::collections::BTreeSet;

use miette::IntoDiagnostic;
use pixi_build_backend::generated_recipe::{
    DefaultMetadataProvider, GenerateRecipe, GeneratedRecipe,
};
use pyo3::{
    Py, PyErr, PyObject, PyResult, Python,
    exceptions::PyValueError,
    pyclass, pymethods,
    types::{PyAnyMethods, PyString},
};
use recipe_stage0::recipe::IntermediateRecipe;

use crate::{
    create_py_wrap,
    recipe_stage0::recipe::PyIntermediateRecipe,
    types::{PyBackendConfig, PyPlatform, PyProjectModelV1, PyPythonParams},
};

create_py_wrap!(PyVecString, Vec<String>, |v: &Vec<String>,
                                           f: &mut std::fmt::Formatter<
    '_,
>| {
    write!(f, "[{}]", v.join(", "))
});

#[pyclass(get_all, set_all)]
#[derive(Clone)]
pub struct PyGeneratedRecipe {
    pub(crate) recipe: Py<PyIntermediateRecipe>,
    pub(crate) metadata_input_globs: Py<PyVecString>,
    pub(crate) build_input_globs: Py<PyVecString>,
}

#[pymethods]
impl PyGeneratedRecipe {
    #[new]
    pub fn new(py: Python) -> PyResult<Self> {
        Ok(PyGeneratedRecipe {
            recipe: Py::new(py, PyIntermediateRecipe::new(py)?)?,
            metadata_input_globs: Py::new(py, PyVecString::default())?,
            build_input_globs: Py::new(py, PyVecString::default())?,
        })
    }

    #[staticmethod]
    pub fn from_model(py: Python, model: PyProjectModelV1) -> PyResult<Self> {
        let generated_recipe =
            GeneratedRecipe::from_model(model.inner.clone(), &mut DefaultMetadataProvider)
                .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

        let py_recipe = Py::new(
            py,
            PyIntermediateRecipe::from_intermediate_recipe(generated_recipe.recipe, py),
        )?;
        let py_metadata_globs = Py::new(
            py,
            PyVecString::from(
                generated_recipe
                    .metadata_input_globs
                    .into_iter()
                    .collect::<Vec<String>>(),
            ),
        )?;
        let py_build_globs = Py::new(
            py,
            PyVecString::from(
                generated_recipe
                    .build_input_globs
                    .into_iter()
                    .collect::<Vec<String>>(),
            ),
        )?;

        Ok(PyGeneratedRecipe {
            recipe: py_recipe,
            metadata_input_globs: py_metadata_globs,
            build_input_globs: py_build_globs,
        })
    }
}

impl PyGeneratedRecipe {
    pub fn to_generated_recipe(&self, py: Python) -> GeneratedRecipe {
        let recipe: IntermediateRecipe = self.recipe.borrow(py).to_intermediate_recipe(py);
        let metadata_input_globs: BTreeSet<String> =
            (*self.metadata_input_globs.borrow(py).clone())
                .clone()
                .into_iter()
                .collect();
        let build_input_globs: BTreeSet<String> = (*self.build_input_globs.borrow(py).clone())
            .clone()
            .into_iter()
            .collect();

        GeneratedRecipe {
            recipe,
            metadata_input_globs,
            build_input_globs,
        }
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

            // we don't pass the wrapper but the python inner model directly
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

            Ok::<_, miette::Report>(generated_recipe.to_generated_recipe(py))
        })?;

        Ok(recipe)
    }
}
