use pyo3::{PyResult, pyfunction};
use rattler_conda_types::package::EntryPoint;
use std::collections::HashMap;

/// Parse entry points from a dictionary of scripts
#[pyfunction]
pub fn parse_entry_points_from_scripts(scripts: HashMap<String, String>) -> PyResult<Vec<String>> {
    let entry_points: Result<Vec<EntryPoint>, _> = scripts
        .into_iter()
        .map(|(name, entry_point)| format!("{} = {}", name, entry_point).parse())
        .collect();

    match entry_points {
        Ok(entry_points) => Ok(entry_points.into_iter().map(|ep| ep.to_string()).collect()),
        Err(_) => Err(pyo3::exceptions::PyValueError::new_err(
            "Invalid entry point format",
        )),
    }
}
