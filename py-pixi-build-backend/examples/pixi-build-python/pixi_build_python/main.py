import asyncio
from .python_generator import PythonGenerator
from pixi_build_backend.main import main_entry_point


def main():
    """Main entry point for the script."""
    generator = PythonGenerator()
    asyncio.run(main_entry_point(generator))
