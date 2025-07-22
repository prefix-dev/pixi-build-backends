use pyo3::prelude::*;

// mod error;
// mod backends;
mod cli;
mod recipe_stage0;
mod types;

#[pymodule]
fn pixi_build_backend(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // // Add exception types
    // m.add("PyPixiBuildError", _py.get_type::<PyPixiBuildError>())?;

    // Add core types
    m.add_class::<types::PyPlatform>()?;
    m.add_class::<types::PyProjectModelV1>()?;
    m.add_class::<types::PyGeneratedRecipe>()?;
    m.add_class::<types::PyGenerateRecipe>()?;
    m.add_class::<types::PyPythonParams>()?;
    m.add_class::<types::PyBackendConfig>()?;

    // Add recipe_stage0 types
    m.add_class::<recipe_stage0::recipe::PyIntermediateRecipe>()?;
    m.add_class::<recipe_stage0::recipe::PyPackage>()?;
    m.add_class::<recipe_stage0::recipe::PySource>()?;
    m.add_class::<recipe_stage0::recipe::PyUrlSource>()?;
    m.add_class::<recipe_stage0::recipe::PyPathSource>()?;
    m.add_class::<recipe_stage0::recipe::PyBuild>()?;
    m.add_class::<recipe_stage0::recipe::PyScript>()?;
    m.add_class::<recipe_stage0::recipe::PyPython>()?;
    m.add_class::<recipe_stage0::recipe::PyNoArchKind>()?;
    m.add_class::<recipe_stage0::recipe::PyValueString>()?;
    m.add_class::<recipe_stage0::recipe::PyValueU64>()?;
    m.add_class::<recipe_stage0::recipe::PyConditionalRequirements>()?;
    m.add_class::<recipe_stage0::recipe::PyAbout>()?;
    m.add_class::<recipe_stage0::recipe::PyExtra>()?;

    // Add requirements types
    m.add_class::<recipe_stage0::requirements::PyPackageSpecDependencies>()?;
    m.add_class::<recipe_stage0::requirements::PyPackageDependency>()?;
    m.add_class::<recipe_stage0::requirements::PySourceMatchSpec>()?;
    m.add_class::<recipe_stage0::requirements::PySerializableMatchSpec>()?;
    m.add_class::<recipe_stage0::requirements::PySelector>()?;

    // Add conditional types
    // m.add_class::<recipe_stage0::conditional::PyItemString>()?;
    m.add_class::<recipe_stage0::conditional::PyConditionalString>()?;
    m.add_class::<recipe_stage0::conditional::PyListOrItemString>()?;

    // m.add_class::<recipe_stage0::conditional::PyConditionalListString>()?;
    m.add_class::<recipe_stage0::conditional::PyItemPackageDependency>()?;
    // m.add_class::<recipe_stage0::conditional::PyConditionalListPackageDependency>()?;

    // Add utility functions
    m.add_function(wrap_pyfunction!(
        recipe_stage0::utils::parse_entry_points_from_pyproject,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        recipe_stage0::utils::parse_entry_points_from_scripts,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        recipe_stage0::utils::validate_entry_point,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        recipe_stage0::utils::parse_entry_point,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        recipe_stage0::utils::create_entry_point,
        m
    )?)?;

    m.add_function(wrap_pyfunction!(cli::py_main, m)?)?;

    Ok(())
}
