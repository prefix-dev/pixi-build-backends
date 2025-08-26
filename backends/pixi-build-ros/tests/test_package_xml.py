from pathlib import Path
import tempfile

from pixi_build_ros.distro import Distro
from pixi_build_ros.ros_generator import ROSGenerator
from pixi_build_ros.utils import convert_package_xml_to_catkin_package, package_xml_to_conda_requirements
from pixi_build_backend.types.platform import Platform
from pixi_build_backend.types.project_model import ProjectModelV1

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

    assert requirements.build[0].concrete.package_name == "ros-noetic-ament-cmake"

def test_generate_recipe(package_xmls: Path):
    """Test the generate_recipe function of ROSGenerator."""
    # Create a temporary directory to simulate the package directory
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)
        
        # Copy the test package.xml to the temp directory
        package_xml_source = package_xmls / "demo_nodes_cpp.xml"
        package_xml_dest = temp_path / "package.xml"
        package_xml_dest.write_text(package_xml_source.read_text(encoding='utf-8'))
        
        # Create a minimal ProjectModelV1 instance
        model = ProjectModelV1()
        
        # Create config for ROS backend
        config = {
            "distro": "jazzy",
            "noarch": False
        }
        
        # Create host platform
        host_platform = Platform.current()
        
        # Create ROSGenerator instance
        generator = ROSGenerator()
        
        # Generate the recipe
        generated_recipe = generator.generate_recipe(
            model=model,
            config=config,
            manifest_path=str(temp_path),
            host_platform=host_platform
        )
        
        # Verify the generated recipe has expected properties
        assert generated_recipe.recipe.package.name.get_concrete() == "ros-jazzy-demo-nodes-cpp"
        assert generated_recipe.recipe.package.version.get_concrete() == "0.37.1"
        
        # Verify build script is generated
        assert generated_recipe.recipe.build.script is not None
        assert generated_recipe.recipe.build.script.content is not None

        # Verify ROS dependencies are included in build requirements
        build_deps = [dep.concrete.package_name for dep in generated_recipe.recipe.requirements.build if dep.concrete]
        expected_ros_deps = ["ros-jazzy-ament-cmake", "ros-jazzy-example-interfaces", "ros-jazzy-rclcpp"]
        
        for expected_dep in expected_ros_deps:
            assert expected_dep in build_deps, f"Expected dependency {expected_dep} not found in build deps: {build_deps}"
        
        # Verify standard build tools are included
        expected_build_tools = ["ninja", "python", "setuptools", "cmake"]
        for tool in expected_build_tools:
            assert tool in build_deps, f"Expected build tool {tool} not found in build deps: {build_deps}"
        
        # Verify run dependencies
        run_deps = [dep.concrete.package_name for dep in generated_recipe.recipe.requirements.run if dep.concrete]
        expected_run_deps = ["ros-jazzy-example-interfaces", "ros-jazzy-rclcpp", "ros-jazzy-launch-ros"]
        
        for expected_dep in expected_run_deps:
            assert expected_dep in run_deps, f"Expected runtime dependency {expected_dep} not found in run deps: {run_deps}"
