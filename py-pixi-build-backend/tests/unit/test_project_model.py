"""Unit tests for project_model.py module."""

from typing import Any
from pixi_build_backend.types.project_model import ProjectModelV1


def test_project_model_initialization(snapshot: Any) -> None:
    """Test initialization of ProjectModelV1."""
    model = ProjectModelV1(name="test_project", version="1.0.0")

    assert model._debug_str() == snapshot
