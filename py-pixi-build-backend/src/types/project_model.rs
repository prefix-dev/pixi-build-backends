use std::str::FromStr;

use pixi_build_types::ProjectModelV1;
use pyo3::prelude::*;
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
    pub fn new(name: Option<String>, version: Option<String>) -> Self {
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

    #[getter]
    pub fn name(&self) -> Option<&String> {
        self.inner.name.as_ref()
    }

    #[getter]
    pub fn version(&self) -> Option<String> {
        self.inner.version.as_ref().map(|v| v.to_string())
    }

    #[getter]
    pub fn description(&self) -> Option<String> {
        self.inner.description.clone()
    }

    #[getter]
    pub fn authors(&self) -> Option<Vec<String>> {
        self.inner.authors.clone()
    }

    #[getter]
    pub fn license(&self) -> Option<String> {
        self.inner.license.clone()
    }

    #[getter]
    pub fn license_file(&self) -> Option<String> {
        self.inner
            .license_file
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    #[getter]
    pub fn readme(&self) -> Option<String> {
        self.inner
            .readme
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    #[getter]
    pub fn homepage(&self) -> Option<String> {
        self.inner.homepage.as_ref().map(|u| u.to_string())
    }

    #[getter]
    pub fn repository(&self) -> Option<String> {
        self.inner.repository.as_ref().map(|u| u.to_string())
    }

    #[getter]
    pub fn documentation(&self) -> Option<String> {
        self.inner.documentation.as_ref().map(|u| u.to_string())
    }

    pub fn _debug_str(&self) -> String {
        format!("{:?}", self.inner)
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
