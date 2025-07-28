use crate::{
    error::PyPixiBuildBackendError,
    recipe_stage0::{
        conditional::{PyItemPackageDependency, PyItemString},
        requirements::PyPackageSpecDependencies,
    },
    types::PyPlatform,
};
use pyo3::{
    Bound, Py, PyAny, PyResult, Python as BindingPython,
    exceptions::PyValueError,
    pyclass, pymethods,
    types::{PyList, PyListMethods},
};
use rattler_conda_types::package::EntryPoint;
use recipe_stage0::recipe::{
    About, Build, ConditionalRequirements, Extra, IntermediateRecipe, NoArchKind, Package,
    PathSource, Python, Script, Source, UrlSource, Value,
};
use std::collections::HashMap;

// Main recipe structure
#[pyclass]
#[derive(Clone, Default)]
pub struct PyIntermediateRecipe {
    pub(crate) inner: IntermediateRecipe,
}

#[pymethods]
impl PyIntermediateRecipe {
    #[new]
    pub fn new() -> Self {
        PyIntermediateRecipe {
            inner: IntermediateRecipe::default(),
        }
    }

    #[getter]
    pub fn package(&self) -> PyPackage {
        PyPackage {
            inner: self.inner.package.clone(),
        }
    }

    #[getter]
    pub fn build(&self) -> PyBuild {
        PyBuild {
            inner: self.inner.build.clone(),
        }
    }

    #[getter]
    pub fn requirements(&self) -> PyConditionalRequirements {
        PyConditionalRequirements {
            inner: self.inner.requirements.clone(),
        }
    }

    #[getter]
    pub fn about(&self) -> Option<PyAbout> {
        self.inner.about.clone().map(|a| PyAbout { inner: a })
    }

    #[getter]
    pub fn extra(&self) -> Option<PyExtra> {
        self.inner.extra.clone().map(|e| PyExtra { inner: e })
    }

    /// Converts the recipe to YAML string
    pub fn to_yaml(&self) -> PyResult<String> {
        Ok(self
            .inner
            .to_yaml()
            .map_err(PyPixiBuildBackendError::YamlSerialization)?)
    }

    /// Creates a recipe from YAML string
    #[staticmethod]
    pub fn from_yaml(yaml: String) -> PyResult<Self> {
        let intermediate_recipe: IntermediateRecipe =
            serde_yaml::from_str(&yaml).map_err(PyPixiBuildBackendError::YamlSerialization)?;

        Ok(Self {
            inner: intermediate_recipe,
        })
    }
}

impl From<IntermediateRecipe> for PyIntermediateRecipe {
    fn from(recipe: IntermediateRecipe) -> Self {
        PyIntermediateRecipe { inner: recipe }
    }
}

impl From<PyIntermediateRecipe> for IntermediateRecipe {
    fn from(py_recipe: PyIntermediateRecipe) -> Self {
        py_recipe.inner
    }
}

#[pyclass]
#[derive(Clone)]
pub struct PyPackage {
    pub(crate) inner: Package,
}

#[pymethods]
impl PyPackage {
    #[new]
    pub fn new(name: PyValueString, version: PyValueString) -> Self {
        PyPackage {
            inner: Package {
                name: name.inner,
                version: version.inner,
            },
        }
    }

    #[getter]
    pub fn name(&self) -> PyValueString {
        PyValueString {
            inner: self.inner.name.clone(),
        }
    }

    #[getter]
    pub fn version(&self) -> PyValueString {
        PyValueString {
            inner: self.inner.version.clone(),
        }
    }
}

#[pyclass]
#[derive(Clone)]
pub struct PySource {
    pub(crate) inner: Source,
}

#[pymethods]
impl PySource {
    #[staticmethod]
    pub fn url(url_source: PyUrlSource) -> Self {
        PySource {
            inner: Source::Url(url_source.inner),
        }
    }

    #[staticmethod]
    pub fn path(path_source: PyPathSource) -> Self {
        PySource {
            inner: Source::Path(path_source.inner),
        }
    }

    pub fn is_url(&self) -> bool {
        matches!(self.inner, Source::Url(_))
    }

