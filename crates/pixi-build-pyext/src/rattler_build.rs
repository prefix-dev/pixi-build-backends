use std::{
    ffi::OsStr, io::Write, path::{Path, PathBuf}
};

use miette::IntoDiagnostic;
use pixi_build_backend::source::Source;
use pyo3::{types::{PyAnyMethods as _, PyModule}, PyResult, Python};
use rattler_build::console_utils::LoggingOutputHandler;
use tempfile::NamedTempFile;

use crate::config::RattlerBuildBackendConfig;

pub struct RattlerBuildBackend {
    pub(crate) logging_output_handler: LoggingOutputHandler,
    /// In case of rattler-build, manifest is the raw recipe
    /// We need to apply later the selectors to get the final recipe
    pub(crate) recipe_source: Source,
    pub(crate) cache_dir: Option<PathBuf>,
    pub(crate) config: RattlerBuildBackendConfig,
}

impl RattlerBuildBackend {
    /// Returns a new instance of [`RattlerBuildBackend`] by reading the
    /// manifest at the given path.
    pub fn new(
        manifest_path: &Path,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
        config: RattlerBuildBackendConfig,
    ) -> miette::Result<Self> {
        // Locate the recipe
        // Create a temporary file to hold the generated recipe
        // Try to place it relative to the manifest path for context, otherwise use system temp
        let mut temp_file = NamedTempFile::with_suffix(".yaml")
            .into_diagnostic()
            .map_err(|e| miette::miette!("Failed to create temporary file: {}", e))?;
        // Call the Python function
        let generated_recipe_content = Python::with_gil(|py| -> PyResult<String> {
            let module = PyModule::import(py, "recipe_generator")?;
            let func = module.getattr("generate_recipe")?;
            let result = func.call0()?;
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

        // Write the generated recipe to the temporary file
        std::fs::write(&temp_file, &generated_recipe_content)
            .into_diagnostic()
            .map_err(|e| miette::miette!("Failed to write to temporary file: {}", e))?;

        // Load the manifest from the source directory
        let manifest_root = manifest_path.parent().expect("manifest must have a root");
        let recipe_source =
            Source::from_rooted_path(manifest_root, temp_file.path().to_path_buf()).into_diagnostic()?;

        Ok(Self {
            recipe_source,
            logging_output_handler,
            cache_dir,
            config,
        })
    }
}
