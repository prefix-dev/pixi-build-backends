from pathlib import Path
import pytest
import tempfile
from pixi_build_backend.types.platform import Platform
from pixi_build_backend.types.project_model import ProjectModelV1

from pixi_build_ros.ros_generator import ROSGenerator


def test_generate_recipe_with_versions(package_xmls: Path, test_data_dir: Path):
    """Test the generate_recipe function of ROSGenerator."""
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
        assert generated_recipe.recipe.package.name.get_concrete() == "ros-noetic-custom-ros"

        req_string = list(str(req) for req in generated_recipe.recipe.requirements.run)
        assert "ros-noetic-ros-package <2.0.0" in req_string
        assert "qt-main >=5.15.0,<5.16.0" in req_string


def test_wrong_version_constraints(package_xmls: Path, test_data_dir: Path):
    """Test the generate_recipe function of ROSGenerator."""
    # Create a temporary directory to simulate the package directory
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)

        # Copy the test package.xml to the temp directory
        package_xml_source = package_xmls / "version_constraints_wrong.xml"
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
        assert "Version specifier can only be used for one package" in excinfo.value.args[0]
