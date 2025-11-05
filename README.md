# Pixi Build Backends

**Work in Progress: Backend Implementations for Building Pixi Projects from Source**

This repository contains backend implementations designed to facilitate the building of pixi projects directly from their source code. These backends aim to enhance the functionality of Pixi, a cross-platform, multi-language package manager and workflow tool built on the foundation of the conda ecosystem.

## Available Build Backends
The idea is that each backend should be able to deal with one kind of build system.
The repository provides the following build backends:

1. **pixi-build-python**: A backend tailored for building Python-based projects.
2. **pixi-build-cmake**: A backend designed for projects utilizing CMake as their build system.
3. **pixi-build-rattler-build**: A backend for building [`recipe.yaml`](https://rattler.build/latest/) directly
4. **pixi-build-rust**: A backend for building Rust projects.


These backends are located in the `crates/*` directory of the repository.

## Features
* **Backend Implementations**: Provides the necessary components to build Pixi projects from source, integrating seamlessly with the Pixi ecosystem.
* **Schema Definitions**: Includes schema definitions to standardize and validate project configurations.

## Getting Started

For example, this `build-section` will use the python backend to build a python project:

```toml
[build-system]
# The name of the build backend to use. This name refers both to the name of
# the package that provides the build backend and the name of the executable
# inside the package that is invoked.
#
# The `build-backend` key also functions as a dependency declaration. At least
# a version specifier must be added.
build-backend = { name = "pixi-build-python", version = "*" }
# These are the conda channels that are used to resolve the dependencies of the
# build backend package.
channels = ["https://prefix.dev/conda-forge"]
```


### Developing on Backends

Even though binary versions are available on the prefix channels and conda-forge, its also quite easy to get started on developing a new backend or work on an existing one.
To start development make sure you have installed [pixi](https://pixi.sh). After which, a number of command should be available:

```bash
# To build the backends
pixi run build
# .. to install a backend, for example the python one:
pixi r install-pixi-build-python
```

You can make use of these backends to overwrite any existing backend in pixi.
This is described in the [pixi docs](https://pixi.sh/dev/build/backends/)

## Contributing
Contributions are welcome! Please refer to the contributing guidelines for more information.
License

This project is licensed under the BSD-3-Clause License. See the LICENSE file for details.
Acknowledgements

## Acknowledgemts
Developed by prefix.dev.
For more information about Pixi and related projects, visit the [prefix-dev](https://github.com/prefix-dev) organization on GitHub.
