use crate::common::model::{convert_test_model_to_project_model_v1, load_project_model_from_json};
use imp::TestGenerateRecipe;
use pixi_build_backend::{intermediate_backend::IntermediateBackend, protocol::Protocol};
use pixi_build_types::{
    ChannelConfiguration, PlatformAndVirtualPackages,
    procedures::{conda_build::CondaBuildParams, conda_metadata::CondaMetadataParams},
};
use rattler_build::console_utils::LoggingOutputHandler;
use rattler_conda_types::Platform;
use serde_json::json;
use tempfile::TempDir;
use url::Url;

#[cfg(test)]
mod imp {
    use std::path::{Path, PathBuf};

    use pixi_build_backend::generated_recipe::{
        BackendConfig, GenerateRecipe, GeneratedRecipe, PythonParams,
    };
    use serde::Deserialize;

    #[derive(Debug, Default, Deserialize, Clone)]
    #[serde(rename_all = "kebab-case")]
    pub struct TestBackendConfig {
        /// If set, internal state will be logged as files in that directory
        pub debug_dir: Option<PathBuf>,
    }

    #[cfg(test)]
    #[derive(Clone, Default)]
    pub(crate) struct TestGenerateRecipe {}

    impl BackendConfig for TestBackendConfig {
        fn debug_dir(&self) -> Option<&Path> {
            self.debug_dir.as_deref()
        }
    }

    impl GenerateRecipe for TestGenerateRecipe {
        type Config = TestBackendConfig;

        fn generate_recipe(
            &self,
            model: &pixi_build_types::ProjectModelV1,
            _config: &Self::Config,
            manifest_path: PathBuf,
            _host_platform: rattler_conda_types::Platform,
            _python_params: Option<PythonParams>,
        ) -> miette::Result<GeneratedRecipe> {
            let generated_recipe = GeneratedRecipe::from_model(model.clone(), manifest_path);
            Ok(generated_recipe)
        }
    }
}

#[tokio::test]
async fn test_conda_get_metadata() {
    let tmp_dir = TempDir::new().unwrap();
    let tmp_dir_path = tmp_dir.path().to_path_buf();

    let pixi_manifest = tmp_dir_path.join("pixi.toml");

    let build_dir = tmp_dir_path.join("build");

    // Load a model from JSON
    let original_model = load_project_model_from_json("minimal_project_model.json");

    // Serialize it back to JSON
    let project_model_v1 = convert_test_model_to_project_model_v1(original_model);

    // save the pixi.toml file to a temporary location
    fs_err::write(&pixi_manifest, toml::to_string(&project_model_v1).unwrap()).unwrap();

    let platform = PlatformAndVirtualPackages {
        platform: Platform::Linux64,
        virtual_packages: None,
    };

    let channel_configuration = ChannelConfiguration {
        base_url: Url::parse("https://prefix.dev").unwrap(),
    };

    let channel_base_urls = vec![Url::parse("https://prefix.dev/conda-forge").unwrap()];

    let params = CondaMetadataParams {
        build_platform: Some(platform.clone()),
        host_platform: Some(platform.clone()),
        channel_base_urls: Some(channel_base_urls),
        channel_configuration,
        variant_configuration: None,
        work_directory: build_dir,
    };

    let some_config = json!({
        "debug-dir": "some_debug_dir",
    });

    let intermediate_backend = IntermediateBackend::<TestGenerateRecipe>::new(
        pixi_manifest.clone(),
        project_model_v1,
        TestGenerateRecipe::default(),
        some_config,
        LoggingOutputHandler::default(),
        None,
    )
    .unwrap();

    let conda_metadata = intermediate_backend
        .conda_get_metadata(params)
        .await
        .unwrap();

    insta::assert_yaml_snapshot!(conda_metadata, {
        ".packages[0].sources.boltons.path" => "[redacted]",
        ".packages[0].subdir" => "[redacted]",
    })
}

#[tokio::test]
async fn test_conda_build() {
    let tmp_dir = TempDir::new().unwrap();
    let tmp_dir_path = tmp_dir.path().to_path_buf();

    let pixi_manifest = tmp_dir_path.join("pixi.toml");
    let build_dir = tmp_dir_path.join("build");

    // Load a model from JSON
    let original_model = load_project_model_from_json("minimal_project_model_for_build.json");

    // Serialize it back to JSON
    let project_model_v1 = convert_test_model_to_project_model_v1(original_model);

    // save the pixi.toml file to a temporary location
    fs_err::write(&pixi_manifest, toml::to_string(&project_model_v1).unwrap()).unwrap();

    let channel_configuration = ChannelConfiguration {
        base_url: Url::parse("https://prefix.dev").unwrap(),
    };

    let channel_base_urls = vec![Url::parse("https://prefix.dev/conda-forge").unwrap()];

    let build_params = CondaBuildParams {
        build_platform_virtual_packages: None,
        host_platform: None,
        channel_base_urls: Some(channel_base_urls),
        channel_configuration,
        outputs: None,
        variant_configuration: None,
        work_directory: build_dir.clone(),
        editable: false,
    };

    let some_config = json!({
        "debug-dir": "some_debug_dir",
    });

    let intermediate_backend = IntermediateBackend::new(
        pixi_manifest.clone(),
        project_model_v1,
        TestGenerateRecipe::default(),
        some_config,
        LoggingOutputHandler::default(),
        None,
    )
    .unwrap();

    let conda_build_result = intermediate_backend
        .conda_build(build_params)
        .await
        .unwrap();

    insta::assert_yaml_snapshot!(conda_build_result, {
        ".packages[0].output_file" => "[redacted]",
        ".packages[0].subdir" => "[redacted]",
    });
}
