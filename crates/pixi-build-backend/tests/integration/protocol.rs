use std::path::PathBuf;

use crate::common::model::{convert_test_model_to_project_model_v1, load_project_model_from_json};
use fs_err::tokio::create_dir_all;
use pixi_build_backend::{
    generated_recipe::GeneratedRecipe,
    intermediate_backend::{IntermediateBackend, IntermediateBackendConfig},
    protocol::Protocol,
};
use pixi_build_types::{
    ChannelConfiguration, PlatformAndVirtualPackages,
    procedures::{conda_build::CondaBuildParams, conda_metadata::CondaMetadataParams},
};
use rattler_build::console_utils::LoggingOutputHandler;
use rattler_conda_types::Platform;
use url::Url;

#[tokio::test]
async fn test_project_model_into_recipe() {
    // Load a model from JSON
    let original_model = load_project_model_from_json("minimal_project_model.json");

    // Serialize it back to JSON
    let project_model_v1 = convert_test_model_to_project_model_v1(original_model);

    // Convert to IR
    let generated_recipe =
        GeneratedRecipe::from_model(project_model_v1, PathBuf::from("/path/to/manifest"));

    let platform = PlatformAndVirtualPackages {
        platform: Platform::current(),
        virtual_packages: None,
    };

    let channel_configuration = ChannelConfiguration {
        base_url: Url::parse("https://prefix.dev").unwrap(),
    };

    let params = CondaMetadataParams {
        build_platform: Some(platform.clone()),
        host_platform: Some(platform.clone()),
        channel_base_urls: Some(vec![]),
        channel_configuration,
        variant_configuration: None,
        work_directory: PathBuf::from("/path/to/workdir"),
    };

    let intermediate_backend = IntermediateBackend::new(
        PathBuf::from("/path/to/manifest"),
        generated_recipe,
        IntermediateBackendConfig::default(),
        LoggingOutputHandler::default(),
        None,
    )
    .unwrap();

    let conda_metadata = intermediate_backend
        .conda_get_metadata(params)
        .await
        .unwrap();

    insta::assert_yaml_snapshot!(conda_metadata)
}

#[tokio::test]
async fn test_conda_build() {
    // Load a model from JSON
    let original_model = load_project_model_from_json("minimal_project_model_for_build.json");

    // Serialize it back to JSON
    let project_model_v1 = convert_test_model_to_project_model_v1(original_model);

    // Convert to IR
    let mut generated_recipe =
        GeneratedRecipe::from_model(project_model_v1, PathBuf::from("/path/to/manifest"));

    generated_recipe.recipe.build.script = vec!["echo 'Hello, World!'".to_string()];

    let channel_configuration = ChannelConfiguration {
        base_url: Url::parse("https://prefix.dev/conda-forge").unwrap(),
    };

    let tmp_dir = tempfile::tempdir().unwrap();
    let tmp_dir_path = tmp_dir.path().to_path_buf();

    create_dir_all(tmp_dir_path.clone()).await.unwrap();

    let build_params = CondaBuildParams {
        build_platform_virtual_packages: None,
        host_platform: None,
        channel_base_urls: None,
        channel_configuration,
        outputs: None,
        variant_configuration: None,
        work_directory: tmp_dir_path.clone(),
        editable: false,
    };

    let intermediate_backend = IntermediateBackend::new(
        tmp_dir_path.clone(),
        generated_recipe,
        IntermediateBackendConfig::default(),
        LoggingOutputHandler::default(),
        None,
    )
    .unwrap();

    let conda_build_result = intermediate_backend
        .conda_build(build_params)
        .await
        .unwrap();

    insta::assert_yaml_snapshot!(conda_build_result);
}
