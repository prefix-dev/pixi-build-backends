use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use fs_err as fs;
use miette::IntoDiagnostic;
use pixi_build_types::{BackendCapabilities, FrontendCapabilities};
use rattler_build::console_utils::LoggingOutputHandler;

pub struct RattlerBuildBackend {
    pub(crate) logging_output_handler: LoggingOutputHandler,
    /// In case of rattler-build, manifest is the raw recipe
    /// We need to apply later the selectors to get the final recipe
    pub(crate) raw_recipe: String,
    pub(crate) recipe_path: PathBuf,
    pub(crate) cache_dir: Option<PathBuf>,
}

impl RattlerBuildBackend {
    /// Returns a new instance of [`RattlerBuildBackend`] by reading the
    /// manifest at the given path.
    pub fn new(
        manifest_path: &Path,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
    ) -> miette::Result<Self> {
        // Locate the recipe
        let manifest_file_name = manifest_path.file_name().and_then(OsStr::to_str);
        let recipe_path = match manifest_file_name {
            Some("recipe.yaml") | Some("recipe.yml") => manifest_path.to_path_buf(),
            _ => {
                // The manifest is not a recipe, so we need to find the recipe.yaml file.
                let recipe_path = manifest_path.parent().and_then(|manifest_dir| {
                    [
                        "recipe.yaml",
                        "recipe.yml",
                        "recipe/recipe.yaml",
                        "recipe/recipe.yml",
                    ]
                    .into_iter()
                    .find_map(|relative_path| {
                        let recipe_path = manifest_dir.join(relative_path);
                        recipe_path.is_file().then_some(recipe_path)
                    })
                });

                recipe_path.ok_or_else(|| miette::miette!("Could not find a recipe.yaml in the source directory to use as the recipe manifest."))?
            }
        };

        // Load the manifest from the source directory
        let raw_recipe = fs::read_to_string(&recipe_path).into_diagnostic()?;

        Ok(Self {
            raw_recipe,
            recipe_path,
            logging_output_handler,
            cache_dir,
        })
    }

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

#[cfg(test)]
mod tests {
    use pixi_build_backend::protocol::{Protocol, ProtocolFactory};
    use pixi_build_types::{
        procedures::{
            conda_build::CondaBuildParams, conda_metadata::CondaMetadataParams,
            initialize::InitializeParams,
        },
        ChannelConfiguration,
    };
    use rattler_build::console_utils::LoggingOutputHandler;
    use std::path::Path;
    use std::{path::PathBuf, str::FromStr};
    use tempfile::tempdir;
    use url::Url;

    use crate::{protocol::RattlerBuildBackendFactory, rattler_build::RattlerBuildBackend};

    #[tokio::test]
    async fn test_get_conda_metadata() {
        // get cargo manifest dir
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = manifest_dir.join("../../recipe/recipe.yaml");

        let factory = RattlerBuildBackendFactory::new(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                manifest_path: recipe,
                project_model: None,
                configuration: None,
                cache_directory: None,
            })
            .await
            .unwrap();

        let current_dir = std::env::current_dir().unwrap();

        let result = factory
            .0
            .get_conda_metadata(CondaMetadataParams {
                host_platform: None,
                build_platform: None,
                channel_configuration: ChannelConfiguration {
                    base_url: Url::from_str("https://prefix.dev").unwrap(),
                },
                channel_base_urls: None,
                work_directory: current_dir,
                variant_configuration: None,
            })
            .await
            .unwrap();

        assert_eq!(result.packages.len(), 3);
    }

    #[tokio::test]
    async fn test_conda_build() {
        // get cargo manifest dir
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let recipe = manifest_dir.join("../../tests/recipe/boltons/recipe.yaml");

        let factory = RattlerBuildBackendFactory::new(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                manifest_path: recipe,
                project_model: None,
                configuration: None,
                cache_directory: None,
            })
            .await
            .unwrap();

        let current_dir = tempdir().unwrap();

        let result = factory
            .0
            .build_conda(CondaBuildParams {
                build_platform_virtual_packages: None,
                host_platform: None,
                channel_base_urls: None,
                channel_configuration: ChannelConfiguration {
                    base_url: Url::from_str("https://prefix.dev").unwrap(),
                },
                outputs: None,
                work_directory: current_dir.into_path(),
                variant_configuration: None,
                editable: false,
            })
            .await
            .unwrap();

        assert_eq!(result.packages[0].name, "boltons-with-extra");
    }

    const FAKE_RECIPE: &str = r#"
    package:
      name: foobar
      version: 0.1.0
    "#;

    async fn try_initialize(
        manifest_path: impl AsRef<Path>,
    ) -> miette::Result<RattlerBuildBackend> {
        RattlerBuildBackendFactory::new(LoggingOutputHandler::default())
            .initialize(InitializeParams {
                project_model: None,
                manifest_path: manifest_path.as_ref().to_path_buf(),
                configuration: None,
                cache_directory: None,
            })
            .await
            .map(|e| e.0)
    }

    #[tokio::test]
    async fn test_recipe_discovery() {
        let tmp = tempdir().unwrap();
        let recipe = tmp.path().join("recipe.yaml");
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_path,
            recipe
        );
        assert_eq!(try_initialize(&recipe).await.unwrap().recipe_path, recipe);

        let tmp = tempdir().unwrap();
        let recipe = tmp.path().join("recipe.yml");
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_path,
            recipe
        );
        assert_eq!(try_initialize(&recipe).await.unwrap().recipe_path, recipe);

        let tmp = tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        let recipe = recipe_dir.join("recipe.yaml");
        std::fs::create_dir(recipe_dir).unwrap();
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_path,
            recipe
        );

        let tmp = tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        let recipe = recipe_dir.join("recipe.yml");
        std::fs::create_dir(recipe_dir).unwrap();
        std::fs::write(&recipe, FAKE_RECIPE).unwrap();
        assert_eq!(
            try_initialize(&tmp.path().join("pixi.toml"))
                .await
                .unwrap()
                .recipe_path,
            recipe
        );
    }
}
