"""
ROS generator implementation using Python bindings.
"""

from pathlib import Path
from importlib.resources import files

from typing import Any
from pixi_build_backend.types.generated_recipe import (
    GenerateRecipeProtocol,
    GeneratedRecipe,
)
from .metadata_provider import ROSPackageXmlMetadataProvider
from pixi_build_backend.types.intermediate_recipe import Script

from pixi_build_backend.types.item import ItemPackageDependency
from pixi_build_backend.types.platform import Platform
from pixi_build_backend.types.project_model import ProjectModelV1
from pixi_build_backend.types.python_params import PythonParams

from .build_script import BuildScriptContext, BuildPlatform
from .utils import (
    get_build_input_globs,
    package_xml_to_conda_requirements,
    convert_package_xml_to_catkin_package,
    get_package_xml_content,
    load_package_map_data,
    merge_requirements,
)
from .config import ROSBackendConfig, PackageMappingSource


class ROSGenerator(GenerateRecipeProtocol):  # type: ignore[misc]  # MetadatProvider is not typed
    """ROS recipe generator using Python bindings."""

    def generate_recipe(
        self,
        model: ProjectModelV1,
        config: dict[str, Any],
        manifest_path: str,
        host_platform: Platform,
        _python_params: PythonParams | None = None,
    ) -> GeneratedRecipe:
        """Generate a recipe for a Python package."""
        manifest_root = Path(manifest_path)
        backend_config: ROSBackendConfig = ROSBackendConfig.model_validate(
            config, context={"manifest_root": manifest_root}
        )
        # Create metadata provider for package.xml
        package_xml_path = manifest_root / "package.xml"
        metadata_provider = ROSPackageXmlMetadataProvider(
            str(package_xml_path),
            backend_config.distro.name,
            extra_input_globs=list(backend_config.extra_input_globs or []),
        )

        # Create base recipe from model with metadata provider
        generated_recipe = GeneratedRecipe.from_model(model, metadata_provider)

        # Read package.xml for dependency extraction
        package_xml_str = get_package_xml_content(manifest_root)
        package_xml = convert_package_xml_to_catkin_package(package_xml_str)

        # load package map

        # TODO: Currently hardcoded and not able to override, this should be configurable
        package_files = files("pixi_build_ros")
        robostack_file = Path(str(package_files)) / "robostack.yaml"
        # workaround for from source install
        if not robostack_file.is_file():
            robostack_file = Path(__file__).parent.parent.parent / "robostack.yaml"

        package_map_data = load_package_map_data(
            backend_config.extra_package_mappings + [PackageMappingSource.from_file(robostack_file)]
        )

        # Get requirements from package.xml
        package_requirements = package_xml_to_conda_requirements(
            package_xml, backend_config.distro, host_platform, package_map_data
        )

        # Add standard dependencies
        build_deps = [
            "ninja",
            "python",
            "setuptools",
            "git",
            "git-lfs",
            "cmake",
            "cpython",
        ]
        if host_platform.is_unix:
            build_deps.extend(["patch", "make", "coreutils"])
        if host_platform.is_windows:
            build_deps.extend(["m2-patch"])
        if host_platform.is_osx:
            build_deps.extend(["tapi"])

        for dep in build_deps:
            package_requirements.build.append(ItemPackageDependency(name=dep))

        # Add compiler dependencies
        package_requirements.build.append(ItemPackageDependency("${{ compiler('c') }}"))
        package_requirements.build.append(ItemPackageDependency("${{ compiler('cxx') }}"))

        host_deps = ["python", "numpy", "pip", "pkg-config"]

        for dep in host_deps:
            package_requirements.host.append(ItemPackageDependency(name=dep))

        # add a simple default host and run dependency on the ros{2}-distro-mutex
        package_requirements.host.append(ItemPackageDependency(name=backend_config.distro.ros_distro_mutex_name))
        package_requirements.run.append(ItemPackageDependency(name=backend_config.distro.ros_distro_mutex_name))

        # Merge package requirements into the model requirements
        requirements = merge_requirements(generated_recipe.recipe.requirements, package_requirements)
        generated_recipe.recipe.requirements = requirements

        # Determine build platform
        build_platform = BuildPlatform.current()

        # Generate build script
        build_script_context = BuildScriptContext.load_from_template(
            package_xml, build_platform, manifest_root, backend_config.distro
        )
        build_script_lines = build_script_context.render()

        generated_recipe.recipe.build.script = Script(
            content=build_script_lines,
            env=backend_config.env,
        )

        if backend_config.debug_dir:
            recipe = generated_recipe.recipe.to_yaml()
            package = generated_recipe.recipe.package
            debug_file_path = backend_config.debug_dir / f"{package.name.get_concrete()}-{package.version}-recipe.yaml"
            debug_file_path.parent.mkdir(parents=True, exist_ok=True)
            with open(debug_file_path, "w") as debug_file:
                debug_file.write(recipe)

        # Test the build script before running to early out.
        # TODO: returned script.content list is not a list of strings, a container for that
        # so it cant be compared directly with the list yet
        # assert generated_recipe.recipe.build.script.content == build_script_lines, f"Script content {generated_recipe.recipe.build.script.content}, build script lines {build_script_lines}"
        return generated_recipe

    def extract_input_globs_from_build(self, config: dict[str, Any], workdir: Path, editable: bool) -> list[str]:
        """Extract input globs for the build."""
        return get_build_input_globs(config, editable)

    def default_variants(self, host_platform: Platform) -> dict[str, Any]:
        """Get the default variants for the generator."""
        variants = {}
        if host_platform.is_windows:
            variants["cxx_compiler"] = ["vs2019"]
        return variants