    pub fn is_path(&self) -> bool {
        matches!(self.inner, Source::Path(_))
    }
}

#[pyclass]
#[derive(Clone)]
pub struct PyUrlSource {
    pub(crate) inner: UrlSource,
}

#[pymethods]
impl PyUrlSource {
    #[new]
    pub fn new(url: String, sha256: Option<String>) -> PyResult<Self> {
        Ok(PyUrlSource {
            inner: UrlSource {
                url: url
                    .parse()
                    .map_err(|e| PyValueError::new_err(format!("Invalid URL: {e}")))?,
                sha256: sha256.map(Value::Concrete),
            },
        })
    }

    #[getter]
    pub fn url(&self) -> String {
        self.inner.url.to_string()
    }

    #[getter]
    pub fn sha256(&self) -> Option<String> {
        self.inner
            .sha256
            .clone()
            .and_then(|v| v.concrete().cloned())
    }
}

#[pyclass]
#[derive(Clone)]
pub struct PyPathSource {
    pub(crate) inner: PathSource,
}

#[pymethods]
impl PyPathSource {
    #[new]
    pub fn new(path: String, sha256: Option<String>) -> Self {
        PyPathSource {
            inner: PathSource {
                path: Value::Concrete(path),
                sha256: sha256.map(Value::Concrete),
            },
        }
    }

    #[getter]
    pub fn path(&self) -> String {
        self.inner.path.to_string()
    }

    #[getter]
    pub fn sha256(&self) -> Option<String> {
        self.inner
            .sha256
            .clone()
            .and_then(|v| v.concrete().cloned())
    }
}

#[pyclass]
#[derive(Clone, Default)]
pub struct PyBuild {
    pub(crate) inner: Build,
}

#[pymethods]
impl PyBuild {
    #[new]
    pub fn new() -> Self {
        PyBuild {
            inner: Build::default(),
        }
    }

    #[getter]
    pub fn number(&self) -> Option<PyValueU64> {
        self.inner.number.clone().map(|n| PyValueU64 { inner: n })
    }

    #[getter]
    pub fn script(&self) -> PyScript {
        PyScript {
            inner: self.inner.script.clone(),
        }
    }

    #[getter]
    pub fn noarch(&self) -> Option<PyNoArchKind> {
        self.inner.noarch.clone().map(|n| PyNoArchKind { inner: n })
    }

    #[getter]
    pub fn python(&self) -> PyPython {
        PyPython {
            inner: self.inner.python.clone(),
        }
    }

    #[setter]
    pub fn set_number(&mut self, number: Option<PyValueU64>) {
        self.inner.number = number.map(|n| n.inner);
    }

    #[setter]
    pub fn set_script(&mut self, script: PyScript) {
        self.inner.script = script.inner;
    }

    #[setter]
    pub fn set_noarch(&mut self, noarch: Option<PyNoArchKind>) {
        self.inner.noarch = noarch.map(|n| n.inner);
    }

    #[setter]
    pub fn set_python(&mut self, python: PyPython) {
        self.inner.python = python.inner;
    }
}

impl From<Build> for PyBuild {
    fn from(build: Build) -> Self {
        PyBuild { inner: build }
    }
}

#[pyclass]
#[derive(Clone)]
pub struct PyScript {
    pub(crate) inner: Script,
}

#[pymethods]
impl PyScript {
    #[new]
    pub fn new(content: Vec<String>, env: Option<HashMap<String, String>>) -> Self {
        PyScript {
            inner: Script {
                content,
                env: env.unwrap_or_default().into_iter().collect(),
                secrets: Vec::new(),
            },
        }
    }

    #[getter]
    pub fn content(&self) -> Vec<String> {
        self.inner.content.clone()
    }

    #[getter]
    pub fn env(&self) -> HashMap<String, String> {
        self.inner.env.clone().into_iter().collect()
    }

    #[getter]
    pub fn secrets(&self) -> Vec<String> {
        self.inner.secrets.clone()
    }

    #[setter]
    pub fn set_content(&mut self, content: Vec<String>) {
        self.inner.content = content;
    }

    #[setter]
    pub fn set_env(&mut self, env: HashMap<String, String>) {
        self.inner.env = env.into_iter().collect();
    }

