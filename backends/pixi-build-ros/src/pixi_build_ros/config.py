import os
import pydantic
import yaml
from pathlib import Path
from typing import Any

from pixi_build_ros.distro import Distro


def _parse_str_as_abs_path(value: str | Path, manifest_root: Path) -> Path:
    """Parse a string as a Path."""
    # Ensure the debug directory is a Path object
    if isinstance(value, str):
        value = Path(value)
    # Ensure it's an absolute path
    if not value.is_absolute():
        # Convert to absolute path relative to manifest root
        return (manifest_root / value).resolve()
    return value


PackageMapEntry = dict[str, list[str] | dict[str, list[str]]]


class PackageMappingSource:
    """Describes where additional package mapping data comes from."""

    def __init__(self, mapping: dict[str, PackageMapEntry], source_file: Path | None = None):
        if mapping is None:
            raise ValueError("PackageMappingSource mapping cannot be null.")
        if not isinstance(mapping, dict):
            raise TypeError("PackageMappingSource mapping must be a dictionary.")
        # Copy to keep the source immutable for callers.
        self.mapping: dict[str, PackageMapEntry] = dict(mapping)
        # Track the source file path if this came from a file
        self.source_file: Path | None = source_file

    @classmethod
    def from_mapping(cls, mapping: dict[str, PackageMapEntry]) -> "PackageMappingSource":
        """Create a source directly from a mapping dictionary."""
        return cls(mapping, source_file=None)

    @classmethod
    def from_file(cls, file_path: str | Path) -> "PackageMappingSource":
        """Create a source from a mapping file."""
        path = Path(file_path)
        if not path.exists():
            raise ValueError(f"Additional package map file '{path}' not found.")
        with open(path) as f:
            data = yaml.safe_load(f) or {}
        if not isinstance(data, dict):
            raise TypeError("Expected package map file to contain a dictionary.")
        return cls(data, source_file=path)

    def get_package_mapping(self) -> dict[str, PackageMapEntry]:
        return dict(self.mapping)

    def get_source_file(self) -> Path | None:
        """Return the source file path if this mapping came from a file."""
        return self.source_file


class ROSBackendConfig(pydantic.BaseModel, extra="forbid", arbitrary_types_allowed=True):
    """ROS backend configuration."""

    # ROS distribution to use, e.g., "foxy", "galactic", "humble"
    # TODO: This should be figured out in some other way, not from the config.
    distro: Distro

    noarch: bool | None = None
    # Environment variables to set during the build
    env: dict[str, str] | None = None
    # Directory for debug files of this script
    debug_dir: Path | None = pydantic.Field(default=None, alias="debug-dir")
    # Extra input globs to include in the build hash
    extra_input_globs: list[str] | None = pydantic.Field(default=None, alias="extra-input-globs")

    # Extra package mappings to use in the build
    extra_package_mappings: list[PackageMappingSource] = pydantic.Field(
        default_factory=list, alias="extra-package-mappings"
    )

    def is_noarch(self) -> bool:
        """Whether to build a noarch package or a platform-specific package."""
        return self.noarch is None or self.noarch

    def get_package_mapping_file_paths(self) -> list[Path]:
        """Get all file paths from package mappings that came from files."""
        file_paths = []
        for source in self.extra_package_mappings:
            if source_file := source.get_source_file():
                file_paths.append(source_file)
        return file_paths

    @pydantic.field_validator("distro", mode="before")
    @classmethod
    def _parse_distro(cls, value: str | Distro) -> Distro:
        """Parse a distro string."""
        if isinstance(value, str):
            return Distro(value)
        return value

    @pydantic.field_validator("debug_dir", mode="before")
    @classmethod
    def _parse_debug_dir(cls, value: Any, info: pydantic.ValidationInfo) -> Path | None:
        """Parse debug directory if set."""
        if value is None:
            return None
        base_path = Path(os.getcwd())
        if info.context and "manifest_root" in info.context:
            base_path = Path(info.context["manifest_root"])
        return _parse_str_as_abs_path(value, base_path)

    @pydantic.field_validator("extra_package_mappings", mode="before")
    @classmethod
    def _parse_package_mappings(
        cls, input_value: Any, info: pydantic.ValidationInfo
    ) -> list[PackageMappingSource] | None:
        """Parse additional package mappings if set."""
        if input_value is None:
            return []
        if info.context and "manifest_root" in info.context:
            base_path = Path(info.context["manifest_root"])

        result: list[PackageMappingSource] = []
        for raw_entry in input_value:
            # match for cases
            # it's already a package mapping source (usually for testing)
            if isinstance(raw_entry, PackageMappingSource):
                entry = raw_entry
            elif isinstance(raw_entry, dict):
                if "file" in raw_entry:
                    file_value = raw_entry["file"]
                    entry = PackageMappingSource.from_file(_parse_str_as_abs_path(file_value, base_path))
                elif "mapping" in raw_entry:
                    mapping_value = raw_entry["mapping"]
                    entry = PackageMappingSource.from_mapping(mapping_value)
                else:
                    entry = PackageMappingSource.from_mapping(raw_entry)
            elif isinstance(raw_entry, str | Path):
                entry = PackageMappingSource.from_file(_parse_str_as_abs_path(raw_entry, base_path))
            else:
                raise ValueError(
                    f"Unrecognized entry for extra-package-mappings: {raw_entry} of type {type(raw_entry)}."
                )
            result.append(entry)
        return result
