# pixi-build-ros

The `pixi-build-ros` backend is designed for building [ROS (Robot Operating System)](https://www.ros.org/) packages using the native ROS build systems.
No more requirement to use `colcon` or `catkin_tools` to build your ROS packages.
It provides seamless integration with Pixi's package management workflow while supporting ROS1 and ROS2 packages with automatic dependency resolution.

!!! warning
    `pixi-build` is a preview feature, and will change until it is stabilized.
    This is why we require users to opt in to that feature by adding "pixi-build" to `workspace.preview`.

    ```toml
    [workspace]
    preview = ["pixi-build"]
    ```

## Overview

This backend automatically generates conda packages from ROS projects by:

- **package.xml Integration**: Automatically reads package metadata (name, version, description, maintainers, dependencies) from your ROS `package.xml` file
- **Multi-build system support**: Supports ament_cmake, ament_python, catkin, and cmake build types
- **ROS Distribution Support**: Works with both ROS1 and ROS2 distributions (noetic, humble, jazzy, etc.)
- **Cross-platform support**: Supports Linux, macOS and Windows
- **Automatic dependency mapping**: Maps ROS dependencies to conda packages using RoboStack mappings

## Basic Usage

To use the ROS backend in your `pixi.toml`, add it to your package's build configuration:

```toml
[workspace]
preview = ["pixi-build"]
channels = [
    "https://prefix.dev/pixi-build-backends",
    "https://prefix.dev/robostack-jazzy",  # or robostack-humble, robostack-noetic, etc.
    "https://prefix.dev/conda-forge"
]
platforms = ["linux-64", "osx-arm64"]

[package.build]
backend = { name = "pixi-build-ros", version = "*" }

[package.build.configuration]
distro = "jazzy"  # or "humble", "noetic", etc.
```

??? Note "Workspace Configuration"
    The workspace can be defined in the `pixi.toml` of the package or in a separate `pixi.toml` at the workspace root.
    For example, with a workspace structure like this:
    ```shell
    tree -L 2
    .
    ├── pixi.toml  # Workspace configuration
    └── src
        └── my_ros_package
            ├── package.xml  # ROS package manifest
            └── pixi.toml  # Package configuration
    ```

Then you can run `pixi build` to create conda packages for your ROS packages.
```shell
pixi build
```

When you want to install it into your environment, you can do so by adding the following to your workspace `pixi.toml`:

```toml
[dependencies]
ros-jazzy-my-ros-package = { path = "." }
# or if the package is in a separate pixi.toml
# ros-jazzy-my-ros-package = { path = "src/my_ros_package" }
```
Note that you need to specify the `ros-jazzy-` prefix when you use a distro configuration. 


### Automatic Metadata Detection

The backend will automatically read metadata from your `package.xml` file to populate package information **that is not** explicitly defined in your `pixi.toml`.
This includes:

- **Package name and version**: Automatically used if not specified in `pixi.toml`
- **Description**: Uses the description from `package.xml`
- **Maintainers**: Extracted from maintainer fields in `package.xml`
- **Homepage**: From URL fields with type "website" in `package.xml`
- **Repository**: From URL fields with type "repository" in `package.xml`

For example, if your `package.xml` contains:

```xml
<package format="3">
  <name>my_ros_package</name>
  <version>1.0.0</version>
  <description>A useful ROS package for navigation</description>
  <maintainer email="developer@example.com">John Doe</maintainer>
  <url type="website">https://github.com/user/my_ros_package</url>
  <url type="repository">https://github.com/user/my_ros_package</url>
</package>
```

It would be equivalent to the following `pixi.toml`:

```toml
[package]
name = "my_ros_package"
version = "1.0.0"
description = "A useful ROS package for navigation"
maintainers = ["John Doe <developer@@example.com"]
homepage = "https://github.com/user/my_ros_package"
repository = "https://github.com/user/my_ros_package"
```

The backend will automatically use the metadata from `package.xml` to generate a complete conda package named `ros-jazzy-my-ros-package`.
The fields in the `pixi.toml` will override the values from `package.xml` if they are explicitly set.

## Configuration Options

You can customize the ROS backend behavior using the `[package.build.config]` section in your `pixi.toml`. The backend supports the following configuration options:

### `distro` (Required)

- **Type**: `String`
- **Default**: Not set (required)
- **Target Merge Behavior**: `Overwrite` - Platform-specific distro takes precedence over base

The ROS distribution to build for. This affects dependency mapping and build configuration.
If set the package name will be prefixed with `ros-<distro>-` automatically, otherwise the package name from `pixi.toml` or `package.xml` is used.

```toml
[package.build.config]
distro = "jazzy"  # or "humble", "noetic", "iron", etc.
```

### `env`

- **Type**: `Map<String, String>`
- **Default**: `{}`
- **Target Merge Behavior**: `Merge` - Platform environment variables override base variables with same name, others are merged

Environment variables to set during the build process. These variables are available during compilation.

```toml
[package.build.config]
env = { ROS_VERSION = "2", AMENT_CMAKE_ENVIRONMENT_HOOKS_ENABLED = "1" }
```

### `debug-dir`

- **Type**: `String` (path)
- **Default**: Not set
- **Target Merge Behavior**: Not allowed - Cannot have target specific value

If specified, internal build state and debug information will be written to this directory. Useful for troubleshooting build issues.

```toml
[package.build.config]
debug-dir = ".build-debug"
```

### `extra-input-globs`

- **Type**: `Array<String>`
- **Default**: `[]`
- **Target Merge Behavior**: `Overwrite` - Platform-specific globs completely replace base globs

Additional glob patterns to include as input files for the build process. These patterns are added to the default input globs that include ROS-specific files.

```toml
[package.build.config]
extra-input-globs = [
    "launch/**/*.py",
    "config/*.yaml",
    "msgs/**/*.msg",
    "srvs/**/*.srv"
]
```

Default input globs include:
- Source files: `**/*.{c,cpp,h,hpp,rs,sh,py,pyx}`
- ROS configuration: `package.xml`, `setup.py`, `setup.cfg`, `pyproject.toml`
- Build files: `CMakeLists.txt`

## Build Process

The ROS backend follows this build process:

1. **Package Detection**: Parses `package.xml` to determine build type (`ament_cmake`, `ament_python`, `catkin`)
2. **Dependency Resolution**: Maps ROS dependencies to conda packages using RoboStack mappings
3. **Environment Setup**: Configures ROS-specific environment variables
4. **Build Execution**: Uses the appropriate build template based on package type.
5. **Installation**: Installs built artifacts to the conda package prefix

## ROS Package Types

The backend supports different ROS package build types:

### ament_cmake (ROS2)
For C++ packages using ament build system:
```xml
<export>
  <build_type>ament_cmake</build_type>
</export>
```

### ament_python (ROS2)
For Python packages using ament build system:
```xml
<export>
  <build_type>ament_python</build_type>
</export>
```

### catkin (ROS1)
For ROS1 packages using catkin build system:
```xml
<export>
  <build_type>catkin</build_type>
</export>
```

## Dependency Mapping

The backend automatically maps ROS dependencies to conda packages using the RoboStack project mappings. Dependencies in `package.xml` are converted as follows:

- **ROS packages**: Mapped to `ros-<distro>-<package-name>` format
- **Known dependencies**: Mapped using the `robostack.yaml` configuration file.
- **Unknown dependencies**: Passed through as-is.

Example dependency mapping:

- `ament_cmake` → `ros-jazzy-ament-cmake`
- `std_msgs` → `ros-jazzy-std-msgs`
- `opencv2` → `libopencv` (via `robostack.yaml` mapping)

## Limitations

- **Version constraints**: Dependency versions from `package.xml` are currently ignored
- **Conditional dependencies**: Dependencies with conditions are not fully supported yet
- **Target-specific dependencies**: Platform-specific dependencies in `package.xml` need manual handling

## See Also

- [ROS Documentation](https://docs.ros.org/) - Official ROS documentation
- [RoboStack](https://robostack.github.io/) - Conda packages for the Robot Operating System
- [ament Build System](https://docs.ros.org/en/rolling/Concepts/Build-System-Development/ament.html) - ROS2 build system
- [catkin Build System](http://wiki.ros.org/catkin) - ROS1 build system