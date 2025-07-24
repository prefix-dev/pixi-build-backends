from typing import Any, Optional, Dict, List
from pixi_build_backend.pixi_build_backend import parse_entry_points_from_scripts


def extract_entry_points(pyproject_manifest: Optional[Dict[str, Any]]) -> List[str]:
    """
    Extract entry points from pyproject.toml.

    Parameters
    ----------
    pyproject_manifest : Optional[dict]
        The pyproject.toml manifest dictionary.

    Returns
    -------
    list
        A list of entry points, or empty list if no scripts found.

    Examples
    --------
    ```python
    >>> manifest = {"project": {"scripts": {"my_script": "module:function"}}}
    >>> extract_entry_points(manifest)
    ['my_script = module:function']
    >>> extract_entry_points(None)
    []
    >>> extract_entry_points({})
    []
    >>>
    ```
    """
    if not pyproject_manifest:
        return []

    project = pyproject_manifest.get("project", {})
    scripts: Dict[str, str] = project.get("scripts", {})

    if not scripts:
        return []

    # call the FFI function to parse entry points from scripts
    return parse_entry_points_from_scripts(scripts)
