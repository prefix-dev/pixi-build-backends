from typing import Optional, List
from pixi_build_backend.pixi_build_backend import ( 
    PyProjectModelV1, 
)



class ProjectModelV1:
    """A project model version 1."""
    _inner: PyProjectModelV1


    @property
    def name(self) -> str:
        """Get the project name."""
        return self._inner.name
    
    @classmethod
    def _from_py(cls, model: PyProjectModelV1) -> "ProjectModelV1":
        """Create a ProjectModelV1 from a FFI PyProjectModelV1."""
        instance = cls()
        instance._inner = model
        return instance

    @property
    def version(self) -> Optional[str]:
        """Get the project version."""
        return self._inner.version

    @property
    def description(self) -> Optional[str]:
        """Get the project description."""
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
        """Get the project homepage URL."""
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

    