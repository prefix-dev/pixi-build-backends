"""
ROS-specific metadata provider that extracts metadata from package.xml files.
"""

from typing import Optional, List
from pixi_build_backend.types.metadata_provider import PackageXmlMetadataProvider
from .distro import Distro


class ROSPackageXmlMetadataProvider(PackageXmlMetadataProvider):
    """
    ROS-specific metadata provider that formats names according to ROS conventions.
    
    This provider extends PackageXmlMetadataProvider to format package names
    as 'ros-<distro>-<package_name>' according to ROS conda packaging conventions.
    """

    def __init__(self, package_xml_path: str, distro_name: Optional[str] = None):
        """
        Initialize the ROS metadata provider.
        
        Args:
            package_xml_path: Path to the package.xml file
            distro_name: ROS distro name (e.g., "humble", "foxy"). If None, will use default.
        """
        super().__init__(package_xml_path)
        self.distro_name = distro_name
        self._distro: Optional[Distro] = None

    def _get_distro(self) -> Distro:
        """Get the ROS distro, creating it if needed."""
        if self._distro is None:
            self._distro = Distro(self.distro_name)
        return self._distro

    def name(self) -> Optional[str]:
        """Return the ROS-formatted package name (ros-<distro>-<name>)."""
        base_name = super().name()
        if base_name is None:
            return None
        
        distro = self._get_distro()
        # Convert underscores to hyphens per ROS conda naming conventions
        formatted_name = base_name.replace('_', '-')
        return f"ros-{distro.name}-{formatted_name}"

    def input_globs(self) -> List[str]:
        """Return input globs that affect this metadata provider."""
        return ["package.xml"]