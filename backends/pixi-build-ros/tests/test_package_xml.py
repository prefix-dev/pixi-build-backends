import json
import shutil
import tempfile
import tomllib
from pathlib import Path
from typing import Dict

from pixi_build_ros.distro import Distro
from pixi_build_ros.ros_generator import ROSGenerator
from pixi_build_ros.utils import (
    convert_package_xml_to_catkin_package,
    package_xml_to_conda_requirements,
    rosdep_to_conda_package_name,
    PackageMapEntry,
)
from pixi_build_backend.types.platform import Platform
from pixi_build_backend.types.project_model import ProjectModelV1


def test_package_xml_to_recipe_config(package_xmls: Path, package_map: Dict[str, PackageMapEntry]):
    # Read content from the file in the test data directory
    package_xml_path = package_xmls / "demo_nodes_cpp.xml"
    package_content = package_xml_path.read_text(encoding="utf-8")
    package = convert_package_xml_to_catkin_package(package_content)

    distro = Distro("jazzy")
    requirements = package_xml_to_conda_requirements(package, distro, Platform.current(), package_map)

    # Build
    expected_build_packages = [
        "example-interfaces",
        "rcl",
        "rclcpp",
        "rclcpp-components",
        "rcl-interfaces",
        "rcpputils",
        "rcutils",
        "rmw",
        "std-msgs",
    ]
    build_names = [pkg.concrete.package_name for pkg in requirements.build]
    print(f"{requirements.build[0].concrete.package_name}")
    for pkg in expected_build_packages:
        assert f"ros-{distro.name}-{pkg}" in build_names

    # TODO: Check the host packages when we figure out how to handle them

    # Run
    expected_run_packages = [
        "example-interfaces",
        "launch-ros",
        "launch-xml",
        "rcl",
        "rclcpp",
        "rclcpp-components",
        "rcl-interfaces",
        "rcpputils",
        "rcutils",
        "rmw",
        "std-msgs",
    ]
    run_names = [pkg.concrete.package_name for pkg in requirements.run]
    for pkg in expected_run_packages:
        assert f"ros-{distro.name}-{pkg}" in run_names


def test_ament_cmake_package_xml_to_recipe_config(package_xmls: Path, package_map: Dict[str, PackageMapEntry]):
    # Read content from the file in the test data directory
    package_xml_path = package_xmls / "demos_action_tutorials_interfaces.xml"
    package_content = package_xml_path.read_text(encoding="utf-8")
    package = convert_package_xml_to_catkin_package(package_content)

    distro = Distro("noetic")
    requirements = package_xml_to_conda_requirements(package, distro, Platform.current(), package_map)

    assert requirements.build[0].concrete.package_name == "ros-noetic-ament-cmake"


