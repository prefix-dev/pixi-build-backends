from pixi_build_backend.pixi_build_backend import PyPythonParams


class PythonParams:
    """A Python parameters wrapper."""
    _inner: PyPythonParams

    def __init__(self, editable: bool = False):
        self._inner = PyPythonParams(editable=editable)

    @property
    def editable(self) -> bool:
        """Get the editable flag."""
        return self._inner.editable

    @editable.setter
    def editable(self, value: bool):
        """Set the editable flag."""
        self._inner.set_editable(value)

    def __repr__(self) -> str:
        return self._inner.__repr__()

    @classmethod
    def _from_py(cls, inner: PyPythonParams) -> "PythonParams":
        """Create a PythonParams from a FFI PyPythonParams."""
        instance = cls.__new__(cls)
        instance._inner = inner
        return instance