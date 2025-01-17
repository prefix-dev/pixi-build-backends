use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use miette::{Context, IntoDiagnostic};
use pixi_build_type_conversions::to_project_model_v1;
use pixi_build_types::{
    procedures::{
        conda_build::CondaBuildParams,
        conda_metadata::{CondaMetadataParams, CondaMetadataResult},
        initialize::InitializeParams,
        negotiate_capabilities::NegotiateCapabilitiesParams,
    },
    BackendCapabilities, ChannelConfiguration, FrontendCapabilities, PlatformAndVirtualPackages,
    ProjectModelV1,
};
use rattler_build::console_utils::{get_default_env_filter, LoggingOutputHandler};
use rattler_conda_types::{ChannelConfig, GenericVirtualPackage, Platform};
use rattler_virtual_packages::{VirtualPackage, VirtualPackageOverrides};
use tempfile::TempDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    consts,
    protocol::{Protocol, ProtocolFactory},
    server::Server,
};

#[allow(missing_docs)]
#[derive(Parser)]
pub struct App {
    /// The subcommand to run.
    #[clap(subcommand)]
    command: Option<Commands>,

    /// The port to expose the json-rpc server on. If not specified will
    /// communicate with stdin/stdout.
    #[clap(long)]
    http_port: Option<u16>,

    /// Enable verbose logging.
    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Get conda metadata for a recipe.
    GetCondaMetadata {
        #[clap(env, long, env = "PIXI_PROJECT_MANIFEST", default_value = consts::PROJECT_MANIFEST)]
        manifest_path: PathBuf,

        #[clap(long)]
        host_platform: Option<Platform>,
    },
    /// Build a conda package.
    CondaBuild {
        #[clap(env, long, env = "PIXI_PROJECT_MANIFEST", default_value = consts::PROJECT_MANIFEST)]
        manifest_path: PathBuf,
    },
    /// Get the capabilities of the backend.
    Capabilities,
}

/// Run the sever on the specified port or over stdin/stdout.
async fn run_server<T: ProtocolFactory>(port: Option<u16>, protocol: T) -> miette::Result<()> {
    let server = Server::new(protocol);
    if let Some(port) = port {
        server.run_over_http(port)
    } else {
        server.run().await
    }
}

/// Run the main CLI.
pub async fn main<T: ProtocolFactory, F: FnOnce(LoggingOutputHandler) -> T>(
    factory: F,
) -> miette::Result<()> {
    let args = App::parse();

    // Setup logging
    let log_handler = LoggingOutputHandler::default();
    let registry = tracing_subscriber::registry()
        .with(get_default_env_filter(args.verbose.log_level_filter()).into_diagnostic()?);
    registry.with(log_handler.clone()).init();

    let factory = factory(log_handler);

    match args.command {
        None => run_server(args.http_port, factory).await,
        Some(Commands::Capabilities) => {
            let backend_capabilities = capabilities::<T>().await?;
            eprintln!(
                "Supports conda metadata: {}",
                backend_capabilities
                    .provides_conda_metadata
                    .unwrap_or_default()
            );
            eprintln!(
                "Supports conda build: {}",
                backend_capabilities
                    .provides_conda_build
                    .unwrap_or_default()
            );
            eprintln!(
                "Highest project model: {}",
                backend_capabilities
                    .highest_supported_project_model
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| String::from("None"))
            );
            Ok(())
        }
        Some(Commands::CondaBuild { manifest_path }) => build(factory, &manifest_path).await,
        Some(Commands::GetCondaMetadata {
            manifest_path,
            host_platform,
        }) => {
            let metadata = get_conda_metadata(factory, &manifest_path, host_platform).await?;
            println!("{}", serde_yaml::to_string(&metadata).unwrap());
            Ok(())
        }
    }
}

/// Convert manifest to project model
fn project_model_v1(
    manifest_path: &Path,
    channel_config: &ChannelConfig,
) -> miette::Result<Option<ProjectModelV1>> {
    // Load the manifest
    let manifest = pixi_manifest::Manifest::from_path(manifest_path)?;
    let package = manifest.package;
    // This can be null in the rattler-build backend
    Ok(package.map(|manifest| {
        to_project_model_v1(&manifest, channel_config)
            .expect("failed to convert manifest to project model")
    }))
}

