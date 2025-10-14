use std::{net::SocketAddr, path::Path, sync::Arc};

use fs_err::tokio as tokio_fs;
use jsonrpc_core::{Error, IoHandler, Params, serde_json, to_value};
use miette::{Context, IntoDiagnostic, JSONReportHandler};
use pixi_build_types::{
    VersionedProjectModel,
    procedures::{
        self, conda_build_v1::CondaBuildV1Params, conda_outputs::CondaOutputsParams,
        initialize::InitializeParams, negotiate_capabilities::NegotiateCapabilitiesParams,
    },
};
use tokio::sync::RwLock;

use crate::protocol::{Protocol, ProtocolInstantiator};

/// A JSONRPC server that can be used to communicate with a client.
pub struct Server<T: ProtocolInstantiator> {
    instatiator: T,
}

enum ServerState<T: ProtocolInstantiator> {
    /// Server has not been initialized yet.
    Uninitialized(T),
    /// Server has been initialized, with a protocol.
    Initialized(Box<dyn Protocol + Send + Sync + 'static>),
}

impl<T: ProtocolInstantiator> ServerState<T> {
    /// Convert to a protocol, if the server has been initialized.
    pub fn as_endpoint(
        &self,
    ) -> Result<&(dyn Protocol + Send + Sync + 'static), jsonrpc_core::Error> {
        match self {
            Self::Initialized(protocol) => Ok(protocol.as_ref()),
            _ => Err(Error::invalid_request()),
        }
    }
}

impl<T: ProtocolInstantiator> Server<T> {
    pub fn new(instatiator: T) -> Self {
        Self { instatiator }
    }

    /// Run the server, communicating over stdin/stdout.
    pub async fn run(self) -> miette::Result<()> {
        let io = self.setup_io();
        jsonrpc_stdio_server::ServerBuilder::new(io).build().await;
        Ok(())
    }

    /// Run the server, communicating over HTTP.
    pub fn run_over_http(self, port: u16) -> miette::Result<()> {
        let io = self.setup_io();
        jsonrpc_http_server::ServerBuilder::new(io)
            .start_http(&SocketAddr::from(([127, 0, 0, 1], port)))
            .into_diagnostic()?
            .wait();
        Ok(())
    }

