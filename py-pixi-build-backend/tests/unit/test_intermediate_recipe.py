from pathlib import Path
from typing import Any
from pixi_build_backend.types.intermediate_recipe import IntermediateRecipe


def test_from_yaml(snapshot: Any) -> None:
    yaml_file = Path(__file__).parent.parent / "data" / "boltons_recipe.yaml"
    yaml_content = yaml_file.read_text()

    recipe = IntermediateRecipe.from_yaml(yaml_content)

    assert snapshot == recipe.to_yaml()
