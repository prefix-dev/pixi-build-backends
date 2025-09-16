from pathlib import Path

from pixi_build_ros.utils import load_package_map_data


def test_package_loading(test_data_dir: Path):
    """Load the package map with overwrites."""
    robostack_file = Path(__file__).parent.parent / "robostack.yaml"
    other_package_map = test_data_dir / "other_package_map.yaml"
    result = load_package_map_data([robostack_file, other_package_map])
    assert "new_package" in result
    assert result["new_package"]["robostack"] == ["new-package"], "Should be added"
    assert result["alsa-oss"]["robostack"] == ["other-alsa-oss"], "Should be overwritten"
    assert "zlib" in result, "Should still be present"
