import asyncio
from .python_generator import PythonGenerator
from pixi_build_backend.main import run_backend, run_backend_sync


def main():
    """Main entry point for the script."""
    generator = PythonGenerator()
    run_backend_sync(generator)
