from rosdistro import get_cached_distribution, get_index, get_index_url


class Distro(object):
    def __init__(self, distro_name):
        index = get_index(get_index_url())
        self._distro = get_cached_distribution(index, distro_name)
        self.distro_name = distro_name

        # cache distribution type
        self._distribution_type = index.distributions[distro_name]["distribution_type"]
        self._python_version = index.distributions[distro_name]["python_version"]

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
        return package_name in self._distro.release_packages
