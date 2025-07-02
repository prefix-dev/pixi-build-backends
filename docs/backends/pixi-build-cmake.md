# pixi-build-cmake

The `pixi-build-cmake` backend is designed for building C and C++ projects using the [CMake](https://cmake.org/) build system. It provides seamless integration with Pixi's package management workflow while maintaining cross-platform compatibility.

!!! warning
    `pixi-build` is a preview feature, and will change until it is stabilized.
    This is why we require users to opt in to that feature by adding "pixi-build" to `workspace.preview`.

    ```toml
    [workspace]
    preview = ["pixi-build"]
    ```


## Overview

This backend automatically generates conda packages from CMake-based projects by:

- **Detecting and configuring compilers**: Automatically includes the appropriate C/C++ compilers for your target platform
- **Building with Ninja**: Uses the fast Ninja build system for optimal build performance
- **Cross-platform support**: Works consistently across Linux, macOS, and Windows
- **Standard CMake workflow**: Follows CMake best practices with sensible defaults

## Basic Usage

To use the CMake backend in your `pixi.toml`, add it to your package's build configuration:

```toml
[package]
name = "cmake_package"
version = "0.1.0"

[package.build]
backend = { name = "pixi-build-cmake", version = "*" }
channels = [
  "https://prefix.dev/pixi-build-backends",
  "https://prefix.dev/conda-forge",
]
```

### Required Dependencies

The backend automatically includes the following build tools:

- `cmake` - The CMake build system
- `ninja` - Fast build system used by CMake
- Platform-specific C++ compilers (e.g., `gcc_linux-64`, `clang_osx-64`)

You can add these to your [`build-dependencies`](https://pixi.sh/latest/build/dependency_types/) if you need specific versions:

```toml
[package.build-dependencies]
ninja = "1.13"
```

## Configuration Options

You can customize the CMake backend behavior using the `[package.build.configuration]` section in your `pixi.toml`. The backend supports the following configuration options:

### `extra-args`

- **Type**: `Array<String>`
- **Default**: `[]`

Additional command-line arguments to pass to the CMake configuration step. These arguments are inserted into the `cmake` command that configures your project.

```toml
[package.build.configuration]
extra-args = [
    "-DENABLE_TESTING=ON",
    "-DCMAKE_CXX_STANDARD=17"
]
```

### `env`

- **Type**: `Map<String, String>`
- **Default**: `{}`

Environment variables to set during the build process. These variables are available to both the CMake configuration and build steps.

```toml
[package.build.configuration]
env = { CMAKE_VERBOSE_MAKEFILE = "ON", CXXFLAGS = "-O3 -march=native" }
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

Additional glob patterns to include as input files for the build process. These patterns are added to the default input globs that include source files (`**/*.{c,cc,cxx,cpp,h,hpp,hxx}`), CMake files (`**/*.{cmake,cmake.in}`, `**/CMakeFiles.txt`), and other build-related files.

```toml
[package.build.configuration]
extra-input-globs = [
    "assets/**/*",
    "config/*.ini",
    "*.md"
]
```

## Build Process

The CMake backend follows this build process:

1. **Version Detection**: Displays CMake and Ninja versions for diagnostics
2. **Configuration**: Runs `cmake` with the following default options:
   - `-GNinja`: Use Ninja generator
   - `-DCMAKE_BUILD_TYPE=Release`: Release build by default
   - `-DCMAKE_INSTALL_PREFIX=$PREFIX`: Install to conda prefix
   - `-DCMAKE_EXPORT_COMPILE_COMMANDS=ON`: Export compile commands for tooling
   - `-DBUILD_SHARED_LIBS=ON`: Build shared libraries by default
   - `-DPython_EXECUTABLE=$PYTHON`: Use the conda Python executable if it's part of the host dependencies.
3. **Build**: Executes `cmake --build` to compile the project
4. **Install**: Installs the built artifacts to the conda package

## CMake Flag Precedence

With CMake, when duplicate flags are provided, the last flag takes precedence.
The `pixi-build-cmake` backend places `extra-args` after the default CMake flags, allowing you to override default settings.

For example, to switch from the default Release build to Debug mode:

```toml
[package.build.configuration]
extra-args = ["-DCMAKE_BUILD_TYPE=Debug"]
```


## Limitations

- Currently, assumes C++ projects (hardcoded to `cxx` language)
- Language detection from CMakeLists.txt is not yet implemented

## See Also

- [Building C++ Packages](https://pixi.sh/latest/build/cpp/) - Tutorial for building C++ packages with Pixi
- [CMake Documentation](https://cmake.org/documentation/) - Official CMake documentation
