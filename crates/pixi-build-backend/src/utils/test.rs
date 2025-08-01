use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use pixi_build_types::procedures::{
    conda_outputs::{CondaOutputsParams, CondaOutputsResult},
    initialize::InitializeParams,
};
use rattler_build::console_utils::LoggingOutputHandler;
use rattler_conda_types::Platform;
use serde_json::Value;

use crate::{
    generated_recipe::GenerateRecipe, intermediate_backend::IntermediateBackendInstantiator,
    protocol::ProtocolInstantiator,
};

/// A utility function to remove empty values from a JSON object.
pub(crate) fn remove_empty_values(value: &mut Value) {
    fn keep_value(value: &Value) -> bool {
        match value {
            Value::Object(map) => !map.is_empty(),
            Value::Array(arr) => !arr.is_empty(),
            Value::Null => false,
            _ => true,
        }
    }

    match value {
        Value::Object(map) => {
            map.retain(|_, v| {
                remove_empty_values(v);
                keep_value(v)
            });
        }
        Value::Array(arr) => {
            arr.retain_mut(|v| {
                remove_empty_values(v);
                keep_value(v)
            });
        }
        _ => {}
    }
}

/// Calls the `conda/outputs` procedure of the `IntermediateBackend` for the
/// given recipe general and with the given project model.
///
/// Returns a pretty-printed JSON string of the outputs that can be used for
/// snapshots
pub fn intermediate_conda_outputs_snapshot<T>(
    project_model: Option<pixi_build_types::ProjectModelV1>,
    source_dir: Option<PathBuf>,
    host_platform: Platform,
    variant_configuration: Option<BTreeMap<String, Vec<String>>>,
) -> String
where
    T: GenerateRecipe + Default + Clone + Send + Sync + 'static,
    <T as GenerateRecipe>::Config: Send + Sync + 'static,
{
    let manifest_path = match &source_dir {
        Some(dir) => dir.join("pixi.toml"),
        None => PathBuf::from("pixi.toml"),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async move {
        let (protocol, _result) = IntermediateBackendInstantiator::<T>::new(
            LoggingOutputHandler::default(),
            Arc::new(T::default()),
        )
        .initialize(InitializeParams {
            source_dir,
            manifest_path,
            project_model: project_model.map(Into::into),
            configuration: None,
            target_configuration: None,
            cache_directory: None,
        })
        .await
        .unwrap();

        let current_dir = std::env::current_dir().unwrap();
        let result = protocol
            .conda_outputs(CondaOutputsParams {
                channels: vec![],
                host_platform,
                build_platform: host_platform,
                variant_configuration,
                work_directory: current_dir,
            })
            .await
            .unwrap();

        conda_outputs_snapshot(result)
    })
}

/// A function to convert a `CondaOutputsResult` into a pretty-printed JSON
/// string.
pub fn conda_outputs_snapshot(result: CondaOutputsResult) -> String {
    let mut value = serde_json::to_value(result).unwrap();
    remove_empty_values(&mut value);
    serde_json::to_string_pretty(&value).unwrap()
}
