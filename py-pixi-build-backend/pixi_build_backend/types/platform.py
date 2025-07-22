
from pixi_build_backend.pixi_build_backend import (
    PyPlatform,
)

class Platform:
    """
    Example class for platform handling.
    This should implement the Platform trait.
    """
    _inner = PyPlatform

    def __init__(self, value):
        self._inner = PyPlatform(value)

    @classmethod
    def current(cls):
        """
        Returns the current platform.
        """
        return cls._from_py(PyPlatform.current())

    @classmethod
    def _from_py(cls, py_platform: PyPlatform) -> "Platform":
        """Construct Rattler version from FFI PyArch object."""
        return cls(py_platform.name)
    

