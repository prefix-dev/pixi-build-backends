#!/usr/bin/env python3
# """
# Example usage of py_pixi_build_backend Python bindings.
# """

# try:
#     from py_pixi_build_backend import (
#         PyPythonBackend,
#         PyProjectModel,
#         PyPlatform,
#         PyPythonParams
#     )

from pixi_build_backend import (
    PyBackendConfig,
)



class BackendConfig:
    """
    Example configuration class for the backend.
    This should implement the BackendConfig trait.
    """

    def __init__(self, debug_dir=None):
        self._python_backend = PyBackendConfig(debug_dir=debug_dir)

        # self._debug_dir = debug_dir

    def debug_dir(self):
        return self._python_backend.debug_dir()




# Example usage of the BackendConfig
config = BackendConfig(debug_dir="/path/to/debug/dir")

# Accessing the debug directory
print("Debug Directory:", config.debug_dir())
