from typing import Any
from pixi_build_backend.types.generated_recipe import GeneratedRecipe
from pixi_build_backend.types.project_model import ProjectModelV1


def test_generated_recipe_from_model(snapshot: Any) -> None:
    """Test initialization of ProjectModelV1."""
    model = ProjectModelV1(name="test_project", version="1.0.0")

    generated_recipe = GeneratedRecipe.from_model(model)

    print(type(generated_recipe.recipe))

    assert snapshot == generated_recipe.recipe.to_yaml()
