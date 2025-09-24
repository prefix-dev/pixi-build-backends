from pathlib import Path

import pytest

from pixi_build_ros.utils import (
    PackageMapEntry,
    load_package_map_data,
    PackageMappingSource,
)


@pytest.fixture
def test_data_dir() -> Path:
    """Fixture to provide the path to the test data directory."""
    return Path(__file__).parent / "data"


@pytest.fixture
def package_xmls(test_data_dir) -> Path:
    """Fixture to read the package.xml content from the test data directory."""
    return test_data_dir / "package_xmls"


@pytest.fixture
def package_map() -> dict[str, PackageMapEntry]:
    """Load the package map"""
    robostack_file = Path(__file__).parent.parent / "robostack.yaml"
    return load_package_map_data([PackageMappingSource.from_file(robostack_file)])
