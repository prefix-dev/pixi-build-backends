#!/usr/bin/env python3
# """
# Example usage of py_pixi_build_backend Python bindings.
# """

# try:
#     from py_pixi_build_backend import (
#         PyPythonBackend,
#         PyProjectModel,
#         PyPlatform,
#         PyPythonParams
#     )
import asyncio
from pixi_build_backend.pixi_build_backend import (
    PyBackendConfig,
    PyProjectModelV1,
    PyGeneratedRecipe,
    PyGenerateRecipe,
    PyPlatform,
    py_main
)

# from rattler.platform import Platform



class BackendConfig:
    """
    Example configuration class for the backend.
    This should implement the BackendConfig trait.
    """

    def __init__(self, debug_dir=None):
        self._python_backend = PyBackendConfig(debug_dir=debug_dir)

        # self._debug_dir = debug_dir

    def debug_dir(self):
        return self._python_backend.debug_dir()



class ProjectModelV1:
    """
    Example project model class.
    This should implement the ProjectModelV1 trait.
    """

    def __init__(self, name, version):
        self._project_model = PyProjectModelV1(name=name, version=version)

    def name(self):
        return self._project_model.name()

    def version(self):
        return self._project_model.version()



# Example usage of the BackendConfig
config = BackendConfig(debug_dir="/path/to/debug/dir")

# Accessing the debug directory
print("Debug Directory:", config.debug_dir())

class Platform:
    """
    Example class for platform handling.
    This should implement the Platform trait.
    """

    def __init__(self, value):
        self._inner = PyPlatform(value)

    @classmethod
    def current(cls):
        """
        Returns the current platform.
        """
        return cls._from_py_platform(PyPlatform.current())

    @classmethod
    def _from_py_platform(cls, py_platform):
        """Construct Rattler version from FFI PyArch object."""
        return cls(py_platform.name)



class GeneratedRecipe:
    """
    Example class for generating recipes.
    This should implement the GeneratedRecipe trait.
    """

    def __init__(self):
        self._py_generated_recipe = PyGeneratedRecipe()


class MyGenerator:
    def generate_recipe(self, *args):
        """
        Generate a recipe based on the project model and platform.
        """
        print("Generating recipe!!!!")
        g = GeneratedRecipe()
        return g._py_generated_recipe




async def backend_main(generator):
    """
    Main function to run the backend with the provided generator.
    """
    print("Running backend main with generator:", generator)
    # Here you would typically call the Rust backend main function
    # For this example, we just simulate it
    return await py_main(generator, sys.argv[1:])




own_generator = PyGenerateRecipe(MyGenerator())

# Test the generator with sample data
if __name__ == "__main__":
    # Create test project model
    project = ProjectModelV1(name="test-project", version="1.0.0")
    
    # Create test platform
    platform = Platform.current()
    
    # Test the generate_recipe function
    print("Testing generate_recipe function...")
    result = own_generator.generate_recipe(project._project_model, config._python_backend, "some_path", platform._inner,  None)
    print(f"Generated recipe result: {result}")
    print(f"Result type: {type(result)}")


    print("Running backend main...")

    # very stinky bob hack to remove the script name from sys.argv
    import sys;
    

    print("sys.argv after pop:", sys.argv)

    # Run the backend main function with the generator
    backend_result = asyncio.run(backend_main(own_generator))
