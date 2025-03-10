//! Collection of traits for package specifications and project models
//!
//! The main entry point is the [`ProjectModel`] trait which defines the core
//! interface for a project model.
//!
//! Any backend that will deal with Project (from pixi frontend as example) should implement this.
//!
#![deny(missing_docs)]

pub mod build_backend;
pub mod build_configuration;
pub mod capabilities;
pub mod package_spec;
pub mod project;
pub mod targets;
pub mod variants;
// pub mod build_context;

pub use build_backend::RequirementsProvider;
pub use build_configuration::BuildConfigurationProvider;
pub use package_spec::{AnyVersion, BinarySpecExt, PackageSpec};
pub use project::ProjectModel;
pub use targets::{Dependencies, TargetSelector, Targets};
pub use variants::VariantsProvider;
