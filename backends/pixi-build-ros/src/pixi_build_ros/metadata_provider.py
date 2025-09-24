"""
ROS-specific metadata provider that extracts metadata from package.xml files.
"""

from pixi_build_backend.types import MetadataProvider
from pixi_build_ros.distro import Distro


class MaintainerInfo:
    """Container for maintainer information from package.xml."""

    def __init__(self, name: str, email: str):
        self.name = name
        self.email = email


class PackageData:
    """Container for parsed package.xml data."""

    def __init__(
        self,
        name: str | None = None,
        version: str | None = None,
        description: str | None = None,
        maintainers: list[MaintainerInfo] | None = None,
        licenses: list[str] | None = None,
        homepage: str | None = None,
        repository: str | None = None,
    ):
        self.name = name
        self.version = version
        self.description = description
        self.maintainers = maintainers or []
        self.licenses = licenses or []
        self.homepage = homepage
        self.repository = repository


class PackageXmlMetadataProvider(MetadataProvider):  # type: ignore[misc]  # MetadatProvider is not typed
    """
    Metadata provider that extracts metadata from ROS package.xml files.

    This provider reads ROS package.xml files and extracts package metadata
    like name, version, description, maintainers, etc.
    """

    def __init__(self, package_xml_path: str, *args, **kwargs):  # type: ignore[no-untyped-def]  # no typing for args and kwargs
        """
        Initialize the metadata provider with a package.xml file path.

        Args:
            package_xml_path: Path to the package.xml file
        """
        super().__init__(*args, **kwargs)
        self.package_xml_path = package_xml_path
        self._package_data: PackageData | None = None

    @property
    def _package_xml_data(self) -> PackageData:
        """Load and parse the package.xml file."""
        if self._package_data is not None:
            return self._package_data

        try:
            import xml.etree.ElementTree as ET

            tree = ET.parse(self.package_xml_path)
            root = tree.getroot()

            # Extract basic package information
            name_elem = root.find("name")
            version_elem = root.find("version")
            description_elem = root.find("description")

            # Extract maintainer and author information
            maintainers: list[MaintainerInfo] = []
            for maintainer in root.findall("maintainer"):
                maintainer_info = MaintainerInfo(
                    name=maintainer.text.strip() if maintainer.text else "", email=maintainer.get("email", "")
                )
                maintainers.append(maintainer_info)

            # Extract license information
            licenses = []
            for license_elem in root.findall("license"):
                if license_elem.text:
                    licenses.append(license_elem.text.strip())

            # Extract URL information
            homepage = None
            repository = None
            for url in root.findall("url"):
                url_type = url.get("type", "")
                if url_type == "website" and not homepage:
                    homepage = url.text.strip() if url.text else None
                elif url_type == "repository" and not repository:
                    repository = url.text.strip() if url.text else None

            self._package_data = PackageData(
                name=name_elem.text.strip() if name_elem is not None and name_elem.text else None,
                version=version_elem.text.strip() if version_elem is not None and version_elem.text else None,
                description=description_elem.text.strip()
                if description_elem is not None and description_elem.text
                else None,
                maintainers=maintainers,
                licenses=licenses,
                homepage=homepage,
                repository=repository,
            )

        except Exception as e:
            print(f"Warning: Failed to parse package.xml at {self.package_xml_path}: {e}")
            self._package_data = PackageData()

        return self._package_data

    def name(self) -> str | None:
        """Return the package name from package.xml."""
        return self._package_xml_data.name

    def version(self) -> str | None:
        """Return the package version from package.xml."""
        return self._package_xml_data.version

    def homepage(self) -> str | None:
        """Return the homepage URL from package.xml."""
        return self._package_xml_data.homepage

    def license(self) -> str | None:
        """Return the license from package.xml."""
        # TODO: Handle License parsing to conform to SPDX standards,
        # ROS package.xml does not enforce SPDX as strictly as rattler-build
        return None

    def license_file(self) -> str | None:
        """Return None as package.xml doesn't typically specify license files."""
        return None

    def summary(self) -> str | None:
        """Return the description as summary from package.xml."""
        description = self._package_xml_data.description
        if description and len(description) > 100:
            # Truncate long descriptions for summary
            return description[:97] + "..."
        return description

    def description(self) -> str | None:
        """Return the full description from package.xml."""
        return self._package_xml_data.description

    def documentation(self) -> str | None:
        """Return None as package.xml doesn't typically specify documentation URLs separately."""
        return None

    def repository(self) -> str | None:
        """Return the repository URL from package.xml."""
        return self._package_xml_data.repository

    def input_globs(self) -> list[str]:
        """Return input globs that affect this metadata provider."""
        return ["package.xml", "CMakeLists.txt", "setup.py", "setup.cfg"]


class ROSPackageXmlMetadataProvider(PackageXmlMetadataProvider):
    """
    ROS-specific metadata provider that formats names according to ROS conventions.

    This provider extends PackageXmlMetadataProvider to format package names
    as 'ros-<distro>-<package_name>' according to ROS conda packaging conventions.
    """

    def __init__(self, package_xml_path: str, distro: Distro | None = None):
        """
        Initialize the ROS metadata provider.

        Args:
            package_xml_path: Path to the package.xml file
            distro: ROS distro. If None, will use the base package name without distro prefix.
        """
        super().__init__(package_xml_path)
        self._distro: Distro | None = distro

    def _get_distro(self) -> Distro | None:
        return self._distro

    def name(self) -> str | None:
        """Return the ROS-formatted package name (ros-<distro>-<name>)."""
        base_name = super().name()
        if base_name is None:
            return None

        distro = self._get_distro()
        if distro:
            # Convert underscores to hyphens per ROS conda naming conventions
            formatted_name = base_name.replace("_", "-")
            return f"ros-{distro.name}-{formatted_name}"
        return base_name
