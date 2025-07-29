use std::path::{Path, PathBuf};

use pixi_build_backend::generated_recipe::BackendConfig;
use pyo3::{PyObject, Python, pyclass, pymethods};
use pythonize::pythonize;
use serde::Deserialize;
use serde::Deserializer;

#[pyclass]
#[derive(Clone, Debug)]
pub struct PyBackendConfig {
    pub(crate) model: PyObject,
    pub(crate) debug_dir: Option<PathBuf>,
}

impl<'de> Deserialize<'de> for PyBackendConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct TempData(serde_json::Value);

        let mut data = TempData::deserialize(deserializer)?.0;

        Python::with_gil(|py| {
            let model = pythonize(py, &data).map_err(serde::de::Error::custom)?;

            let debug_dir: Option<PathBuf> = data
                .as_object_mut()
                .and_then(|obj| obj.get("debug_dir"))
                .and_then(|v| v.as_str().map(PathBuf::from));

            Ok(PyBackendConfig {
                model: model.unbind(),
                debug_dir,
            })
        })
    }
}

#[pymethods]
impl PyBackendConfig {
    #[new]
    fn new(debug_dir: Option<PathBuf>, model: PyObject) -> Self {
        PyBackendConfig { debug_dir, model }
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
