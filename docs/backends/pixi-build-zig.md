# pixi-build-zig

The `pixi-build-zig` backend is designed for building Zig projects using the [Zig build system](https://ziglang.org/learn/build-system/). It provides seamless integration with Pixi's package management workflow while maintaining cross-platform compatibility.

!!! warning
    `pixi-build` is a preview feature, and will change until it is stabilized.
    This is why we require users to opt in to that feature by adding "pixi-build" to `workspace.preview`.

    ```toml
    [workspace]
    preview = ["pixi-build"]
    ```


## Overview

This backend automatically generates conda packages from Zig projects by:

- **Using Zig Build**: Leverages Zig's native build system for compilation and installation
- **Cross-platform support**: Works consistently across Linux, macOS, and Windows
- **Standard installation**: Uses `zig build --prefix` to install artifacts to the conda prefix
- **Flexible configuration**: Supports custom build arguments and environment variables

## Basic Usage

To use the Zig backend in your `pixi.toml`, add it to your package's build configuration:

```toml
[package]
name = "zig_package"
version = "0.1.0"

[package.build]
backend = { name = "pixi-build-zig", version = "*" }
channels = ["https://prefix.dev/conda-forge"]
```

### build.zig Requirements

Your Zig project must use `b.installArtifact()` in `build.zig` to mark which artifacts should be installed. The Zig build system will automatically place them in the correct locations:

- **Executables** → `$PREFIX/bin/`
- **Libraries** → `$PREFIX/lib/`
- **Headers** → `$PREFIX/include/`

Example `build.zig`:

```zig
pub fn build(b: *std.Build) void {
    const exe = b.addExecutable(.{
        .name = "my-tool",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
        }),
    });

    // This marks the executable for installation
    b.installArtifact(exe);
}
```

### Required Dependencies

The backend automatically includes the following build tools:

- `zig` - The Zig compiler and build system

The backend will automatically add `zig` to your build dependencies if it's not already specified. You can add it explicitly if you need a specific version:

```toml
[package.build-dependencies]
zig = ">=0.14.0,<0.14"
```

## Configuration Options

You can customize the Zig backend behavior using the `[package.build.config]` section in your `pixi.toml`. The backend supports the following configuration options:

### `extra-args`

- **Type**: `Array<String>`
- **Default**: `[]`
- **Target Merge Behavior**: `Overwrite` - Platform-specific arguments completely replace base arguments

Additional command-line arguments to pass to the `zig build` command. These arguments are appended to the build command.

```toml
[package.build.config]
extra-args = [
    "-Doptimize=ReleaseFast",
    "-Dtarget=x86_64-linux-gnu"
]
```

For target-specific configuration, platform arguments completely replace the base configuration:

```toml
[package.build.config]
extra-args = ["-Doptimize=Debug"]

[package.build.config.targets.linux-64]
extra-args = ["-Doptimize=ReleaseFast", "-Dtarget=x86_64-linux-gnu"]
# Result for linux-64: ["-Doptimize=ReleaseFast", "-Dtarget=x86_64-linux-gnu"]
```

### `env`

- **Type**: `Map<String, String>`
- **Default**: `{}`
- **Target Merge Behavior**: `Merge` - Platform environment variables override base variables with same name, others are merged

Environment variables to set during the build process. These variables are available during compilation.

```toml
[package.build.config]
env = { ZIG_GLOBAL_CACHE_DIR = "/tmp/zig-cache", CUSTOM_VAR = "value" }
```

For target-specific configuration, platform environment variables are merged with base variables:

```toml
[package.build.config]
env = { COMMON_VAR = "base", ZIG_LOCAL_CACHE_DIR = ".zig-cache" }

[package.build.config.targets.linux-64]
env = { COMMON_VAR = "linux", ZIG_SYSTEM_LINKER_HACK = "1" }
# Result for linux-64: { COMMON_VAR = "linux", ZIG_LOCAL_CACHE_DIR = ".zig-cache", ZIG_SYSTEM_LINKER_HACK = "1" }
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

Additional glob patterns to include as input files for the build process. These patterns are added to the default input globs that include Zig source files (`**/*.zig`), build configuration files (`build.zig`, `build.zig.zon`), and other build-related files.

```toml
[package.build.config]
extra-input-globs = [
    "assets/**/*",
    "data/*.json",
    "*.md"
]
```

For target-specific configuration, platform-specific globs completely replace the base:

```toml
[package.build.config]
extra-input-globs = ["*.txt"]

