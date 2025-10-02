import dataclasses
import os
from itertools import chain
from pathlib import Path

from catkin_pkg.package import Package as CatkinPackage, parse_package_string, Dependency

from pixi_build_backend.types.intermediate_recipe import ConditionalRequirements
from pixi_build_backend.types.item import ItemPackageDependency
from pixi_build_backend.types.platform import Platform
from pixi_build_ros.distro import Distro
from rattler import Version
from .config import PackageMapEntry, PackageMappingSource, ROSBackendConfig


@dataclasses.dataclass
class PackageNameWithSpec:
    """Package name with spec."""

    name: str
    spec: str | None = None


# Any in here means ROSBackendConfig
def get_build_input_globs(config: ROSBackendConfig, editable: bool) -> list[str]:
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
    if config.extra_input_globs:
        all_globs.extend(config.extra_input_globs)
    return all_globs


def get_package_xml_content(manifest_root: Path) -> str:
    """Read package.xml file from the manifest root."""
    package_xml_path = manifest_root / "package.xml"
    if not package_xml_path.exists():
        raise FileNotFoundError(f"package.xml not found at {package_xml_path}")

    with open(package_xml_path) as f:
        return f.read()


def convert_package_xml_to_catkin_package(package_xml_content: str) -> CatkinPackage:
    """Convert package.xml content to a CatkinPackage object."""
    package_reading_warnings = None
    package_xml = parse_package_string(package_xml_content, package_reading_warnings)

    # Evaluate conditions in the package.xml
    # TODO: validate the need for dealing with configuration conditions
    package_xml.evaluate_conditions(os.environ)

    return package_xml


def load_package_map_data(package_map_sources: list[PackageMappingSource]) -> dict[str, PackageMapEntry]:
    """Load and merge package map data from files and inline mappings."""

    result: dict[str, PackageMapEntry] = {}
    for source in reversed(package_map_sources):
        result.update(source.get_package_mapping())
    return result


def rosdep_to_conda_package_name(
    dep_name: str,
    distro: Distro,
    host_platform: Platform,
    package_map_data: dict[str, PackageMapEntry],
    spec_str: str = "",
) -> list[str]:
    """Convert a ROS dependency name to a conda package name."""
    if host_platform.is_linux:
        target_platform = "linux"
    elif host_platform.is_windows:
        target_platform = "win64"
    elif host_platform.is_osx:
        target_platform = "osx"
    else:
        raise RuntimeError(f"Unsupported platform: {host_platform}")

    if dep_name not in package_map_data:
        # Package name isn't found in the package map, so we are going to assume it is a ROS package.
        return [f"ros-{distro.name}-{dep_name.replace('_', '-')}{spec_str or ''}"]

    # Dependency found in package map

    # Case 1: It's a custom ROS dependency
    if "ros" in package_map_data[dep_name]:
        return [f"ros-{distro.name}-{dep.replace('_', '-')}{spec_str}" for dep in package_map_data[dep_name]["ros"]]

    # Case 2: It's a custom package name
    elif "conda" in package_map_data[dep_name] or "robostack" in package_map_data[dep_name]:
        # determine key
        key = "robostack" if "robostack" in package_map_data[dep_name] else "conda"

        # Get the conda packages for the dependency
        conda_packages = package_map_data[dep_name].get(key, [])

        if isinstance(conda_packages, dict):
            # TODO: Handle different platforms
            conda_packages = conda_packages.get(target_platform, [])

        additional_packages = []
        # Deduplicate of the code in:
        # https://github.com/RoboStack/vinca/blob/7d3a05e01d6898201a66ba2cf6ea771250671f58/vinca/main.py#L562
        if "REQUIRE_GL" in conda_packages:
            conda_packages.remove("REQUIRE_GL")
            if "linux" in target_platform:
                additional_packages.append("libgl-devel")
        if "REQUIRE_OPENGL" in conda_packages:
            conda_packages.remove("REQUIRE_OPENGL")
            if "linux" in target_platform:
                # TODO: this should only go into the host dependencies
                additional_packages.extend(["libgl-devel", "libopengl-devel"])
            if target_platform in ["linux", "osx", "unix"]:
                # TODO: force this into the run dependencies
                additional_packages.extend(["xorg-libx11", "xorg-libxext"])

        # Add the version specifier if it exists and it is only one package defined
        if spec_str:
            if len(conda_packages) == 1:
                if " " not in conda_packages[0]:
                    conda_packages = [f"{conda_packages[0]}{spec_str}"]
                else:
                    raise ValueError(
                        f"Version specifier can only be used for a package without constraint already present, "
                        f"but found {conda_packages[0]} for {dep_name} "
                        f"in the package map."
                    )
            else:
                raise ValueError(
                    f"Version specifier can only be used for one package, "
                    f"but found {len(conda_packages)} packages for {dep_name} "
                    f"in the package map."
                )

        return conda_packages + additional_packages
    else:
        raise ValueError(f"Unknown package map entry: {dep_name}.")


