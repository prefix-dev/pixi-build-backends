from pathlib import Path

from pixi_build_ros.metadata_provider import (
    PackageXmlMetadataProvider,
    ROSPackageXmlMetadataProvider,
)


def test_metadata_provider(package_xmls: Path):
    """Test the MetaDataProvider class."""
    package_xml_path = package_xmls / "custom_ros.xml"
    metadata_provider = PackageXmlMetadataProvider(str(package_xml_path))
    assert metadata_provider.name() == "custom_ros"
    assert metadata_provider.version() == "0.0.1"
    assert metadata_provider.license() == "LicenseRef-Apache License 2.0"
    assert metadata_provider.description() == "Demo"
    assert metadata_provider.homepage() == "https://test.io/custom_ros"
    assert metadata_provider.repository() == "https://github.com/test/custom_ros"


def test_ros_metadata_provider(package_xmls: Path):
    """Test the RosMetaDataProvider class."""
    package_xml_path = package_xmls / "custom_ros.xml"
    metadata_provider = ROSPackageXmlMetadataProvider(str(package_xml_path), distro_name="noetic")
    assert metadata_provider.name() == "ros-noetic-custom-ros"
    assert metadata_provider.version() == "0.0.1"
    assert metadata_provider.license() == "LicenseRef-Apache License 2.0"
    assert metadata_provider.description() == "Demo"
    assert metadata_provider.homepage() == "https://test.io/custom_ros"
    assert metadata_provider.repository() == "https://github.com/test/custom_ros"
