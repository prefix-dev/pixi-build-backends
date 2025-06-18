use std::path::PathBuf;

use crate::common::model::{convert_test_model_to_project_model_v1, load_project_model_from_json};
use recipe_stage0::recipe::IntermediateRecipe;

#[test]
fn test_project_model_into_recipe() {
    // Load a model from JSON
    let original_model = load_project_model_from_json("minimal_project_model.json");

    // Serialize it back to JSON
    let project_model_v1 = convert_test_model_to_project_model_v1(original_model);

    // Convert to IR
    let ir = IntermediateRecipe::from_model(project_model_v1, PathBuf::from("/path/to/manifest"));

    insta::assert_yaml_snapshot!(ir)
}
