"""Unit tests for project_model.py module."""

import pytest
from unittest.mock import Mock
from pixi_build_backend.types.project_model import ProjectModelV1


def test_project_model_initialization(snapshot):
    """Test initialization of ProjectModelV1."""
    model = ProjectModelV1(name="test_project", version="1.0.0")

    assert model._debug_str() == snapshot
