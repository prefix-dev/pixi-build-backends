use std::{
    ffi::CString,
    io::Write,
    path::{Path, PathBuf},
};

use miette::IntoDiagnostic;
use pixi_build_backend::source::Source;
use pixi_build_types::ProjectModelV1;
use pyo3::{
    types::{PyAnyMethods as _, PyModule},
    PyResult, Python,
};
use rattler_build::console_utils::LoggingOutputHandler;
use tempfile::NamedTempFile;

use crate::config::PyExtConfig;

pub struct RattlerBuildBackend {
    pub(crate) logging_output_handler: LoggingOutputHandler,
    /// In case of rattler-build, manifest is the raw recipe
    /// We need to apply later the selectors to get the final recipe
    pub(crate) recipe_source: Source,
    pub(crate) cache_dir: Option<PathBuf>,
    pub(crate) config: PyExtConfig,

    _temp_recipe_file: NamedTempFile,
}

impl RattlerBuildBackend {
    /// Returns a new instance of [`RattlerBuildBackend`] by reading the
    /// manifest at the given path.
    pub fn new(
        manifest_path: &Path,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
        config: PyExtConfig,
        project_model: ProjectModelV1,
    ) -> miette::Result<Self> {
        // Locate the recipe
        // Create a temporary file to hold the generated recipe
        // Try to place it relative to the manifest path for context, otherwise use system temp
        let mut temp_file = NamedTempFile::with_suffix(".yaml")
            .into_diagnostic()
            .map_err(|e| miette::miette!("Failed to create temporary file: {}", e))?;

        eprintln!("Manifest path: {}", manifest_path.display());
        let pyscript = PathBuf::from("backend.py");
        let py_script_path = manifest_path.parent().unwrap().join(&pyscript);
        eprintln!("Python script path: {}", py_script_path.display());
        eprintln!("Python script path: xx {:?}", &pyscript);

        let py_file_content = fs_err::read_to_string(&py_script_path)
            .into_diagnostic()
            .map_err(|e| miette::miette!("Failed to read Python script: {}", e))?;
        let c_str = CString::new(py_file_content).unwrap();

        // Call the Python function
        let generated_recipe_content = Python::with_gil(|py| -> PyResult<String> {
            let spec = PyModule::from_code(
                py,
                &c_str,
                &CString::new("recipe_generator.py").unwrap(),
                &CString::new("recipe_generator").unwrap(),
            )?;

            let func = spec.getattr("generate_recipe")?;

            let project_model_json =
                serde_json::to_string(&project_model).expect("Failed to serialize project model");
            let config_json = serde_json::to_string(&config).expect("Failed to serialize config");

            let args = (project_model_json, config_json);
            let result = func.call1(args)?;
            result.extract::<String>()
        })
        .map_err(|e| {
            miette::miette!(
                "Python error generating recipe via '{}.{}': {}",
                "recipe_generator",
                "generate_recipe",
                e
            )
        })?;

        eprintln!("Generated recipe content:\n{}\n", generated_recipe_content);

        // Write the generated recipe to the temporary file
        temp_file
            .write_all(generated_recipe_content.as_bytes())
            .into_diagnostic()
            .map_err(|e| miette::miette!("Failed to write to temporary file: {}", e))?;

        // Load the manifest from the source directory
        let manifest_root = manifest_path.parent().expect("manifest must have a root");
        let recipe_source = Source::from_rooted_path(manifest_root, temp_file.path().to_path_buf())
            .into_diagnostic()?;

        Ok(Self {
            recipe_source,
            logging_output_handler,
            cache_dir,
            config,
            _temp_recipe_file: temp_file,
        })
    }
}