    /// Setup the IO inner handler.
    fn setup_io(self) -> IoHandler {
        // Construct a server
        let mut io = IoHandler::new();
        io.add_method(
            procedures::negotiate_capabilities::METHOD_NAME,
            move |params: Params| async move {
                let params: NegotiateCapabilitiesParams = params.parse()?;
                let result = T::negotiate_capabilities(params)
                    .await
                    .map_err(convert_error)?;
                Ok(to_value(result).expect("failed to convert to json"))
            },
        );

        let state = Arc::new(RwLock::new(ServerState::Uninitialized(self.instatiator)));
        let initialize_state = state.clone();
        io.add_method(
            procedures::initialize::METHOD_NAME,
            move |params: Params| {
                let state = initialize_state.clone();

                async move {
                    let params: InitializeParams = params.parse()?;
                    let mut state = state.write().await;
                    let ServerState::Uninitialized(initializer) = &mut *state else {
                        return Err(Error::invalid_request());
                    };

                    let debug_dir = T::debug_dir(params.configuration.clone());
                    let _ =
                        log_initialize(debug_dir.as_deref(), params.project_model.clone()).await;

                    let (protocol_endpoint, result) = initializer
                        .initialize(params)
                        .await
                        .map_err(convert_error)?;
                    *state = ServerState::Initialized(protocol_endpoint);

                    Ok(to_value(result).expect("failed to convert to json"))
                }
            },
        );

        let conda_outputs = state.clone();
        io.add_method(
            procedures::conda_outputs::METHOD_NAME,
            move |params: Params| {
                let state = conda_outputs.clone();

                async move {
                    let params: CondaOutputsParams = params.parse()?;
                    let state = state.read().await;
                    let endpoint = state.as_endpoint()?;

                    let debug_dir = endpoint.debug_dir();
                    log_conda_outputs(debug_dir, &params)
                        .await
                        .map_err(convert_error)?;

                    endpoint
                        .conda_outputs(params)
                        .await
                        .map(|value| to_value(value).expect("failed to convert to json"))
                        .map_err(convert_error)
                }
            },
        );

        let conda_build_v1 = state.clone();
        io.add_method(
            procedures::conda_build_v1::METHOD_NAME,
            move |params: Params| {
                let state = conda_build_v1.clone();

                async move {
                    let params: CondaBuildV1Params = params.parse()?;
                    let state = state.read().await;
                    let endpoint = state.as_endpoint()?;

                    let debug_dir = endpoint.debug_dir();
                    log_conda_build_v1(debug_dir, &params)
                        .await
                        .map_err(convert_error)?;

                    endpoint
                        .conda_build_v1(params)
                        .await
                        .map(|value| to_value(value).expect("failed to convert to json"))
                        .map_err(convert_error)
                }
            },
        );

        io
    }
}

fn convert_error(err: miette::Report) -> jsonrpc_core::Error {
    let rendered = JSONReportHandler::new();
    let mut json_str = String::new();
    rendered
        .render_report(&mut json_str, err.as_ref())
        .expect("failed to convert error to json");
    let data = serde_json::from_str(&json_str).expect("failed to parse json error");
    jsonrpc_core::Error {
        code: jsonrpc_core::ErrorCode::ServerError(-32000),
        message: err.to_string(),
        data: Some(data),
    }
}

async fn log_initialize(
    debug_dir: Option<&Path>,
    project_model: Option<VersionedProjectModel>,
) -> miette::Result<()> {
    let Some(debug_dir) = debug_dir else {
        return Ok(());
    };

    let project_model = project_model
        .ok_or_else(|| miette::miette!("project model is required if debug_dir is given"))?
        .into_v1()
        .ok_or_else(|| miette::miette!("project model needs to be v1"))?;

    let project_model_json = serde_json::to_string_pretty(&project_model)
        .into_diagnostic()
        .context("failed to serialize project model to JSON")?;

    let project_model_path = debug_dir.join("project_model.json");
    tokio_fs::write(&project_model_path, project_model_json)
        .await
        .into_diagnostic()
        .context("failed to write project model JSON to file")?;
    Ok(())
}

async fn log_conda_outputs(
    debug_dir: Option<&Path>,
    params: &CondaOutputsParams,
) -> miette::Result<()> {
    let Some(debug_dir) = debug_dir else {
        return Ok(());
    };

    let json = serde_json::to_string_pretty(&params)
        .into_diagnostic()
        .context("failed to serialize parameters to JSON")?;

    tokio_fs::create_dir_all(&debug_dir)
        .await
        .into_diagnostic()
        .context("failed to create data directory")?;

    let path = debug_dir.join("conda_outputs_params.json");
    tokio_fs::write(&path, json)
        .await
        .into_diagnostic()
        .context("failed to write JSON to file")?;
    Ok(())
}

async fn log_conda_build_v1(
    debug_dir: Option<&Path>,
    params: &CondaBuildV1Params,
) -> miette::Result<()> {
    let Some(debug_dir) = debug_dir else {
        return Ok(());
    };

    let json = serde_json::to_string_pretty(&params)
        .into_diagnostic()
        .context("failed to serialize parameters to JSON")?;

    tokio_fs::create_dir_all(&debug_dir)
        .await
        .into_diagnostic()
        .context("failed to create data directory")?;

    let path = debug_dir.join("conda_build_v1_params.json");
    tokio_fs::write(&path, json)
        .await
        .into_diagnostic()
        .context("failed to write JSON to file")?;
    Ok(())
}