[package.build.config.targets.linux-64]
extra-input-globs = ["*.txt", "*.so", "linux-configs/**/*"]
# Result for linux-64: ["*.txt", "*.so", "linux-configs/**/*"]
```

## Build Process

The Zig backend follows this build process:

1. **Environment Setup**: Configures environment variables if specified in the configuration
2. **Build and Install**: Executes `zig build` with the following behavior:
   - `--prefix $PREFIX` (Unix/macOS) or `--prefix %LIBRARY_PREFIX%` (Windows): Install to the correct conda package location
   - Additional arguments from `extra-args` configuration
3. **Artifact Installation**: The Zig build system automatically places installed artifacts in the correct subdirectories based on their type

### Windows Considerations

On Windows, the backend uses `%LIBRARY_PREFIX%` instead of `%PREFIX%` to follow conda's convention for Unix-style packages. This means your artifacts will be installed to:

- **Executables** → `%LIBRARY_PREFIX%\bin\` (i.e., `%PREFIX%\Library\bin\`)
- **Libraries** → `%LIBRARY_PREFIX%\lib\` (i.e., `%PREFIX%\Library\lib\`)
- **Headers** → `%LIBRARY_PREFIX%\include\` (i.e., `%PREFIX%\Library\include\`)

## Accessing Host Dependencies

If your Zig project depends on C libraries from conda (e.g., SDL3, OpenSSL), you need to configure `build.zig` to find them. During the build, dependencies are available in the `$PREFIX` environment variable:

```zig
fn getLibraryAndIncludePath(b: *std.Build) ?EnvPaths {
    // During build (via pixi-build-zig backend), PREFIX points to host dependencies
    // During development (via pixi shell), CONDA_PREFIX points to the environment
    const env_var = if (std.process.getEnvVarOwned(b.allocator, "PREFIX")) |prefix|
        prefix
    else |_| blk: {
        break :blk std.process.getEnvVarOwned(b.allocator, "CONDA_PREFIX") catch {
            return null;
        };
    };
    defer b.allocator.free(env_var);

    const include_subdir = if (builtin.os.tag == .windows) "Library/include" else "include";
    const lib_subdir = if (builtin.os.tag == .windows) "Library/lib" else "lib";

    const include_path = std.fs.path.join(b.allocator, &.{ env_var, include_subdir }) catch return null;
    const lib_path = std.fs.path.join(b.allocator, &.{ env_var, lib_subdir }) catch return null;

    return EnvPaths{
        .includePath = include_path,
        .libraryPath = lib_path,
    };
}
```

## Example Configuration

Here's a complete example for a Zig project with SDL3:

```toml
[package]
name = "zig-sdl-game"
version = "0.1.0"

[package.build]
backend = { name = "pixi-build-zig", version = "*" }
channels = ["https://prefix.dev/conda-forge"]

[package.build.config]
extra-args = ["-Doptimize=ReleaseFast"]

[package.host-dependencies]
sdl3 = ">=3.0.0,<4"

[package.run-dependencies]
sdl3 = ">=3.0.0,<4"
```

## Limitations

- Requires Zig projects to use `b.installArtifact()` in `build.zig` (standard practice)
- No automatic metadata extraction from `build.zig.zon` (must be specified in `pixi.toml`)
- The backend runs `zig build` from the project root directory

## See Also

- [Zig Build System Documentation](https://ziglang.org/learn/build-system/) - Official Zig build system guide
- [Zig Documentation](https://ziglang.org/documentation/master/) - Official Zig documentation
- [zig.guide](https://zig.guide/) - Community-maintained Zig learning resource