    #[setter]
    pub fn set_secrets(&mut self, secrets: Vec<String>) {
        self.inner.secrets = secrets;
    }
}

#[pyclass]
#[derive(Clone)]
pub struct PyPython {
    pub(crate) inner: Python,
}

#[pymethods]
impl PyPython {
    #[new]
    pub fn new(entry_points: Vec<String>) -> PyResult<Self> {
        let entry_points: Result<Vec<EntryPoint>, _> =
            entry_points.into_iter().map(|s| s.parse()).collect();

        match entry_points {
            Ok(entry_points) => Ok(PyPython {
                inner: Python { entry_points },
            }),
            Err(_) => Err(pyo3::exceptions::PyValueError::new_err(
                "Invalid entry point format",
            )),
        }
    }

    #[getter]
    pub fn entry_points(&self) -> Vec<String> {
        self.inner
            .entry_points
            .iter()
            .map(|e| e.to_string())
            .collect()
    }

    #[setter]
    pub fn set_entry_points(&mut self, entry_points: Vec<String>) -> PyResult<()> {
        let entry_points: Result<Vec<EntryPoint>, _> =
            entry_points.into_iter().map(|s| s.parse()).collect();

        match entry_points {
            Ok(entry_points) => {
                self.inner.entry_points = entry_points;
                Ok(())
            }
            Err(_) => Err(pyo3::exceptions::PyValueError::new_err(
                "Invalid entry point format",
            )),
        }
    }
}

#[pyclass]
#[derive(Clone)]
pub struct PyNoArchKind {
    pub(crate) inner: NoArchKind,
}

#[pymethods]
impl PyNoArchKind {
    #[staticmethod]
    pub fn python() -> Self {
        PyNoArchKind {
            inner: NoArchKind::Python,
        }
    }

    #[staticmethod]
    pub fn generic() -> Self {
        PyNoArchKind {
            inner: NoArchKind::Generic,
        }
    }

    pub fn is_python(&self) -> bool {
        matches!(self.inner, NoArchKind::Python)
    }

    pub fn is_generic(&self) -> bool {
        matches!(self.inner, NoArchKind::Generic)
    }
}

macro_rules! create_py_value {
    ($name: ident, $type: ident) => {
        #[pyclass]
        #[derive(Clone)]
        pub struct $name {
            pub(crate) inner: Value<$type>,
        }

        #[pymethods]
        impl $name {
            #[staticmethod]
            pub fn concrete(value: $type) -> Self {
                $name {
                    inner: Value::Concrete(value),
                }
            }

            #[staticmethod]
            pub fn template(template: String) -> Self {
                $name {
                    inner: Value::Template(template),
                }
            }

            pub fn is_concrete(&self) -> bool {
                matches!(self.inner, Value::Concrete(_))
            }

            pub fn is_template(&self) -> bool {
                matches!(self.inner, Value::Template(_))
            }

            pub fn get_concrete(&self) -> Option<$type> {
                match &self.inner {
                    Value::Concrete(v) => Some(v.clone()),
                    _ => None,
                }
            }

            pub fn get_template(&self) -> Option<String> {
                match &self.inner {
                    Value::Template(t) => Some(t.clone()),
                    _ => None,
                }
            }
        }
    };
}

create_py_value!(PyValueString, String);
create_py_value!(PyValueU64, u64);

#[pyclass]
#[derive(Clone, Default)]
pub struct PyConditionalRequirements {
    pub(crate) inner: ConditionalRequirements,
}

#[pymethods]
impl PyConditionalRequirements {
    #[new]
    pub fn new() -> Self {
        PyConditionalRequirements {
            inner: ConditionalRequirements::default(),
        }
    }

    #[getter]
    // We erase the type here to return a list of PyItemPackageDependency
    // which can be used in Python as list[PyItemPackageDependency]
    pub fn build(&self) -> PyResult<Py<PyList>> {
        BindingPython::with_gil(|py| {
            let list = PyList::empty(py);
            for dep in &self.inner.build {
                list.append(PyItemPackageDependency { inner: dep.clone() })?;
            }
            Ok(list.unbind())
        })
    }

