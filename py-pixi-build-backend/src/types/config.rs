use std::path::{Path, PathBuf};

use pixi_build_backend::generated_recipe::BackendConfig;
use pyo3::{Py, PyAny, PyObject, Python, pyclass, pymethods, types::PyAnyMethods};
use pythonize::pythonize;
use serde::{Deserialize, de::DeserializeOwned};
// use serde::{de::DeserializeOwned, Deserialize, Deserializer};

#[derive(Deserialize, Clone)]
#[pyclass]
pub struct PyBackendConfig {
    debug_dir: Option<PathBuf>,
}

#[pymethods]
impl PyBackendConfig {
    #[new]
    fn new(debug_dir: Option<PathBuf>) -> Self {
        PyBackendConfig { debug_dir }
    }

    fn debug_dir(&self) -> Option<&Path> {
        BackendConfig::debug_dir(self)
    }
}

impl BackendConfig for PyBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }
}
