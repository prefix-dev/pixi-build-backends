"""Unit tests for platform.py module."""

import pytest
from unittest.mock import Mock, patch
from pixi_build_backend.types.platform import Platform


def test_current_class_method():
    """Test creation of Platform from string and its underlying magic methods."""
    result = Platform("linux-64")

    assert str(result) == "linux-64"
    assert result.is_linux
