# pixi-build-python

The `pixi-build-python` backend is designed for building Python projects using standard Python packaging tools. It provides seamless integration with Pixi's package management workflow while supporting both [PEP 517](https://peps.python.org/pep-0517/) and [PEP 518](https://peps.python.org/pep-0518/) compliant projects.

!!! warning
    `pixi-build` is a preview feature, and will change until it is stabilized.
    This is why we require users to opt in to that feature by adding "pixi-build" to `workspace.preview`.

    ```toml
    [workspace]
    preview = ["pixi-build"]
    ```


## Overview

This backend automatically generates conda packages from Python projects by:

- **PEP 517/518 compliance**: Works with modern Python packaging standards including `pyproject.toml`
- **Cross-platform support**: Works consistently across Linux, macOS, and Windows
- **Flexible installation**: Automatically selects between `pip` and `uv` for package installation

## Basic Usage

To use the Python backend in your `pixi.toml`, add it to your package's build configuration:

```toml
[package]
name = "python_package"
version = "0.1.0"

[package.build]
backend = { name = "pixi-build-python", version = "*" }
channels = [
  "https://prefix.dev/pixi-build-backends",
  "https://prefix.dev/conda-forge",
]
```

### Required Dependencies

The backend automatically includes the following build tools:

- `python` - The Python interpreter
- `pip` - Python package installer (or `uv` if specified)

You can add these to your [`host-dependencies`](https://pixi.sh/latest/build/dependency_types/) if you need specific versions:

```toml
[package.build-dependencies]
python = "3.11"
```

You'll also need to specify your Python build backend (like `hatchling`, `setuptools`, etc.) in your `package.host-dependencies`:

```toml
[package.host-dependencies]
hatchling = "*"
```

## Configuration Options

You can customize the Python backend behavior using the `[package.build.configuration]` section in your `pixi.toml`. The backend supports the following configuration options:

### `noarch`

- **Type**: `Boolean`
- **Default**: `true`

Controls whether to build a platform-independent (noarch) package or a platform-specific package. Most pure Python packages should be `noarch` and therefore don't need to set this option since the default is `noarch = true`.

```toml
[package.build.configuration]
noarch = false  # Build platform-specific package
```

### `env`

- **Type**: `Map<String, String>`
- **Default**: `{}`

Environment variables to set during the build process. These variables are available during package installation.

```toml
[package.build.configuration]
env = { SETUPTOOLS_SCM_PRETEND_VERSION = "1.0.0" }
```

### `debug-dir`

- **Type**: `String` (path)
- **Default**: Not set

If specified, internal build state and debug information will be written to this directory. Useful for troubleshooting build issues.

```toml
[package.build.configuration]
debug-dir = ".build-debug"
```

### `extra-input-globs`

- **Type**: `Array<String>`
- **Default**: `[]`

Additional glob patterns to include as input files for the build process. These patterns are added to the default input globs that include Python source files, configuration files (`setup.py`, `pyproject.toml`, etc.), and other build-related files.

```toml
[package.build.configuration]
extra-input-globs = [
    "data/**/*",
    "templates/*.html",
    "*.md"
]
```

## Build Process

The Python backend follows this build process:

1. **Installer Detection**: Automatically chooses between `uv` and `pip` based on available dependencies
2. **Environment Setup**: Configures Python environment variables for the build
3. **Package Installation**: Executes the selected installer with the following options:
   - `--no-deps`: Don't install dependencies (handled by conda)
   - `--no-build-isolation`: Use the conda environment for building
   - `-vv`: Verbose output for debugging
4. **Package Creation**: Creates either a noarch or platform-specific conda package

## Installer Selection

The backend automatically detects which Python installer to use:

- **uv**: Used if `uv` is present in any dependency category (build, host, or run)
- **pip**: Used as the default fallback installer

To use `uv` for faster installations, add it to your dependencies:

```toml
[package.host-dependencies]
uv = "*"
```

# Editable Installations

Until profiles are implemented, editable installations are not easily configurable.
This is the current behaviour:

- `editable` is `true` when installing the package (e.g. with `pixi install`)
- `editable` is `false` when building the package (e.g. with `pixi build`)
- Set environment variable `BUILD_EDITABLE_PYTHON` to `true` or `false` to enforce a certain behavior

## Limitations

- Requires a PEP 517/518 compliant Python project with `pyproject.toml`
- Limited support for complex build customization compared to direct recipe-based approaches
- Limited ways to configure editable installations


## See Also

- [Building Python Packages](https://pixi.sh/latest/build/python/) - Tutorial for building Python packages with Pixi
- [Python Packaging User Guide](https://packaging.python.org/) - Official Python packaging documentation
- [PEP 517](https://peps.python.org/pep-0517/) - A build-system independent format for source trees
