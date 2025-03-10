//! Traits for providing capabilities of a backend based on input params
//!
//!

use pixi_build_types::{
    procedures::negotiate_capabilities::{
        NegotiateCapabilitiesParams, NegotiateCapabilitiesResult,
    },
    BackendCapabilities,
};

/// The trait to provide capabilities of a backend
pub trait CapabilitiesProvider {
    /// Returns the capabilities for a recipe
    fn default_capabilities(_params: &NegotiateCapabilitiesParams) -> BackendCapabilities {
        BackendCapabilities {
            provides_conda_metadata: Some(true),
            provides_conda_build: Some(true),
            highest_supported_project_model: Some(
                pixi_build_types::VersionedProjectModel::highest_version(),
            ),
        }
    }

    /// Adjust default capabilities based on the backend
    fn backend_capabilities(
        _params: &NegotiateCapabilitiesParams,
        backend: BackendCapabilities,
    ) -> miette::Result<BackendCapabilities>;

    /// Returns the capabilities for a recipe
    fn capabilities(
        params: &NegotiateCapabilitiesParams,
    ) -> miette::Result<NegotiateCapabilitiesResult> {
        let default_capabilities = Self::default_capabilities(params);
        let capabilities = Self::backend_capabilities(params, default_capabilities)?;
        Ok(NegotiateCapabilitiesResult { capabilities })
    }
}
