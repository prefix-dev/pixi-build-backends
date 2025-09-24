import pytest
from pixi_build_backend.types.item import ItemPackageDependency
from pixi_build_ros.ros_generator import merge_unique_items


def test_with_star_items():
    list1 = [ItemPackageDependency("ros-noetic *")]
    list2 = [ItemPackageDependency("ros-noetic <=2.0")]
    for res in merge_unique_items(list1, list2), merge_unique_items(list2, list1):
        assert len(res) == 1
        assert res[0].concrete.binary_spec == "ros-noetic <=2.0"


def test_equal_items():
    list1 = [ItemPackageDependency("ros-noetic ==2.0")]
    list2 = [ItemPackageDependency("ros-noetic <=2.0")]
    for res in merge_unique_items(list1, list2), merge_unique_items(list2, list1):
        assert len(res) == 1
        assert res[0].concrete.binary_spec == "ros-noetic ==2.0"


def test_multiple_specs_items():
    list1 = [ItemPackageDependency("ros-noetic >=2.0")]
    list2 = [ItemPackageDependency("ros-noetic <=2.0,<3.0")]
    res = merge_unique_items(list1, list2)
    assert len(res) == 1
    assert res[0].concrete.binary_spec == "ros-noetic >=2.0,<=2.0,<3.0"


def test_different_specs_items():
    list1 = [ItemPackageDependency("ros-noetic >=2.0")]
    list2 = [ItemPackageDependency("ros-noetic2 <=2.0,<3.0")]
    res = merge_unique_items(list1, list2)
    assert len(res) == 2


def test_specs_with_spaces():
    list1 = [ItemPackageDependency("ros-noetic 2.* noetic")]
    list2 = [ItemPackageDependency("ros-noetic <=2.0,<3.0")]
    with pytest.raises(ValueError) as exc:
        merge_unique_items(list1, list2)
    assert "contains spaces" in str(exc)
