// use pyo3::prelude::*;
// use pyo3::types::PyDict;
// use pixi_build_backend::generated_recipe::GeneratedRecipe;
// use serde_json;

// #[pyclass]
// // #[derive(Clone)]
// pub struct PyGeneratedRecipe {
//     pub(crate) inner: GeneratedRecipe,
// }

// #[pymethods]
// impl PyGeneratedRecipe {
//     #[getter]
//     pub fn metadata_input_globs(&self) -> Vec<String> {
//         self.inner.metadata_input_globs.clone()
//     }

//     #[getter]
//     pub fn build_input_globs(&self) -> Vec<String> {
//         self.inner.build_input_globs.clone()
//     }

//     pub fn to_dict(&self, py: Python) -> PyResult<PyObject> {
//         // Convert the IntermediateRecipe to a JSON value, then to Python dict
//         let recipe_json = serde_json::to_value(&self.inner.recipe)
//             .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("Failed to serialize recipe: {}", e)))?;

//         let dict = PyDict::new(py);

//         // Convert recipe to Python object
//         let recipe_py = pythonize::pythonize(py, &recipe_json)?;
//         dict.set_item("recipe", recipe_py)?;

//         dict.set_item("metadata_input_globs", &self.inner.metadata_input_globs)?;
//         dict.set_item("build_input_globs", &self.inner.build_input_globs)?;

//         Ok(dict.into())
//     }

//     pub fn __repr__(&self) -> String {
//         format!(
//             "PyGeneratedRecipe(metadata_globs={}, build_globs={})",
//             self.inner.metadata_input_globs.len(),
//             self.inner.build_input_globs.len()
//         )
//     }
// }

// impl From<GeneratedRecipe> for PyGeneratedRecipe {
//     fn from(recipe: GeneratedRecipe) -> Self {
//         PyGeneratedRecipe { inner: recipe }
//     }
// }

// impl From<PyGeneratedRecipe> for GeneratedRecipe {
//     fn from(py_recipe: PyGeneratedRecipe) -> Self {
//         py_recipe.inner
//     }
// }
