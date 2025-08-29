"""
ROS-specific metadata provider that extracts metadata from package.xml files.
"""

from typing import Optional, List, Dict, Any

from pixi_build_backend.types import MetadataProvider
from pixi_build_ros.distro import Distro

class PackageXmlMetadataProvider(MetadataProvider):
    """
    Metadata provider that extracts metadata from ROS package.xml files.

    This provider reads ROS package.xml files and extracts package metadata
    like name, version, description, maintainers, etc.
    """

    def __init__(self, package_xml_path: str, *args, **kwargs):
        """
        Initialize the metadata provider with a package.xml file path.

        Args:
            package_xml_path: Path to the package.xml file
        """
        super().__init__(*args, **kwargs)
        self.package_xml_path = package_xml_path
        self._package_data: Optional[Dict[str, Any]] = None

    def _load_package_xml(self) -> Dict[str, Any]:
        """Load and parse the package.xml file."""
        if self._package_data is not None:
            return self._package_data

        try:
            import xml.etree.ElementTree as ET

            tree = ET.parse(self.package_xml_path)
            root = tree.getroot()

            # Extract basic package information
            name_elem = root.find('name')
            version_elem = root.find('version')
            description_elem = root.find('description')

            # Extract maintainer and author information
            maintainers = []
            for maintainer in root.findall('maintainer'):
                maintainer_info = {
                    'name': maintainer.text.strip() if maintainer.text else '',
                    'email': maintainer.get('email', '')
                }
                maintainers.append(maintainer_info)

            # Extract license information
            licenses = []
            for license_elem in root.findall('license'):
                if license_elem.text:
                    licenses.append(license_elem.text.strip())

            # Extract URL information
            homepage = None
            repository = None
            for url in root.findall('url'):
                url_type = url.get('type', '')
                if url_type == 'website' and not homepage:
                    homepage = url.text.strip() if url.text else None
                elif url_type == 'repository' and not repository:
                    repository = url.text.strip() if url.text else None

            self._package_data = {
                'name': name_elem.text.strip() if name_elem is not None and name_elem.text else None,
                'version': version_elem.text.strip() if version_elem is not None and version_elem.text else None,
                'description': description_elem.text.strip() if description_elem is not None and description_elem.text else None,
                'maintainers': maintainers,
                'licenses': licenses,
                'homepage': homepage,
                'repository': repository,
            }

        except Exception as e:
            print(f"Warning: Failed to parse package.xml at {self.package_xml_path}: {e}")
            self._package_data = {}

        return self._package_data

    def name(self) -> Optional[str]:
        """Return the package name from package.xml."""
        data = self._load_package_xml()
        return data.get('name')

    def version(self) -> Optional[str]:
        """Return the package version from package.xml."""
        data = self._load_package_xml()
        return data.get('version')

    def homepage(self) -> Optional[str]:
        """Return the homepage URL from package.xml."""
        data = self._load_package_xml()
        return data.get('homepage')

    def license(self) -> Optional[str]:
        """Return the license from package.xml."""
        data = self._load_package_xml()

        # TODO: Handle License parsing to conform to SPDX standards,
        # ROS package.xml does not enforce SPDX as strictly as rattler-build

        return None

    def license_file(self) -> Optional[str]:
        """Return None as package.xml doesn't typically specify license files."""
        return None

    def summary(self) -> Optional[str]:
        """Return the description as summary from package.xml."""
        data = self._load_package_xml()
        description = data.get('description')
        if description and len(description) > 100:
            # Truncate long descriptions for summary
            return description[:97] + "..."
        return description

    def description(self) -> Optional[str]:
        """Return the full description from package.xml."""
        data = self._load_package_xml()
        return data.get('description')

    def documentation(self) -> Optional[str]:
        """Return None as package.xml doesn't typically specify documentation URLs separately."""
        return None

    def repository(self) -> Optional[str]:
        """Return the repository URL from package.xml."""
        data = self._load_package_xml()
        return data.get('repository')

    def input_globs(self) -> List[str]:
        """Return input globs that affect this metadata provider."""
        return ["package.xml"]


class ROSPackageXmlMetadataProvider(PackageXmlMetadataProvider):
    """
    ROS-specific metadata provider that formats names according to ROS conventions.

    This provider extends PackageXmlMetadataProvider to format package names
    as 'ros-<distro>-<package_name>' according to ROS conda packaging conventions.
    """

    def __init__(self, package_xml_path: str, distro: Optional[Distro] = None):
        """
        Initialize the ROS metadata provider.

        Args:
            package_xml_path: Path to the package.xml file
            distro: ROS distro. If None, will use the base package name without distro prefix.
        """
        super().__init__(package_xml_path)
        self._distro: Optional[Distro] = distro

    def _get_distro(self) -> Optional[Distro]:
        return self._distro

    def name(self) -> Optional[str]:
        """Return the ROS-formatted package name (ros-<distro>-<name>)."""
        base_name = super().name()
        if base_name is None:
            return None

        distro = self._get_distro()
        if distro:
            # Convert underscores to hyphens per ROS conda naming conventions
            formatted_name = base_name.replace('_', '-')
            return f"ros-{distro.name}-{formatted_name}"
        return base_name
