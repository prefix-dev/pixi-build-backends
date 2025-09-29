from pathlib import Path
import pytest
import tempfile

from pixi_build_backend.types.intermediate_recipe import Script
from pixi_build_backend.types.platform import Platform
from pixi_build_backend.types.project_model import ProjectModelV1
from pixi_build_ros.ros_generator import ROSGenerator


def test_generate_recipe_with_versions(package_xmls: Path, test_data_dir: Path, snapshot):
    """Test the generate_recipe function of ROSGenerator with versions."""
    # Create a temporary directory to simulate the package directory
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)

        # Copy the test package.xml to the temp directory
        package_xml_source = package_xmls / "version_constraints.xml"
        package_xml_dest = temp_path / "package.xml"
        package_xml_dest.write_text(package_xml_source.read_text(encoding="utf-8"))

        # Create a minimal ProjectModelV1 instance
        model = ProjectModelV1()

        # Create config for ROS backend
        config = {
            "distro": "noetic",
            "noarch": False,
            "extra-package-mappings": [str(test_data_dir / "other_package_map.yaml")],
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
            host_platform=host_platform,
        )

        # Verify the generated recipe has the expected requirements
        # remove the build script as it container a tmp variable
        generated_recipe.recipe.build.script = Script("")
        assert generated_recipe.recipe.to_yaml() == snapshot


def test_generate_recipe_with_mutex_version(package_xmls: Path, test_data_dir: Path, snapshot):
    """Test the generate_recipe function of ROSGenerator with versions."""
    # Create a temporary directory to simulate the package directory
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)

        # Copy the test package.xml to the temp directory
        package_xml_source = package_xmls / "version_constraints.xml"
        package_xml_dest = temp_path / "package.xml"
        package_xml_dest.write_text(package_xml_source.read_text(encoding="utf-8"))

        # Create config for ROS backend
        config = {
            "distro": "noetic",
            "noarch": False,
            "extra-package-mappings": [str(test_data_dir / "other_package_map.yaml")],
        }

        model_payload = {
            "name": "custom_ros",
            "version": "0.0.1",
            "description": "Demo",
            "authors": ["Tester the Tester"],
            "targets": {
                "defaultTarget": {
                    "hostDependencies": {"ros-distro-mutex": {"binary": {"version": "0.5.*"}}},
                    "buildDependencies": {},
                    "runDependencies": {"rich": {"binary": {"version": ">=10.0"}}},
                },
                "targets": {},
            },
        }
        model = ProjectModelV1.from_dict(model_payload)

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

        # Verify the generated recipe has the mutex requirements
        # remove the build script as it container a tmp variable
        generated_recipe.recipe.build.script = Script("")
        assert generated_recipe.recipe.to_yaml() == snapshot


def test_generate_recipe_with_versions_in_model_and_package(package_xmls: Path, test_data_dir: Path, snapshot):
    """Test the generate_recipe function of ROSGenerator with versions."""
    # Create a temporary directory to simulate the package directory
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)

        # Copy the test package.xml to the temp directory
        package_xml_source = package_xmls / "version_constraints.xml"
        package_xml_dest = temp_path / "package.xml"
        package_xml_dest.write_text(package_xml_source.read_text(encoding="utf-8"))

        # Create config for ROS backend
        config = {
            "distro": "noetic",
            "noarch": False,
            "extra-package-mappings": [str(test_data_dir / "other_package_map.yaml")],
        }

        model_payload = {
            "name": "custom_ros",
            "version": "0.0.1",
            "description": "Demo",
            "authors": ["Tester the Tester"],
            "targets": {
                "defaultTarget": {
                    "hostDependencies": {},
                    "buildDependencies": {},
                    "runDependencies": {"asio": {"binary": {"version": ">=9.0"}}},
                },
                "targets": {},
            },
        }
        model = ProjectModelV1.from_dict(model_payload)

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

        # Verify the generated recipe has the mutex requirements
        # remove the build script as it container a tmp variable
        generated_recipe.recipe.build.script = Script("")
        assert generated_recipe.recipe.to_yaml() == snapshot


def test_version_constraints_for_package_with_multiple_keys(package_xmls: Path, test_data_dir: Path):
    """Test that generate recipe for packages with multiple keys fails."""
    # Create a temporary directory to simulate the package directory
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)

        # Copy the test package.xml to the temp directory
        package_xml_source = package_xmls / "version_constraints_double_package_map_entry.xml"
        package_xml_dest = temp_path / "package.xml"
        package_xml_dest.write_text(package_xml_source.read_text(encoding="utf-8"))

        # Create a minimal ProjectModelV1 instance
        model = ProjectModelV1()

        # Create config for ROS backend
        config = {
            "distro": "noetic",
            "noarch": False,
            "extra-package-mappings": [str(test_data_dir / "other_package_map.yaml")],
        }

        # Create host platform
        host_platform = Platform.current()

        # Create ROSGenerator instance
        generator = ROSGenerator()

        with pytest.raises(ValueError) as excinfo:
            # Generate the recipe
            generator.generate_recipe(
                model=model,
                config=config,
                manifest_path=str(temp_path),
                host_platform=host_platform,
            )
        assert "Version specifier can only be used for one package" in str(excinfo.value)
