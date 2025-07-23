"""
Utility functions for Python backend.
"""

import os
import toml
from pathlib import Path
from typing import Dict, List, Optional


def read_pyproject_toml(manifest_root: Path) -> Optional[Dict]:
    """Read pyproject.toml if it exists."""
    pyproject_path = manifest_root / "pyproject.toml"
    if pyproject_path.exists():
        return toml.load(pyproject_path)
    return None


def get_build_input_globs(config, workdir: Path, editable: bool) -> List[str]:
    """Get build input globs for Python package."""
    base_globs = [
        # Source files
        "**/*.c",
        "**/*.cpp",
        "**/*.rs",
        "**/*.sh",
        # Common data files
        "**/*.json",
        "**/*.yaml",
        "**/*.yml",
        "**/*.txt",
        # Project configuration
        "setup.py",
        "setup.cfg",
        "pyproject.toml",
        "requirements*.txt",
        "Pipfile",
        "Pipfile.lock",
        "poetry.lock",
        "tox.ini",
        # Build configuration
        "Makefile",
        "MANIFEST.in",
        "tests/**/*.py",
        "docs/**/*.rst",
        "docs/**/*.md",
        # Versioning
        "VERSION",
        "version.py",
    ]

    python_globs = [] if editable else ["**/*.py", "**/*.pyx"]

    all_globs = base_globs + python_globs
    if hasattr(config, "extra_input_globs"):
        all_globs.extend(config.extra_input_globs)

    return all_globs


def get_editable_setting(python_params) -> bool:
    """Get editable setting from environment or params."""
    env_editable = os.environ.get("BUILD_EDITABLE_PYTHON", "").lower() == "true"
    if env_editable:
        return True

    if python_params and hasattr(python_params, "editable"):
        return python_params.editable

    return False
