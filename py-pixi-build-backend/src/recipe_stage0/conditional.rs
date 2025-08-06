use ::serde::{Deserialize, Serialize};
use pyo3::exceptions::PyTypeError;
use pyo3::types::PyAnyMethods;
use pyo3::{Bound, FromPyObject, PyAny, PyErr, PyResult, intern, pyclass, pymethods};
use recipe_stage0::matchspec::PackageDependency;
use recipe_stage0::recipe::{Conditional, Item, ListOrItem, Source, Value};
use std::fmt::Display;

use std::ops::Deref;

use crate::recipe_stage0::recipe::PySource;
use crate::recipe_stage0::requirements::PyPackageDependency;

/// Creates a PyItem class for a given type.
/// The first argument is the name of the class, the second
/// is the type it wraps, and the third is the Python type.
/// It is necessary to provide the Python type because
/// the String equivalent is still String
/// but for other types it will be some type
/// prefixed with Py, like PyPackageDependency.
macro_rules! create_py_item {
    ($name: ident, $type: ident, $py_type: ident) => {
        paste::paste! {
            #[pyclass]
            #[derive(Clone, Serialize, Deserialize)]
            pub struct $name {
                pub(crate) inner: Item<$type>,
            }

            #[pymethods]
            impl $name {
                #[new]
                pub fn new(value: String) -> PyResult<Self> {
                    let item: Item<_> = value
                        .parse()
                        .map_err(|_| PyTypeError::new_err(format!("Failed to parse {value}")))?;

                    Ok($name { inner: item })
                }

                pub fn is_value(&self) -> bool {
                    matches!(self.inner, Item::Value(_))
                }

                pub fn is_template(&self) -> bool {
                    matches!(self.inner, Item::Value(Value::Template(_)))
                }

                pub fn is_conditional(&self) -> bool {
                    matches!(self.inner, Item::Conditional(_))
                }

                pub fn __str__(&self) -> String {
                    self.inner.to_string()
                }

                pub fn concrete(&self) -> Option<$py_type> {
                    if let Item::Value(Value::Concrete(val)) = &self.inner {
                        Some(val.clone().into())
                    } else {
                        None
                    }
                }

                pub fn template(&self) -> Option<String> {
                    if let Item::Value(Value::Template(val)) = &self.inner {
                        Some(val.clone())
                    } else {
                        None
                    }
                }

                pub fn conditional(&self) -> Option<[<PyConditional $type>]> {
                    if let Item::Conditional(cond) = &self.inner {
                        Some(cond.clone().into())
                    } else {
                        None
                    }
                }
            }

            impl Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "{}", self.inner)
                }
            }

            impl From<Item<$type>> for $name {
                fn from(item: Item<$type>) -> Self {
                    $name { inner: item }
                }
            }

            impl Deref for $name {
                type Target = Item<$type>;
                fn deref(&self) -> &Self::Target {
                    &self.inner
                }
            }
        }
    };
}

create_py_item!(
    PyItemPackageDependency,
    PackageDependency,
    PyPackageDependency
);
create_py_item!(PyItemString, String, String);
create_py_item!(PyItemSource, Source, PySource);

macro_rules! create_pylist_or_item {
    ($name: ident, $type: ident) => {
        #[pyclass]
        #[derive(Clone)]
        pub struct $name {
            pub(crate) inner: ListOrItem<$type>,
        }

        #[pymethods]
        impl $name {
            // #[staticmethod]
            // pub fn single(item: $type) -> Self {
            //     $name {
            //         inner: ListOrItem::single(item),
            //     }
            // }

            // #[staticmethod]
            // pub fn list(items: Vec<$type>) -> Self {
            //     $name {
            //         inner: ListOrItem::new(items),
            //     }
            // }

            pub fn is_single(&self) -> bool {
                self.inner.len() == 1
            }

            pub fn is_list(&self) -> bool {
                self.inner.len() > 1
            }

            // pub fn get_single(&self) -> Option<$type> {
            //     if self.inner.len() == 1 {
            //         self.inner.0.first().cloned()
            //     } else {
            //         None
            //     }
            // }

            // pub fn get_list(&self) -> Vec<$type> {
            //     self.inner.0.clone()
            // }
        }
    };
}

create_pylist_or_item!(PyListOrItemString, String);
create_pylist_or_item!(PyListOrItemPackageDependency, PackageDependency);

create_pylist_or_item!(PyListOrItemSource, Source);

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
                pub fn condition(&self) -> String {
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

            impl From<Conditional<$type>> for $name {
                fn from(inner: Conditional<$type>) -> Self {
                    $name { inner }
                }
            }
        }
    };
}

create_conditional_interface!(PyConditionalString, String);
create_conditional_interface!(PyConditionalPackageDependency, PackageDependency);

create_conditional_interface!(PyConditionalSource, Source);

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

// impl From<Conditional<String>> for PyConditionalString {
//     fn from(conditional: Conditional<String>) -> Self {
//         PyConditionalString { inner: conditional }
//     }
// }

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
