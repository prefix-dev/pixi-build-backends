"""
Python metadata provider types and protocols.
"""

from typing import Optional, Protocol, Dict, Any, List
from abc import ABC, abstractmethod


class MetadataProvider(Protocol):
    """
    Protocol for providing package metadata.
    
    This protocol defines the interface that metadata providers must implement
    to supply package metadata like name, version, description, etc.
    """

    def name(self) -> Optional[str]:
        """Return the package name or None if not available."""
        return None

    def version(self) -> Optional[str]:
        """Return the package version or None if not available."""
        return None

    def homepage(self) -> Optional[str]:
        """Return the package homepage URL or None if not available."""
        return None

    def license(self) -> Optional[str]:
        """Return the package license or None if not available."""
        return None

    def license_file(self) -> Optional[str]:
        """Return the path to the license file or None if not available."""
        return None

    def summary(self) -> Optional[str]:
        """Return a short package summary or None if not available."""
        return None

    def description(self) -> Optional[str]:
        """Return the package description or None if not available."""
        return None

    def documentation(self) -> Optional[str]:
        """Return the documentation URL or None if not available."""
        return None

    def repository(self) -> Optional[str]:
        """Return the repository URL or None if not available."""
        return None


class DefaultMetadataProvider:
    """
    Default metadata provider that returns None for all metadata fields.
    
    This is equivalent to the Rust DefaultMetadataProvider and can be used
    when no custom metadata is needed.
    """

    def name(self) -> Optional[str]:
        return None

    def version(self) -> Optional[str]:
        return None

    def homepage(self) -> Optional[str]:
        return None

    def license(self) -> Optional[str]:
        return None

    def license_file(self) -> Optional[str]:
        return None

    def summary(self) -> Optional[str]:
        return None

    def description(self) -> Optional[str]:
        return None

    def documentation(self) -> Optional[str]:
        return None

    def repository(self) -> Optional[str]:
        return None


class PackageXmlMetadataProvider:
    """
    Metadata provider that extracts metadata from ROS package.xml files.
    
    This provider reads ROS package.xml files and extracts package metadata
    like name, version, description, maintainers, etc.
    """

    def __init__(self, package_xml_path: str):
        """
        Initialize the metadata provider with a package.xml file path.
        
        Args:
            package_xml_path: Path to the package.xml file
        """
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
        licenses = data.get('licenses', [])
        return ', '.join(licenses) if licenses else None

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