use std::{net::SocketAddr, sync::Arc};

use jsonrpc_core::{serde_json, to_value, Error, IoHandler, Params};
use miette::{IntoDiagnostic, JSONReportHandler};
use pixi_build_types::procedures::{
    self, conda_build::CondaBuildParams, conda_metadata::CondaMetadataParams,
    initialize::InitializeParams, negotiate_capabilities::NegotiateCapabilitiesParams,
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

                    let (protocol_endpoint, result) = initializer
                        .initialize(params)
                        .await
                        .map_err(convert_error)?;
                    *state = ServerState::Initialized(protocol_endpoint);

                    Ok(to_value(result).expect("failed to convert to json"))
                }
            },
        );

        let conda_get_metadata = state.clone();
        io.add_method(
            procedures::conda_metadata::METHOD_NAME,
            move |params: Params| {
                let state = conda_get_metadata.clone();

                async move {
                    let params: CondaMetadataParams = params.parse()?;
                    let state = state.read().await;
                    state
                        .as_endpoint()?
                        .conda_get_metadata(params)
                        .await
                        .map(|value| to_value(value).expect("failed to convert to json"))
                        .map_err(convert_error)
                }
            },
        );

        let conda_build = state.clone();
        io.add_method(
            procedures::conda_build::METHOD_NAME,
            move |params: Params| {
                let state = conda_build.clone();

                async move {
                    let params: CondaBuildParams = params.parse()?;
                    let state = state.read().await;
                    state
                        .as_endpoint()?
                        .conda_build(params)
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
