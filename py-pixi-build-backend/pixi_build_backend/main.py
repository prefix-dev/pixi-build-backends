"""
Main entry point for Python backend implementation.
"""

import sys
import asyncio
from pixi_build_backend.types.generated_recipe import GenerateRecipeProtocol
from pixi_build_backend.pixi_build_backend import py_main, PyGenerateRecipe



async def main_entry_point(instance: GenerateRecipeProtocol):
    """Main entry point for the build backend"""
    py_generator = PyGenerateRecipe(instance)

    # Remove script name from argv
    args = sys.argv[1:] if len(sys.argv) > 1 else []
    
    try:
        await py_main(py_generator, args)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

