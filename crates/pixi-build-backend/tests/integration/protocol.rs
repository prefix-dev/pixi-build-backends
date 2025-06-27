use crate::common::model::{convert_test_model_to_project_model_v1, load_project_model_from_json};
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
use tempfile::TempDir;
use url::Url;

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

    // Convert to IR
    let generated_recipe = GeneratedRecipe::from_model(project_model_v1, pixi_manifest.clone());

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

    let intermediate_backend = IntermediateBackend::new(
        pixi_manifest.clone(),
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

    // Convert to IR
    let mut generated_recipe = GeneratedRecipe::from_model(project_model_v1, pixi_manifest.clone());

    generated_recipe.recipe.build.script = vec!["echo 'Hello, World!'".to_string()];

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

    let intermediate_backend = IntermediateBackend::new(
        pixi_manifest.clone(),
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

    insta::assert_yaml_snapshot!(conda_build_result, {
        ".packages[0].output_file" => "[redacted]",
        ".packages[0].subdir" => "[redacted]",
    });
}
