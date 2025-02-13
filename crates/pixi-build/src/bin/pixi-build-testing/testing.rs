use pixi_build_types::{BackendCapabilities, FrontendCapabilities};
use rattler_build::console_utils::LoggingOutputHandler;

use crate::config::TestingBackendConfig;

pub struct TestingBackend {
    #[allow(unused)]
    pub logging_output_handler: LoggingOutputHandler,

    pub(crate) config: TestingBackendConfig,
}

impl TestingBackend {
    /// Returns the capabilities of this backend based on the capabilities of
    /// the frontend.
    pub fn capabilities(_frontend_capabilities: &FrontendCapabilities) -> BackendCapabilities {
        BackendCapabilities {
            provides_conda_metadata: Some(true),
            provides_conda_build: Some(true),
            highest_supported_project_model: None,
        }
    }
}