/// Negotiate the capabilities of the backend and initialize the backend.
async fn initialize<T: ProtocolFactory>(
    factory: T,
    manifest_path: &Path,
) -> miette::Result<T::Protocol> {
    // Negotiate the capabilities of the backend.
    let capabilities = capabilities::<T>().await?;
    let channel_config = ChannelConfig::default_with_root_dir(
        manifest_path
            .parent()
            .expect("manifest should always reside in a directory")
            .to_path_buf(),
    );
    let project_model = project_model_v1(manifest_path, &channel_config)?;

    // Check if the project model is required
    // and if it is not present, return an error.
    if capabilities.highest_supported_project_model.is_some() && project_model.is_none() {
        miette::bail!(
            "Could not extract 'project_model' from: {}, while it is required",
            manifest_path.display()
        );
    }

    // Initialize the backend
    let (protocol, _initialize_result) = factory
        .initialize(InitializeParams {
            manifest_path: manifest_path.to_path_buf(),
            project_model: project_model.map(Into::into),
            cache_directory: None,
            configuration: None,
        })
        .await?;
    Ok(protocol)
}

/// Frontend implementation for getting conda metadata.
async fn get_conda_metadata<T: ProtocolFactory>(
    factory: T,
    manifest_path: &Path,
    host_platform: Option<Platform>,
) -> miette::Result<CondaMetadataResult> {
    let channel_config = ChannelConfig::default_with_root_dir(
        manifest_path
            .parent()
            .expect("manifest should always reside in a directory")
            .to_path_buf(),
    );

    let protocol = initialize(factory, manifest_path).await?;
    let virtual_packages: Vec<_> = VirtualPackage::detect(&VirtualPackageOverrides::from_env())
        .into_diagnostic()?
        .into_iter()
        .map(GenericVirtualPackage::from)
        .collect();

    let tempdir = TempDir::new_in(".")
        .into_diagnostic()
        .context("failed to create a temporary directory in the current directory")?;

    protocol
        .get_conda_metadata(CondaMetadataParams {
            build_platform: None,
            host_platform: host_platform.map(|platform| PlatformAndVirtualPackages {
                platform,
                virtual_packages: Some(virtual_packages.clone()),
            }),
            channel_base_urls: None,
            channel_configuration: ChannelConfiguration {
                base_url: channel_config.channel_alias,
            },
            work_directory: tempdir.path().to_path_buf(),
            variant_configuration: None,
        })
        .await
}

/// Returns the capabilities of the backend.
async fn capabilities<Factory: ProtocolFactory>() -> miette::Result<BackendCapabilities> {
    let result = Factory::negotiate_capabilities(NegotiateCapabilitiesParams {
        capabilities: FrontendCapabilities {},
    })
    .await?;

    Ok(result.capabilities)
}

/// Frontend implementation for building a conda package.
async fn build<T: ProtocolFactory>(factory: T, manifest_path: &Path) -> miette::Result<()> {
    let channel_config = ChannelConfig::default_with_root_dir(
        manifest_path
            .parent()
            .expect("manifest should always reside in a directory")
            .to_path_buf(),
    );

    let protocol = initialize(factory, manifest_path).await?;
    let work_dir = TempDir::new_in(".")
        .into_diagnostic()
        .context("failed to create a temporary directory in the current directory")?;

    let result = protocol
        .build_conda(CondaBuildParams {
            host_platform: None,
            build_platform_virtual_packages: None,
            channel_base_urls: None,
            channel_configuration: ChannelConfiguration {
                base_url: channel_config.channel_alias,
            },
            outputs: None,
            work_directory: work_dir.path().to_path_buf(),
            variant_configuration: None,
            editable: false,
        })
        .await?;

    for package in result.packages {
        eprintln!("Successfully build '{}'", package.output_file.display());
        eprintln!("Use following globs to revalidate: ");
        for glob in package.input_globs {
            eprintln!("  - {}", glob);
        }
    }

    Ok(())
}