def test_generate_recipe(package_xmls: Path):
    """Test the generate_recipe function of ROSGenerator."""
    # Create a temporary directory to simulate the package directory
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)

        # Copy the test package.xml to the temp directory
        package_xml_source = package_xmls / "demo_nodes_cpp.xml"
        package_xml_dest = temp_path / "package.xml"
        package_xml_dest.write_text(package_xml_source.read_text(encoding="utf-8"))

        # Create a minimal ProjectModelV1 instance
        model = ProjectModelV1()

        # Create config for ROS backend
        config = {"distro": "jazzy", "noarch": False}

        # Create host platform
        host_platform = Platform.current()

        # Create ROSGenerator instance
        generator = ROSGenerator()

        # Generate the recipe
        generated_recipe = generator.generate_recipe(
            model=model,
            config=config,
            manifest_path=str(temp_path),
            host_platform=host_platform,
        )

        # Verify the generated recipe has expected properties
        assert generated_recipe.recipe.package.name.get_concrete() == "ros-jazzy-demo-nodes-cpp"
        assert generated_recipe.recipe.package.version.get_concrete() == "0.37.1"

        # Verify build script is generated
        assert generated_recipe.recipe.build.script is not None
        assert generated_recipe.recipe.build.script.content is not None

        # Verify ROS dependencies are included in build requirements
        build_deps = [dep.concrete.package_name for dep in generated_recipe.recipe.requirements.build if dep.concrete]
        expected_ros_deps = [
            "ros-jazzy-ament-cmake",
            "ros-jazzy-example-interfaces",
            "ros-jazzy-rclcpp",
        ]

        for expected_dep in expected_ros_deps:
            assert expected_dep in build_deps, (
                f"Expected dependency {expected_dep} not found in build deps: {build_deps}"
            )

        # Verify standard build tools are included
        expected_build_tools = ["ninja", "python", "setuptools", "cmake"]
        for tool in expected_build_tools:
            assert tool in build_deps, f"Expected build tool {tool} not found in build deps: {build_deps}"

        # Verify run dependencies
        run_deps = [dep.concrete.package_name for dep in generated_recipe.recipe.requirements.run if dep.concrete]
        expected_run_deps = [
            "ros-jazzy-example-interfaces",
            "ros-jazzy-rclcpp",
            "ros-jazzy-launch-ros",
        ]

        for expected_dep in expected_run_deps:
            assert expected_dep in run_deps, (
                f"Expected runtime dependency {expected_dep} not found in run deps: {run_deps}"
            )


def test_recipe_includes_project_run_dependency(package_xmls: Path):
    """Ensure dependencies declared in project manifest merge into run requirements."""

    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)

        package_xml_source = package_xmls / "custom_ros.xml"
        package_xml_dest = temp_path / "package.xml"
        package_xml_dest.write_text(package_xml_source.read_text(encoding="utf-8"))

        model_payload = {
            "name": "custom_ros",
            "version": "0.0.1",
            "description": "Demo",
            "authors": ["Tester the Tester"],
            "targets": {
                "defaultTarget": {
                    "hostDependencies": {},
                    "buildDependencies": {},
                    "runDependencies": {
                        "rich": {
                            "binary": {
                                "version": ">=10.0"
                            }
                        }
                    },
                },
                "targets": {},
            },
        }
        model = ProjectModelV1.from_json(json.dumps(model_payload))

        config = {"distro": "noetic", "noarch": False}
        host_platform = Platform.current()
        generator = ROSGenerator()

        generated_recipe = generator.generate_recipe(
            model=model,
            config=config,
            manifest_path=str(temp_path),
            host_platform=host_platform,
        )

        run_requirements = [str(dep) for dep in generated_recipe.recipe.requirements.run]

        assert any("rich" in dep for dep in run_requirements), (
            f"Expected pixi run dependency 'rich' missing from recipe run requirements"
        )


def test_robostack_target_platform_linux(package_map: Dict[str, PackageMapEntry]):
    """Test that target platform correctly selects Linux packages from robostack.yaml."""
    distro = Distro("jazzy")

    # Create a mock Linux platform
    linux_platform = Platform("linux-64")

    # Test packages with platform-specific mappings
    acl_packages = rosdep_to_conda_package_name("acl", distro, linux_platform, package_map)
    assert acl_packages == ["libacl"], f"Expected ['libacl'] for acl on Linux, got {acl_packages}"


def test_robostack_target_platform_osx(package_map: Dict[str, PackageMapEntry]):
    """Test that target platform correctly selects macOS packages from robostack.yaml."""
    distro = Distro("jazzy")

    # Create a mock macOS platform
    osx_platform = Platform("osx-64")

    # Test packages with platform-specific mappings
    acl_packages = rosdep_to_conda_package_name("acl", distro, osx_platform, package_map)
    assert acl_packages == [], f"Expected [] for acl on macOS, got {acl_packages}"


def test_robostack_target_platform_windows(package_map: Dict[str, PackageMapEntry]):
    """Test that target platform correctly selects Windows packages from robostack.yaml."""
    distro = Distro("jazzy")

    # Create a mock Windows platform
    win_platform = Platform("win-64")

    # Test packages with platform-specific mappings
    binutils_packages = rosdep_to_conda_package_name("binutils", distro, win_platform, package_map)
    assert binutils_packages == [], f"Expected [] for binutils on Windows, got {binutils_packages}"


