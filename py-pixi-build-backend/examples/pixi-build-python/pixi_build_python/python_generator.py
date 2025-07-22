"""
Python generator implementation using Python bindings.
"""

from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Optional, List, Protocol
from pixi_build_backend.types.generated_recipe import GenerateRecipeProtocol, GeneratedRecipe
from pixi_build_backend.types.intermediate_recipe import NoArchKind, Python, Script
from pixi_build_backend.types.platform import Platform
from pixi_build_backend.types.project_model import ProjectModelV1
from pixi_build_backend.types.python_params import PythonParams

from .build_script import BuildScriptContext, Installer, BuildPlatform
from pixi_build_backend.types.entry_points import extract_entry_points
from .utils import (
    read_pyproject_toml,
    get_build_input_globs,
    get_editable_setting,
)

@dataclass
class PythonBackendConfig:
    """Python backend configuration."""
    noarch: Optional[bool] = None
    env: Optional[Dict[str, str]] = None
    debug_dir: Optional[Path] = None
    extra_input_globs: Optional[List[str]] = None
    
    
    def is_noarch(self) -> bool:
        """Whether to build a noarch package or a platform-specific package."""
        return self.noarch is None or self.noarch
    
    def get_debug_dir(self) -> Optional[Path]:
        """Get debug directory if set."""
        return self.debug_dir




class PythonGenerator(GenerateRecipeProtocol):
    """Python recipe generator using Python bindings."""
    
    
    def generate_recipe(
        self,
        model: ProjectModelV1,
        config: dict,
        manifest_path: str,
        host_platform: Platform,
        python_params: Optional[PythonParams] = None,
    ) -> GeneratedRecipe:
        """Generate a recipe for a Python package."""
        config: PythonBackendConfig = PythonBackendConfig(**config)

        manifest_root = Path(manifest_path).parent

        # host_platform = Platform._from_py_platform(host_platform)

        # python_params = PythonParams._from_inner(python_params) if python_params else None

        print(f"model is : {type(model)}")

        # model = ProjectModelV1.from_model(model)

        # Create base recipe from model
        generated_recipe = GeneratedRecipe.from_model(model, manifest_root)
        
        # Get recipe components
        recipe = generated_recipe.recipe
        requirements = recipe.requirements
        
        # Resolve requirements for the host platform
        resolved_requirements = requirements.resolve(host_platform)
        
        # Determine installer (pip or uv)
        installer = Installer.determine_installer(resolved_requirements.host)
        installer_name = installer.package_name()
        
        # Add installer to host requirements if not present
        if installer_name not in resolved_requirements.host:
            requirements.host.append(installer_name)
        
        # Add python to both host and run requirements if not present
        if "python" not in resolved_requirements.host:
            requirements.host.append("python")
        if "python" not in resolved_requirements.run:
            requirements.run.append("python")
        
        # Determine build platform
        build_platform = BuildPlatform.current()
        
        # Get editable setting
        editable = get_editable_setting(python_params)
        
        # Generate build script
        build_script_context = BuildScriptContext(
            installer=installer,
            build_platform=build_platform,
            editable=editable,
            manifest_root=manifest_root,
        )
        build_script_lines = build_script_context.render()
        
        # Determine noarch setting
        noarch_kind = NoArchKind.python() if config.is_noarch() else None

        # Read pyproject.toml
        pyproject_manifest = read_pyproject_toml(manifest_root)
        
        # Extract entry points
        entry_points = extract_entry_points(pyproject_manifest)
        
        # Update recipe components
        recipe.build.python = Python(entry_points=entry_points)
        recipe.build.noarch = noarch_kind
        recipe.build.script = Script(
            content=build_script_lines,
            env=config.env,
        )
        
        return generated_recipe
    
    def extract_input_globs_from_build(
        self, 
        config: PythonBackendConfig, 
        workdir: Path, 
        editable: bool
    ) -> List[str]:
        """Extract input globs for the build."""
        return get_build_input_globs(config, workdir, editable)