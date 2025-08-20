from pathlib import Path

import pytest


@pytest.fixture
def test_data_dir() -> Path:
    """Fixture to provide the path to the test data directory."""
    return Path(__file__).parent / 'data'

@pytest.fixture
def package_xmls(test_data_dir) -> Path:
    """Fixture to read the package.xml content from the test data directory."""
    return test_data_dir / 'package_xmls'