def test_robostack_target_platform_cross_platform(
    package_map: Dict[str, PackageMapEntry],
):
    """Test packages that have different mappings across all platforms."""
    distro = Distro("jazzy")

    linux_platform = Platform("linux-64")
    osx_platform = Platform("osx-64")
    win_platform = Platform("win-64")

    # libudev-dev has different packages for each platform
    linux_udev = rosdep_to_conda_package_name("libudev-dev", distro, linux_platform, package_map)
    osx_udev = rosdep_to_conda_package_name("libudev-dev", distro, osx_platform, package_map)
    win_udev = rosdep_to_conda_package_name("libudev-dev", distro, win_platform, package_map)

    assert linux_udev == [
        "libusb",
        "libudev",
    ], f"Expected ['libusb', 'libudev'] for libudev-dev on Linux, got {linux_udev}"
    assert osx_udev == ["libusb"], f"Expected ['libusb'] for libudev-dev on macOS, got {osx_udev}"
    assert win_udev == ["libusb"], f"Expected ['libusb'] for libudev-dev on Windows, got {win_udev}"

    # libomp-dev has different OpenMP implementations per platform
    linux_omp = rosdep_to_conda_package_name("libomp-dev", distro, linux_platform, package_map)
    osx_omp = rosdep_to_conda_package_name("libomp-dev", distro, osx_platform, package_map)
    win_omp = rosdep_to_conda_package_name("libomp-dev", distro, win_platform, package_map)

    assert linux_omp == ["libgomp"], f"Expected ['libgomp'] for libomp-dev on Linux, got {linux_omp}"
    assert osx_omp == ["llvm-openmp"], f"Expected ['llvm-openmp'] for libomp-dev on macOS, got {osx_omp}"
    assert win_omp == [], f"Expected [] for libomp-dev on Windows, got {win_omp}"


def test_robostack_require_opengl_handling(package_map: Dict[str, PackageMapEntry]):
    """Test that REQUIRE_OPENGL is correctly handled for different platforms."""
    distro = Distro("jazzy")

    linux_platform = Platform("linux-64")
    osx_platform = Platform("osx-64")
    win_platform = Platform("win-64")

    # opengl package has REQUIRE_OPENGL handling
    linux_opengl = rosdep_to_conda_package_name("opengl", distro, linux_platform, package_map)
    osx_opengl = rosdep_to_conda_package_name("opengl", distro, osx_platform, package_map)
    win_opengl = rosdep_to_conda_package_name("opengl", distro, win_platform, package_map)

    # According to the code, REQUIRE_OPENGL should be replaced with actual packages on Linux
    # and should add xorg packages for linux/osx/unix platforms
    assert "libgl-devel" in linux_opengl, f"Expected libgl-devel in Linux opengl packages, got {linux_opengl}"
    assert "libopengl-devel" in linux_opengl, f"Expected libopengl-devel in Linux opengl packages, got {linux_opengl}"
    assert "xorg-libx11" in linux_opengl, f"Expected xorg-libx11 in Linux opengl packages, got {linux_opengl}"
    assert "xorg-libxext" in linux_opengl, f"Expected xorg-libxext in Linux opengl packages, got {linux_opengl}"

    # macOS should get xorg packages but no gl-devel packages
    assert "xorg-libx11" in osx_opengl, f"Expected xorg-libx11 in macOS opengl packages, got {osx_opengl}"
    assert "xorg-libxext" in osx_opengl, f"Expected xorg-libxext in macOS opengl packages, got {osx_opengl}"
    assert "libgl-devel" not in osx_opengl, f"Did not expect libgl-devel in macOS opengl packages, got {osx_opengl}"

    # Windows should have empty packages
    assert win_opengl == [], f"Expected [] for opengl on Windows, got {win_opengl}"
