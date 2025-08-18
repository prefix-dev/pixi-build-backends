from pathlib import Path

from pixi_build_ros.distro import Distro
from pixi_build_ros.utils import convert_package_xml_to_catkin_package, package_xml_to_conda_requirements

def test_package_xml_to_recipe_config(package_xmls: Path):
    # Read content from the file in the test data directory
    package_xml_path = package_xmls / "demo_nodes_cpp.xml"
    package_content = package_xml_path.read_text(encoding='utf-8')
    package = convert_package_xml_to_catkin_package(package_content)

    distro = Distro("jazzy")
    requirements = package_xml_to_conda_requirements(package, distro)

    # Build
    expected_build_packages = [
        "example-interfaces", "rcl", "rclcpp", "rclcpp-components",
        "rcl-interfaces", "rcpputils", "rcutils", "rmw", "std-msgs"
    ]
    build_names = [pkg.concrete.package_name for pkg in requirements.build]
    print(f"{requirements.build[0].concrete.package_name}")
    for pkg in expected_build_packages:
        assert f"ros-{distro.name}-{pkg}" in build_names

    # TODO: Check the host packages when we figure out how to handle them

    # Run
    expected_run_packages = [
        "example-interfaces", "launch-ros", "launch-xml", "rcl", "rclcpp",
        "rclcpp-components", "rcl-interfaces", "rcpputils", "rcutils", "rmw", "std-msgs"
    ]
    run_names = [pkg.concrete.package_name for pkg in requirements.run]
    for pkg in expected_run_packages:
        assert f"ros-{distro.name}-{pkg}" in run_names


def test_ament_cmake_package_xml_to_recipe_config(package_xmls: Path):
    # Read content from the file in the test data directory
    package_xml_path = package_xmls / "demos_action_tutorials_interfaces.xml"
    package_content = package_xml_path.read_text(encoding='utf-8')
    package = convert_package_xml_to_catkin_package(package_content)

    distro = Distro("noetic")
    requirements = package_xml_to_conda_requirements(package, distro)