    #[getter]
    pub fn host(&self) -> PyResult<Py<PyList>> {
        BindingPython::with_gil(|py| {
            let list = PyList::empty(py);
            for dep in &self.inner.host {
                list.append(PyItemPackageDependency { inner: dep.clone() })?;
            }
            Ok(list.unbind())
        })
    }

    #[getter]
    pub fn run(&self) -> PyResult<Py<PyList>> {
        BindingPython::with_gil(|py| {
            let list = PyList::empty(py);
            for dep in &self.inner.run {
                list.append(PyItemPackageDependency { inner: dep.clone() })?;
            }
            Ok(list.unbind())
        })
    }

    #[getter]
    pub fn run_constraints(&self) -> PyResult<Py<PyList>> {
        BindingPython::with_gil(|py| {
            let list = PyList::empty(py);
            for dep in &self.inner.run_constraints {
                list.append(PyItemPackageDependency { inner: dep.clone() })?;
            }
            Ok(list.unbind())
        })
    }

    #[setter]
    pub fn set_build(&mut self, build: Vec<Bound<'_, PyAny>>) -> PyResult<()> {
        self.inner.build = build
            .into_iter()
            .map(|item| Ok(PyItemPackageDependency::try_from(item)?.inner))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(())
    }

    #[setter]
    pub fn set_host(&mut self, host: Vec<Bound<'_, PyAny>>) -> PyResult<()> {
        self.inner.host = host
            .into_iter()
            .map(|item| Ok(PyItemPackageDependency::try_from(item)?.inner))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(())
    }

    #[setter]
    pub fn set_run(&mut self, run: Vec<Bound<'_, PyAny>>) -> PyResult<()> {
        self.inner.run = run
            .into_iter()
            .map(|item| Ok(PyItemPackageDependency::try_from(item)?.inner))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(())
    }

    #[setter]
    pub fn set_run_constraints(&mut self, run_constraints: Vec<Bound<'_, PyAny>>) -> PyResult<()> {
        self.inner.run_constraints = run_constraints
            .into_iter()
            .map(|item| Ok(PyItemPackageDependency::try_from(item)?.inner))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(())
    }

    pub fn resolve(&self, host_platform: Option<&PyPlatform>) -> PyPackageSpecDependencies {
        let platform = host_platform.map(|p| p.inner);

        let resolved = self.inner.resolve(platform);

        resolved.into()
    }
}

impl From<ConditionalRequirements> for PyConditionalRequirements {
    fn from(requirements: ConditionalRequirements) -> Self {
        PyConditionalRequirements {
            inner: requirements,
        }
    }
}

#[pyclass]
#[derive(Clone, Default)]
pub struct PyAbout {
    pub(crate) inner: About,
}

#[pymethods]
impl PyAbout {
    #[new]
    pub fn new() -> Self {
        PyAbout {
            inner: About::default(),
        }
    }

    #[getter]
    pub fn homepage(&self) -> Option<String> {
        self.inner
            .homepage
            .clone()
            .and_then(|v| v.concrete().cloned())
    }

    #[getter]
    pub fn license(&self) -> Option<String> {
        self.inner
            .license
            .clone()
            .and_then(|v| v.concrete().cloned())
    }

    #[getter]
    pub fn summary(&self) -> Option<String> {
        self.inner
            .summary
            .clone()
            .and_then(|v| v.concrete().cloned())
    }

    #[getter]
    pub fn description(&self) -> Option<String> {
        self.inner
            .description
            .clone()
            .and_then(|v| v.concrete().cloned())
    }
}

#[pyclass]
#[derive(Clone, Default)]
pub struct PyExtra {
    pub(crate) inner: Extra,
}

#[pymethods]
impl PyExtra {
    #[new]
    pub fn new() -> Self {
        PyExtra {
            inner: Extra::default(),
        }
    }

    #[getter]
    pub fn recipe_maintainers(&self) -> PyResult<Py<PyList>> {
        BindingPython::with_gil(|py| {
            let list = PyList::empty(py);
            for dep in &self.inner.recipe_maintainers {
                list.append(PyItemString { inner: dep.clone() })?;
            }
            Ok(list.unbind())
        })
    }
}
