"""
Build script generation for Python backend.
"""

from enum import Enum
from pathlib import Path
from typing import Any, Dict, List
import platform
from catkin_pkg.package import Package as CatkinPackage

class BuildPlatform(Enum):
    """Build platform types."""

    WINDOWS = "windows"
    UNIX = "unix"

    @classmethod
    def current(cls) -> "BuildPlatform":
        """Get current build platform."""
        return cls.WINDOWS if platform.system() == "Windows" else cls.UNIX


class BuildScriptContext:
    """Context for build script generation."""

    def __init__(
        self,
        script_content: str,
        build_platform: BuildPlatform,
        source_dir: Path,
    ):
        self.script_content = script_content
        self.build_platform = build_platform
        self.source_dir = source_dir

    def render(self) -> List[str]:
        """Render the build script content into a list of lines."""
        return self.script_content.splitlines()

    @classmethod
    def load_from_template(cls, pkg: CatkinPackage, platform: BuildPlatform, source_dir: Path) -> "BuildScriptContext":
        """Get the build script from the template directory based on the package type."""
        # TODO: deal with other script languages, e.g. for Windows
        templates_dir = Path(__file__).parent.parent.parent / "templates"
        if pkg.get_build_type() in ["ament_cmake"]:
            script_path = templates_dir / "build_ament_cmake.sh.in"
        elif pkg.get_build_type() in ["ament_python"]:
            script_path = templates_dir / "build_ament_python.sh.in"
        elif pkg.get_build_type() in ["cmake", "catkin"]:
            script_path = templates_dir / "build_catkin.sh.in"
        else:
            raise ValueError(f"Unsupported build type: {pkg.get_build_type()}")
        
        with open(script_path, 'r') as f:
            script_content = f.read()

        script_content = script_content.replace("@SRC_DIR@", str(source_dir))

        return cls(
            script_content=script_content,
            build_platform=platform,
            source_dir=source_dir,
        )
