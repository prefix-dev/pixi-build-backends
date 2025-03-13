//! Common utilities that are shared between the different build backends.
mod configuration;
mod requirements;
mod variants;

pub use configuration::{build_configuration, BuildConfigurationParams};
pub use requirements::requirements;
pub use variants::compute_variants;
