from typing import Optional, List, Mapping, Dict, Any
from pathlib import Path
from pixi_build_backend.pixi_build_backend import (
    PyProjectModelV1,
)


class ProjectModelV1:
    """A project model version 1."""

    _inner: PyProjectModelV1

    def __init__(self, name: Optional[str] = None, version: Optional[str] = None):
        self._inner = PyProjectModelV1(name, version)

    @property
    def name(self) -> str:
        """
        Get the project name.

        Examples
        --------
        ```python
        >>> model = ProjectModelV1("my-project")
        >>> model.name
        'my-project'
        >>>
        ```
        """
        return self._inner.name

    @classmethod
    def _from_py(cls, model: PyProjectModelV1) -> "ProjectModelV1":
        """Create a ProjectModelV1 from a FFI PyProjectModelV1."""
        instance = cls.__new__(cls)
        instance._inner = model
        return instance

    @classmethod
    def from_json(cls, json: str) -> "ProjectModelV1":
        """Create a ProjectModelV1 from a JSON document."""
        instance = cls.__new__(cls)
        instance._inner = PyProjectModelV1.from_json(json)
        return instance

    @classmethod
    def from_dict(cls, data: Mapping[str, Any] | Dict[str, Any]) -> "ProjectModelV1":
        """Create a ProjectModelV1 from a Python mapping."""
        instance = cls.__new__(cls)
        instance._inner = PyProjectModelV1.from_dict(dict(data))
        return instance

    @classmethod
    def from_json_file(cls, path: Path | str) -> "ProjectModelV1":
        """Create a ProjectModelV1 from a JSON file."""
        instance = cls.__new__(cls)
        instance._inner = PyProjectModelV1.from_json_file(str(path))
        return instance

    @property
    def version(self) -> Optional[str]:
        """
        Get the project version.

        Examples
        --------
        ```python
        >>> model = ProjectModelV1("my-project", "1.0.0")
        >>> model.version
        '1.0.0'
        >>> ProjectModelV1("test").version is None
        True
        >>>
        ```
        """
        return self._inner.version

    @property
    def description(self) -> Optional[str]:
        """
        Get the project description.

        Examples
        --------
        ```python
        >>> model = ProjectModelV1("test")
        >>> model.description is None
        True
        >>>
        ```
        """
        return self._inner.description

    @property
    def authors(self) -> Optional[List[str]]:
        """Get the project authors."""
        return self._inner.authors

    @property
    def license(self) -> Optional[str]:
        """Get the project license."""
        return self._inner.license

    @property
    def license_file(self) -> Optional[str]:
        """Get the project license file path."""
        return self._inner.license_file

    @property
    def readme(self) -> Optional[str]:
        """Get the project readme file path."""
        return self._inner.readme

    @property
    def homepage(self) -> Optional[str]:
        """
        Get the project homepage URL.

        Examples
        --------
        ```python
        >>> model = ProjectModelV1("test")
        >>> model.homepage is None
        True
        >>>
        ```
        """
        return self._inner.homepage

    @property
    def repository(self) -> Optional[str]:
        """Get the project repository URL."""
        return self._inner.repository

    @property
    def documentation(self) -> Optional[str]:
        """Get the project documentation URL."""
        return self._inner.documentation

    def __repr__(self) -> str:
        return self._inner.__repr__()

    def _debug_str(self) -> str:
        """Get a debug string representation of the project model."""
        return self._inner._debug_str()
