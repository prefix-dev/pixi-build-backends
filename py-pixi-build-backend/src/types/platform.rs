use pyo3::prelude::*;
use rattler_conda_types::Platform;

#[pyclass]
#[derive(Clone)]
pub struct PyPlatform {
    pub(crate) inner: Platform,
}

#[pymethods]
impl PyPlatform {
    #[new]
    pub fn new(platform_str: &str) -> PyResult<Self> {
        let platform = platform_str.parse::<Platform>().map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid platform: {}", e))
        })?;
        Ok(PyPlatform { inner: platform })
    }

    #[staticmethod]
    pub fn linux64() -> Self {
        PyPlatform {
            inner: Platform::Linux64,
        }
    }

    #[staticmethod]
    pub fn linux_aarch64() -> Self {
        PyPlatform {
            inner: Platform::LinuxAarch64,
        }
    }

    #[staticmethod]
    pub fn osx64() -> Self {
        PyPlatform {
            inner: Platform::Osx64,
        }
    }

    #[staticmethod]
    pub fn osx_arm64() -> Self {
        PyPlatform {
            inner: Platform::OsxArm64,
        }
    }

    #[staticmethod]
    pub fn win64() -> Self {
        PyPlatform {
            inner: Platform::Win64,
        }
    }

    pub fn __str__(&self) -> String {
        self.inner.to_string()
    }

    pub fn __repr__(&self) -> String {
        format!("PyPlatform('{}')", self.inner)
    }

    #[getter]
    pub fn name(&self) -> String {
        self.inner.to_string()
    }

    #[staticmethod]
    pub fn current() -> PyResult<Self> {
        let platform = Platform::current();
        Ok(PyPlatform { inner: platform })
    }
}

impl From<Platform> for PyPlatform {
    fn from(platform: Platform) -> Self {
        PyPlatform { inner: platform }
    }
}

impl From<PyPlatform> for Platform {
    fn from(py_platform: PyPlatform) -> Self {
        py_platform.inner
    }
}
