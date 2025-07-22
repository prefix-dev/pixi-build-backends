use pyo3::{PyResult, pyfunction};
use pyproject_toml::PyProjectToml;
use rattler_conda_types::package::EntryPoint;
use std::collections::HashMap;

/// Parse entry points from a pyproject.toml content string
#[pyfunction]
pub fn parse_entry_points_from_pyproject(content: String) -> PyResult<Vec<String>> {
    let pyproject: PyProjectToml = toml::from_str(&content).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Invalid pyproject.toml: {}", e))
    })?;

    let entry_points = pyproject
        .project
        .as_ref()
        .and_then(|p| p.scripts.as_ref())
        .map(|scripts| {
            scripts
                .iter()
                .map(|(name, entry_point)| format!("{} = {}", name, entry_point))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(entry_points)
}

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

/// Validate an entry point string
#[pyfunction]
pub fn validate_entry_point(entry_point: String) -> PyResult<bool> {
    match entry_point.parse::<EntryPoint>() {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Parse a single entry point string and return its components
#[pyfunction]
pub fn parse_entry_point(entry_point: String) -> PyResult<(String, String, Option<String>)> {
    let ep: EntryPoint = entry_point.parse().map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Invalid entry point: {}", e))
    })?;

    Ok((ep.command, ep.module, Some(ep.function)))
}

/// Create an entry point string from components
#[pyfunction]
pub fn create_entry_point(name: String, module: String, function: Option<String>) -> String {
    match function {
        Some(function) => format!("{} = {}:{}", name, module, function),
        None => format!("{} = {}", name, module),
    }
}