def _format_version_constraints_to_string(dependency: Dependency) -> str:
    """Format the version constraints from a ros package.xml to a string"""
    for version in [
        dependency.version_eq,
        dependency.version_gte,
        dependency.version_gt,
        dependency.version_lte,
        dependency.version_lt,
    ]:
        if version is None:
            continue
        elif version == "":
            raise ValueError(
                f"Incorrect version specification in package.xml: '{dependency.name}': version is empty string (\"\")"
            )
        try:
            # check if we can parse the version
            Version(version)
        except TypeError as e:
            raise ValueError(
                f"Incorrect version specification in package.xml: '{dependency.name}' at version '{version}' "
            ) from e

    if dependency.version_eq:
        return f" =={dependency.version_eq}"

    version_string_list = []
    if dependency.version_gte:
        version_string_list.append(f">={dependency.version_gte}")
    if dependency.version_gt:
        version_string_list.append(f">{dependency.version_gt}")
    if dependency.version_lte:
        version_string_list.append(f"<={dependency.version_lte}")
    if dependency.version_lt:
        version_string_list.append(f"<{dependency.version_lt}")

    if not version_string_list:
        return ""

    return " " + ",".join(version_string_list)


def package_xml_to_conda_requirements(
    pkg: CatkinPackage,
    distro: Distro,
    host_platform: Platform,
    package_map_data: dict[str, PackageMapEntry],
) -> ConditionalRequirements:
    """Convert a CatkinPackage to ConditionalRequirements for conda."""
    # All build related dependencies go into the build requirements
    build_deps = pkg.buildtool_depends
    # TODO: should the export dependencies be included here?
    build_deps += pkg.buildtool_export_depends
    build_deps += pkg.build_depends
    build_deps += pkg.build_export_depends
    # Also add test dependencies, because they might be needed during build (i.e. for pytest/catch2 etc in CMake macros)
    build_deps += pkg.test_depends
    build_deps = [
        PackageNameWithSpec(name=d.name, spec=_format_version_constraints_to_string(d))
        for d in build_deps
        if d.evaluated_condition
    ]
    # Add the ros_workspace dependency as a default build dependency for ros2 packages
    if not distro.check_ros1():
        build_deps += [PackageNameWithSpec(name="ros_workspace")]
    conda_build_deps_chain = [
        rosdep_to_conda_package_name(dep.name, distro, host_platform, package_map_data, dep.spec) for dep in build_deps
    ]
    conda_build_deps = list(chain.from_iterable(conda_build_deps_chain))

    run_deps = pkg.run_depends
    run_deps += pkg.exec_depends
    run_deps += pkg.build_export_depends
    run_deps += pkg.buildtool_export_depends
    run_deps = [
        PackageNameWithSpec(d.name, spec=_format_version_constraints_to_string(d))
        for d in run_deps
        if d.evaluated_condition
    ]
    conda_run_deps_chain = [
        rosdep_to_conda_package_name(dep.name, distro, host_platform, package_map_data, dep.spec) for dep in run_deps
    ]
    conda_run_deps = list(chain.from_iterable(conda_run_deps_chain))

    build_requirements = [ItemPackageDependency(name) for name in conda_build_deps]
    run_requirements = [ItemPackageDependency(name) for name in conda_run_deps]

    cond = ConditionalRequirements()
    # TODO: should we add all build dependencies to the host requirements?
    cond.host = build_requirements
    cond.build = build_requirements
    cond.run = run_requirements
    if "navigator" == pkg.name:
        raise Exception(cond)

    return cond
