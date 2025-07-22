use pyo3::{PyResult, pyclass, pymethods};
use recipe_stage0::recipe::IntermediateRecipe;

use crate::recipe_stage0::recipe::{PyBuild, PyConditionalRequirements};

#[pyclass]
pub struct PyIntermediateRecipe {
    pub(crate) inner: IntermediateRecipe,
}

#[pymethods]
impl PyIntermediateRecipe {
    #[getter]
    pub fn requirements(&self) -> PyConditionalRequirements {
        self.inner.requirements.clone().into()
    }

    #[getter]
    pub fn get_build(&self) -> PyBuild {
        self.inner.build.clone().into()
    }

    #[setter]
    pub fn set_build(&mut self, py_build: PyBuild) -> PyResult<()> {
        self.inner.build = py_build.inner;
        Ok(())
    }
}

impl From<IntermediateRecipe> for PyIntermediateRecipe {
    fn from(recipe: IntermediateRecipe) -> Self {
        PyIntermediateRecipe { inner: recipe }
    }
}
