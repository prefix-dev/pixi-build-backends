use std::str::FromStr;

use pixi_build_types::ProjectModelV1;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rattler_conda_types::Version;

#[pyclass]
#[derive(Clone)]
pub struct PyProjectModelV1 {
    pub(crate) inner: ProjectModelV1,
}

#[pymethods]
impl PyProjectModelV1 {
    #[new]
    #[pyo3(signature = (name, version=None))]
    pub fn new(name: String, version: Option<String>) -> Self {
        PyProjectModelV1 {
            inner: ProjectModelV1 {
                name,
                version: version.map(|v| {
                    v.parse()
                        .unwrap_or_else(|_| Version::from_str(&v).expect("Invalid version"))
                }),
                targets: None,
                description: None,
                authors: None,
                license: None,
                license_file: None,
                readme: None,
                homepage: None,
                repository: None,
                documentation: None,
            },
        }
    }

    // #[staticmethod]
    // pub fn from_dict(data: &PyDict) -> PyResult<Self> {
    //     let name = data
    //         .get_item("name")?
    //         .and_then(|item| item.extract::<String>().ok())
    //         .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Missing 'name' field"))?;

    //     let version = data
    //         .get_item("version")?
    //         .and_then(|item| item.extract::<String>().ok())
    //         .map(|v| v.parse().unwrap_or_else(|_| {
    //             Version::from_str(&v).expect("Invalid version")
    //         }));

    //     // TODO: Parse targets from dict
    //     let targets = None;

    //     Ok(PyProjectModel {
    //         inner: ProjectModelV1 {
    //             name,
    //             version,
    //             targets,
    //         },
    //     })
    // }

    #[getter]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    pub fn version(&self) -> Option<String> {
        self.inner.version.as_ref().map(|v| v.to_string())
    }

    // pub fn to_dict(&self, py: Python) -> PyResult<PyObject> {
    //     let dict = PyDict::new(py);
    //     dict.set_item("name", &self.inner.name)?;
    //     if let Some(version) = &self.inner.version {
    //         dict.set_item("version", version.to_string())?;
    //     }
    //     // TODO: Add targets to dict
    //     Ok(dict.into())
    // }

    pub fn __repr__(&self) -> String {
        match &self.inner.version {
            Some(version) => format!(
                "PyProjectModel(name='{}', version='{}')",
                self.inner.name, version
            ),
            None => format!("PyProjectModel(name='{}')", self.inner.name),
        }
    }
}

impl From<ProjectModelV1> for PyProjectModelV1 {
    fn from(model: ProjectModelV1) -> Self {
        PyProjectModelV1 { inner: model }
    }
}

impl From<&ProjectModelV1> for PyProjectModelV1 {
    fn from(model: &ProjectModelV1) -> Self {
        PyProjectModelV1 {
            inner: model.clone(),
        }
    }
}

impl From<PyProjectModelV1> for ProjectModelV1 {
    fn from(py_model: PyProjectModelV1) -> Self {
        py_model.inner
    }
}
