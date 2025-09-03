import os
from itertools import chain
from pathlib import Path
from typing import Any, List

import yaml

from importlib.resources import files
from catkin_pkg.package import Package as CatkinPackage, parse_package_string

from pixi_build_backend.types.intermediate_recipe import ConditionalRequirements
from pixi_build_backend.types.item import ItemPackageDependency
from pixi_build_ros.distro import Distro


def get_build_input_globs(config: Any, editable: bool) -> List[str]:
    """Get build input globs for ROS package."""
    base_globs = [
        # Source files
        "**/*.c",
        "**/*.cpp",
        "**/*.h",
        "**/*.hpp",
        "**/*.rs",
        "**/*.sh",
        # Project configuration
        "package.xml",
        "setup.py",
        "setup.cfg",
        "pyproject.toml",
        # Build configuration
        "Makefile",
        "CMakeLists.txt",
        "MANIFEST.in",
        "tests/**/*.py",
        "docs/**/*.rst",
        "docs/**/*.md",
    ]

    python_globs = [] if editable else ["**/*.py", "**/*.pyx"]

    all_globs = base_globs + python_globs
    if hasattr(config, "extra_input_globs"):
        all_globs.extend(config.extra_input_globs)

    return all_globs
def get_package_xml_content(manifest_root: Path) -> str:
    """Read package.xml file from the manifest root."""
    package_xml_path = manifest_root / "package.xml"
    if not package_xml_path.exists():
        raise FileNotFoundError(f"package.xml not found at {package_xml_path}")

    with open(package_xml_path, 'r') as f:
        return f.read()

def convert_package_xml_to_catkin_package(package_xml_content: str) -> CatkinPackage:
    """Convert package.xml content to a CatkinPackage object."""
    package_reading_warnings = None
    package_xml = parse_package_string(package_xml_content, package_reading_warnings)

    # Evaluate conditions in the package.xml
    # TODO: validate the need for dealing with configuration conditions
    package_xml.evaluate_conditions(os.environ)

    return package_xml

def rosdep_to_conda_package_name(dep_name: str, distro: Distro) -> List[str]:
    """Convert a ROS dependency name to a conda package name."""
    # TODO: make this configurable
    # either `linux`, `osx` or `win64`
    target_platform = "osx"

    # If dependency any of the following return custom name:
    if dep_name in ["ament_cmake", "ament_python", "rosidl_default_generators", "ros_workspace"]:
        return [f"ros-{distro.name}-{dep_name.replace('_', '-')}"]

    # TODO: Currently hardcoded and not able to override, this should be configurable
    try:
        # Try to load from package data first (when installed)
        package_files = files("pixi_build_ros")
        robostack_file = package_files / "robostack.yaml"
        robostack_data = yaml.safe_load(robostack_file.read_text())
    except (FileNotFoundError, ModuleNotFoundError):
        # Fallback to development path (when running from source)
        with open(Path(__file__).parent.parent.parent / "robostack.yaml") as f:
            robostack_data = yaml.safe_load(f)

    if dep_name not in robostack_data:
        # If the dependency is not found in robostack.yaml, check the actual distro whether it exists
        if distro.has_package(dep_name):
            # This means that it is a ROS package, so we are going to assume has the `ros-<distro>-<dep_name>` format.
            return [f"ros-{distro.name}-{dep_name.replace('_', '-')}"]
        else:
            # If the dependency is not found in robostack.yaml and not in the distro, return the dependency name as is.
            return [dep_name]

    # Dependency found in robostack.yaml
    # Get the conda packages for the dependency
    conda_packages = robostack_data[dep_name].get("robostack", [])

    if isinstance(conda_packages, dict):
        # TODO: Handle different platforms
        conda_packages = conda_packages.get(target_platform, [])

    # Deduplicate of the code in:
    # https://github.com/RoboStack/vinca/blob/7d3a05e01d6898201a66ba2cf6ea771250671f58/vinca/main.py#L562
    if "REQUIRE_GL" in conda_packages:
        conda_packages.remove("REQUIRE_GL")
        if "linux" in target_platform:
            conda_packages.append("libgl-devel")
    if "REQUIRE_OPENGL" in conda_packages:
        conda_packages.remove("REQUIRE_OPENGL")
        if "linux" in target_platform:
            # TODO: this should only go into the host dependencies
            conda_packages.extend(["libgl-devel", "libopengl-devel"])
        if target_platform in ["linux", "osx", "unix"]:
            # TODO: force this into the run dependencies
            conda_packages.extend(["xorg-libx11", "xorg-libxext"])

    return conda_packages

def package_xml_to_conda_requirements(
        pkg: CatkinPackage,
        distro: Distro,
) -> ConditionalRequirements:
    """Convert a CatkinPackage to ConditionalRequirements for conda."""

    # All build related dependencies go into the build requirements
    build_deps = pkg.buildtool_depends
    # TODO: should the export dependencies be included here?
    build_deps += pkg.buildtool_export_depends
    build_deps += pkg.build_depends
    build_deps += pkg.build_export_depends
    build_deps = [d.name for d in build_deps if d.evaluated_condition]
    # Add the ros_workspace dependency as a default build dependency for ros2 packages
    if not distro.check_ros1():
        build_deps += ["ros_workspace"]
    conda_build_deps = [rosdep_to_conda_package_name(dep, distro) for dep in build_deps]
    conda_build_deps = list(dict.fromkeys(chain.from_iterable(conda_build_deps)))

    run_deps = pkg.run_depends
    run_deps += pkg.exec_depends
    run_deps += pkg.build_export_depends
    run_deps += pkg.buildtool_export_depends
    run_deps = [d.name for d in run_deps if d.evaluated_condition]
    conda_run_deps = [rosdep_to_conda_package_name(dep, distro) for dep in run_deps]
    conda_run_deps = list(dict.fromkeys(chain.from_iterable(conda_run_deps)))

    build_requirements = [ItemPackageDependency(name) for name in conda_build_deps]
    run_requirements = [ItemPackageDependency(name) for name in conda_run_deps]

    cond = ConditionalRequirements()
    # TODO: should we add all build dependencies to the host requirements?
    cond.host = build_requirements
    # assert False, "HERE I FAIL before CONDA.BUILD"
    cond.build = build_requirements
    cond.run = run_requirements

    return cond

