from typing import Optional
from pixi_build_backend.pixi_build_backend import parse_entry_points_from_scripts



def extract_entry_points(pyproject_manifest: Optional[dict]) -> list:
    """Extract entry points from pyproject.toml."""
    if not pyproject_manifest:
        return []
    
    project = pyproject_manifest.get("project", {})
    scripts = project.get("scripts", {})
    
    if not scripts:
        return []
    
    # call the FFI function to parse entry points from scripts
    return parse_entry_points_from_scripts(scripts)
