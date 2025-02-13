use miette::{Context, IntoDiagnostic};
use pixi_build_backend::protocol::{Protocol, ProtocolInstantiator};
use pixi_build_types::procedures::{
    conda_build::{CondaBuildParams, CondaBuildResult},
    conda_metadata::{CondaMetadataParams, CondaMetadataResult},
    initialize::{InitializeParams, InitializeResult},
    negotiate_capabilities::{NegotiateCapabilitiesParams, NegotiateCapabilitiesResult},
};
use rattler_build::console_utils::LoggingOutputHandler;

use crate::{config::TestingBackendConfig, testing::TestingBackend};
pub struct TestingBackendInstantiator {
    logging_output_handler: LoggingOutputHandler,
}

impl TestingBackendInstantiator {
    /// This type implements [`ProtocolInstantiator`] and can be used to
    /// initialize a new [`TestingBackend`].
    pub fn new(logging_output_handler: LoggingOutputHandler) -> TestingBackendInstantiator {
        TestingBackendInstantiator {
            logging_output_handler,
        }
    }
}

#[async_trait::async_trait]
impl Protocol for TestingBackend {
    async fn conda_get_metadata(
        &self,
        params: CondaMetadataParams,
    ) -> miette::Result<CondaMetadataResult> {
        let json = serde_json::to_string(&params)
            .into_diagnostic()
            .context("failed to serialize parameters to JSON")?;

        fs_err::tokio::create_dir_all(&self.config.data_dir)
            .await
            .into_diagnostic()
            .context("failed to create data directory")?;

        let path = self.config.data_dir.join("conda_metadata_params.json");
        fs_err::tokio::write(&path, json)
            .await
            .into_diagnostic()
            .context("failed to write JSON to file")?;

        Ok(CondaMetadataResult {
            packages: Vec::new(),
            input_globs: None,
        })
    }

    async fn conda_build(&self, params: CondaBuildParams) -> miette::Result<CondaBuildResult> {
        let json = serde_json::to_string(&params)
            .into_diagnostic()
            .context("failed to serialize parameters to JSON")?;

        fs_err::tokio::create_dir_all(&self.config.data_dir)
            .await
            .into_diagnostic()
            .context("failed to create data directory")?;

        let path = self.config.data_dir.join("conda_build_params.json");
        fs_err::tokio::write(&path, json)
            .await
            .into_diagnostic()
            .context("failed to write JSON to file")?;

        Ok(CondaBuildResult {
            packages: Vec::new(),
        })
    }
}

#[async_trait::async_trait]
impl ProtocolInstantiator for TestingBackendInstantiator {
    type ProtocolEndpoint = TestingBackend;

    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> miette::Result<(Self::ProtocolEndpoint, InitializeResult)> {
        let project_model = params
            .project_model
            .ok_or_else(|| miette::miette!("project model is required"))?
            .into_v1()
            .ok_or_else(|| miette::miette!("project model v1 is required"))?;

        let config = if let Some(config) = params.configuration {
            serde_json::from_value(config)
                .into_diagnostic()
                .context("failed to parse configuration")?
        } else {
            TestingBackendConfig::default()
        };

        let project_model_json = serde_json::to_string(&project_model)
            .into_diagnostic()
            .context("failed to serialize project model to JSON")?;

        let project_model_path = config.data_dir.join("project_model.json");
        fs_err::tokio::write(&project_model_path, project_model_json)
            .await
            .into_diagnostic()
            .context("failed to write project model JSON to file")?;

        let instance = TestingBackend {
            config,
            logging_output_handler: self.logging_output_handler.clone(),
        };

        Ok((instance, InitializeResult {}))
    }

    async fn negotiate_capabilities(
        params: NegotiateCapabilitiesParams,
    ) -> miette::Result<NegotiateCapabilitiesResult> {
        Ok(NegotiateCapabilitiesResult {
            capabilities: TestingBackend::capabilities(&params.capabilities),
        })
    }
}
