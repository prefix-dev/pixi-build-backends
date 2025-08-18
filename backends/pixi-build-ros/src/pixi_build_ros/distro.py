import os
import urllib.request

from rosdistro import get_cached_distribution, get_index, get_index_url
from rosdistro.dependency_walker import DependencyWalker
from rosdistro.manifest_provider import get_release_tag


class Distro(object):
    def __init__(self, distro_name):
        index = get_index(get_index_url())
        self._distro = get_cached_distribution(index, distro_name)
        self.distro_name = distro_name

        # cache distribution type
        self._distribution_type = index.distributions[distro_name]["distribution_type"]
        self._python_version = index.distributions[distro_name]["python_version"]

        os.environ["ROS_VERSION"] = "1" if self.check_ros1() else "2"

    @property
    def name(self) -> str:
        return self.distro_name

    def check_ros1(self):
        return self._distribution_type == "ros1"

    def get_python_version(self):
        return self._python_version

    def get_package_names(self):
        return self._distro.release_packages.keys()

    def has_package(self, package_name):
        """Check if the distribution has a specific package."""
        packages = self._distro.release_packages
        return package_name in self._distro.release_packages
