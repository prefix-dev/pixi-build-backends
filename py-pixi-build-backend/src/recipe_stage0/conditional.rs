use pyo3::exceptions::PyTypeError;
use pyo3::types::PyAnyMethods;
use pyo3::{Bound, FromPyObject, PyAny, PyErr, intern, pyclass, pymethods};
use recipe_stage0::matchspec::PackageDependency;
use recipe_stage0::recipe::{Conditional, Item, ListOrItem};

macro_rules! create_py_item {
    ($name: ident, $type: ident) => {
        #[pyclass]
        #[derive(Clone)]
        pub struct $name {
            pub(crate) inner: Item<$type>,
        }

        #[pymethods]
        impl $name {
            pub fn is_value(&self) -> bool {
                matches!(self.inner, Item::Value(_))
            }

            pub fn is_conditional(&self) -> bool {
                matches!(self.inner, Item::Conditional(_))
            }
        }
    };
}

create_py_item!(PyItemPackageDependency, PackageDependency);
create_py_item!(PyItemString, String);

macro_rules! create_pylist_or_item {
    ($name: ident, $type: ident) => {
        #[pyclass]
        #[derive(Clone)]
        pub struct $name {
            pub(crate) inner: ListOrItem<$type>,
        }

        #[pymethods]
        impl $name {
            #[staticmethod]
            pub fn single(item: $type) -> Self {
                $name {
                    inner: ListOrItem::single(item),
                }
            }

            #[staticmethod]
            pub fn list(items: Vec<$type>) -> Self {
                $name {
                    inner: ListOrItem::new(items),
                }
            }

            pub fn is_single(&self) -> bool {
                self.inner.len() == 1
            }

            pub fn is_list(&self) -> bool {
                self.inner.len() > 1
            }

            pub fn get_single(&self) -> Option<String> {
                if self.inner.len() == 1 {
                    self.inner.0.first().cloned()
                } else {
                    None
                }
            }

            pub fn get_list(&self) -> Vec<String> {
                self.inner.0.clone()
            }
        }
    };
}

create_pylist_or_item!(PyListOrItemString, String);

macro_rules! create_conditional_interface {
    ($name: ident, $type: ident) => {
        paste::paste! {
            #[pyclass]
            #[derive(Clone)]
            pub struct $name {
                pub(crate) inner: Conditional<$type>,
            }

            #[pymethods]
            impl $name {
                #[new]
                pub fn new(
                    condition: String,
                    then_value: [<PyListOrItem $type>],
                    else_value: Option<[<PyListOrItem $type>]>,
                ) -> Self {
                    $name {
                        inner: Conditional {
                            condition,
                            then: then_value.inner,
                            else_value: else_value.map(|e| e.inner).unwrap_or_default(),
                        },
                    }
                }

                #[getter]
                pub fn condition(&self) -> $type {
                    self.inner.condition.clone()
                }

                #[getter]
                pub fn then_value(&self) -> [<PyListOrItem $type>] {
                    [<PyListOrItem $type>] {
                        inner: self.inner.then.clone(),
                    }
                }

                #[getter]
                pub fn else_value(&self) -> [<PyListOrItem $type>] {
                    [<PyListOrItem $type>] {
                        inner: self.inner.else_value.clone(),
                    }
                }
            }
        }
    };
}

create_conditional_interface!(PyConditionalString, String);

impl<'a> TryFrom<Bound<'a, PyAny>> for PyItemPackageDependency {
    type Error = PyErr;
    fn try_from(value: Bound<'a, PyAny>) -> Result<Self, Self::Error> {
        let intern_val = intern!(value.py(), "_inner");
        if !value.hasattr(intern_val)? {
            return Err(PyTypeError::new_err(
                "object is not a PackageDependency type",
            ));
        }

        let inner = value.getattr(intern_val)?;
        if !inner.is_instance_of::<Self>() {
            return Err(PyTypeError::new_err("'_inner' is invalid"));
        }

        PyItemPackageDependency::extract_bound(&inner)
    }
}

impl From<Conditional<String>> for PyConditionalString {
    fn from(conditional: Conditional<String>) -> Self {
        PyConditionalString { inner: conditional }
    }
}

impl From<PyConditionalString> for Conditional<String> {
    fn from(py_conditional: PyConditionalString) -> Self {
        py_conditional.inner
    }
}

impl From<ListOrItem<String>> for PyListOrItemString {
    fn from(list_or_item: ListOrItem<String>) -> Self {
        PyListOrItemString {
            inner: list_or_item,
        }
    }
}

impl From<PyListOrItemString> for ListOrItem<String> {
    fn from(py_list_or_item: PyListOrItemString) -> Self {
        py_list_or_item.inner
    }
}
