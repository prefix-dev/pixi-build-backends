use pixi_build_types::procedures::{
    conda_build::{CondaBuildParams, CondaBuildResult},
    conda_metadata::{CondaMetadataParams, CondaMetadataResult},
    initialize::{InitializeParams, InitializeResult},
    negotiate_capabilities::{NegotiateCapabilitiesParams, NegotiateCapabilitiesResult},
};

/// A trait that is used to instantiate a new protocol connection
/// and endpoint that can handle the RPC calls.
#[async_trait::async_trait]
pub trait ProtocolInstantiator: Send + Sync + 'static {
    /// The endpoint implements the protocol RPC methods
    type ProtocolEndpoint: Protocol + Send + Sync + 'static;

    /// Called when negotiating capabilities with the client.
    /// This is determine how the rest of the initialization will proceed.
    async fn negotiate_capabilities(
        params: NegotiateCapabilitiesParams,
    ) -> miette::Result<NegotiateCapabilitiesResult>;

    /// Called when the client requests initialization.
    /// Returns the protocol endpoint and the result of the initialization.
    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> miette::Result<(Self::ProtocolEndpoint, InitializeResult)>;
}

/// A trait that defines the protocol for a pixi build backend.
/// These are implemented by the different backends. Which
/// server as an endpoint for the RPC calls.
#[async_trait::async_trait]
pub trait Protocol {
    /// Called when the client requests metadata for a Conda package.
    async fn conda_get_metadata(
        &self,
        _params: CondaMetadataParams,
    ) -> miette::Result<CondaMetadataResult> {
        unimplemented!("conda_get_metadata not implemented");
    }

    /// Called when the client requests to build a Conda package.
    async fn conda_build(&self, _params: CondaBuildParams) -> miette::Result<CondaBuildResult> {
        unimplemented!("conda_build not implemented");
    }
}
