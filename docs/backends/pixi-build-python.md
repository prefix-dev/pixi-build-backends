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
channels = ["https://prefix.dev/conda-forge"]

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
- **Default**: `true` (unless [compilers](#compilers) are specified)
- **Target Merge Behavior**: `Overwrite` - Platform-specific noarch setting takes precedence over base

Controls whether to build a platform-independent (noarch) package or a platform-specific package.
The backend tries to derive whether the package can be built as `noarch` based on the presence of [compilers](#compilers).
If compilers are specified, the backend assume that native extensions are build as part of the build process.
Most of the time these are platform-specific, so the package will be built as a platform-specific package.
If no compilers are specified, the default value for `noarch` is `true`, meaning the package will be built as a noarch python package.

```toml
[package.build.configuration]
noarch = false  # Build platform-specific package
```

For target-specific configuration, platform-specific noarch setting overrides the base:

```toml
[package.build.configuration]
noarch = true

[package.build.configuration.targets.win-64]
noarch = false  # Windows needs platform build
# Result for win-64: false
```

### `env`

- **Type**: `Map<String, String>`
- **Default**: `{}`
- **Target Merge Behavior**: `Merge` - Platform environment variables override base variables with same name, others are merged

Environment variables to set during the build process. These variables are available during package installation.

```toml
[package.build.configuration]
env = { SETUPTOOLS_SCM_PRETEND_VERSION = "1.0.0" }
```

For target-specific configuration, platform environment variables are merged with base variables:

```toml
[package.build.configuration]
env = { PYTHONPATH = "/base/path", COMMON_VAR = "base" }

[package.build.configuration.targets.win-64]
env = { COMMON_VAR = "windows", WIN_SPECIFIC = "value" }
# Result for win-64: { PYTHONPATH = "/base/path", COMMON_VAR = "windows", WIN_SPECIFIC = "value" }
```

### `debug-dir`

- **Type**: `String` (path)
- **Default**: Not set
- **Target Merge Behavior**: Not allowed - Cannot have target specific value

If specified, internal build state and debug information will be written to this directory. Useful for troubleshooting build issues.

```toml
[package.build.configuration]
debug-dir = ".build-debug"
```

### `extra-input-globs`

- **Type**: `Array<String>`
- **Default**: `[]`
- **Target Merge Behavior**: `Overwrite` - Platform-specific globs completely replace base globs

Additional glob patterns to include as input files for the build process. These patterns are added to the default input globs that include Python source files, configuration files (`setup.py`, `pyproject.toml`, etc.), and other build-related files.

```toml
[package.build.configuration]
extra-input-globs = [
    "data/**/*",
    "templates/*.html",
    "*.md"
]
```

For target-specific configuration, platform-specific globs completely replace the base:

```toml
[package.build.configuration]
extra-input-globs = ["*.py"]

[package.build.configuration.targets.win-64]
extra-input-globs = ["*.py", "*.dll", "*.pyd", "windows-resources/**/*"]
# Result for win-64: ["*.py", "*.dll", "*.pyd", "windows-resources/**/*"]
```

### `compilers`

- **Type**: `Array<String>`
- **Default**: `[]` (no compilers)
- **Target Merge Behavior**: `Overwrite` - Platform-specific compilers completely replace base compilers

List of compilers to use for the build. Most pure Python packages don't need compilers, but this is useful for packages with C extensions or other compiled components. The backend automatically generates appropriate compiler dependencies using conda-forge's compiler infrastructure.

```toml
[package.build.configuration]
compilers = ["c", "cxx"]
```

For target-specific configuration, platform compilers completely replace the base configuration:

```toml
[package.build.configuration]
compilers = []

[package.build.configuration.targets.win-64]
compilers = ["c", "cxx"]
# Result for win-64: ["c", "cxx"] (only on Windows)
```

!!! info "Pure Python vs. Extension Packages"
    The Python backend defaults to no compilers (`[]`) since most Python packages are pure Python and don't need compilation. This is different from other backends like CMake which default to `["cxx"]`. Only specify compilers if your package has C extensions or other compiled components:

    ```toml
    # Pure Python package (default behavior)
    [package.build.configuration]
    # No compilers needed - defaults to []

    # Python package with C extensions
    [package.build.configuration]
    compilers = ["c", "cxx"]
    ```

!!! info "Comprehensive Compiler Documentation"
    For detailed information about available compilers, platform-specific behavior, and how conda-forge compilers work, see the [Compilers Documentation](../key_concepts/compilers.md).

### `extra-args`

- **Type**: `Array<String>`
- **Default**: `[]`
- **Target Merge Behavior**: `Overwrite` - Platform-specific globs completely replace base globs

Extra arguments to pass to `pip`.
A use-case could be [`pip`'s `--config-settings` parameter](https://pip.pypa.io/en/stable/cli/pip_install/#cmdoption-C).

```toml
[package.build.config]
extra-args = ["-Cbuilddir=mybuilddir"]
```

For target-specific configuration, platform-specific globs completely replace the base:

```toml
[package.build.config]
extra-args = ["-Cbuilddir=mybuilddir"]

[package.build.config.targets.win-64]
extra-args = ["-Cbuilddir=foo"]
# Result for win-64: ["-Cbuilddir=foo"]
```

### `ignore-pyproject-manifest`

- **Type**: `Boolean`
- **Default**: `false`
- **Target Merge Behavior**: `Overwrite` - Platform-specific setting takes precedence over base

Controls whether to ignore the `pyproject.toml` manifest file and rely solely on the project model for package metadata. When set to `true`, the backend will not extract metadata (name, version, description, license, URLs) from `pyproject.toml` and will use only the information provided in the Pixi project model.

```toml
[package.build.configuration]
ignore-pyproject-manifest = true  # Ignore pyproject.toml metadata
```

This option is useful when you want complete control over package metadata through the Pixi project configuration, or when the `pyproject.toml` contains metadata that conflicts with your conda package requirements.

For target-specific configuration, platform-specific setting overrides the base:

```toml
[package.build.configuration]
ignore-pyproject-manifest = false

[package.build.configuration.targets.win-64]
ignore-pyproject-manifest = true  # Ignore pyproject.toml on Windows only
# Result for win-64: true
```

!!! info "Metadata Extraction from pyproject.toml"
    By default (when `ignore-pyproject-manifest` is `false`), the backend automatically extracts package metadata from your `pyproject.toml` file, including:

    - **name**: Package name from `project.name`
    - **version**: Package version from `project.version`
    - **description/summary**: From `project.description`
    - **license**: From `project.license` (supports text, file, or SPDX formats)
    - **homepage**: From `project.urls.Homepage`
    - **repository**: From `project.urls.Repository`, `project.urls.Source`, or `project.urls."Source Code"`
    - **documentation**: From `project.urls.Documentation` or `project.urls.Docs`

    This metadata is automatically included in the generated conda recipe. The `pyproject.toml` file itself is also added to the input globs for incremental build detection.

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
