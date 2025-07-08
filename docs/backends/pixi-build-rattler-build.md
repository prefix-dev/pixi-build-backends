# Rattler-Build Backend

The `pixi-build-rattler-build` backend enables building conda packages using rattler-build recipes.
This backend is designed for projects that either have existing recipe.yaml files or where customization is necessary that isn't possible with the currently available backends.

!!! warning
    `pixi-build` is a preview feature, and will change until it is stabilized.
    This is why we require users to opt in to that feature by adding "pixi-build" to `workspace.preview`.

    ```toml
    [workspace]
    preview = ["pixi-build"]
    ```


## Overview

The rattler-build backend:

- Uses existing `recipe.yaml` files as build manifests
- Supports all standard rattler-build recipe features and selectors
- Handles dependency resolution and virtual package detection automatically
- Can build multiple outputs from a single recipe

## Usage

To use the rattler-build backend in your `pixi.toml`, specify it in your build system configuration:

```toml
[package]
name = "rattler_build_package"
version = "0.1.0"

[package.build]
backend = { name = "pixi-build-rattler-build", version = "*" }
channels = [
  "https://prefix.dev/pixi-build-backends",
  "https://prefix.dev/conda-forge",
]
```

The backend expects a rattler-build recipe file in one of these locations (searched in order):

1. `recipe.yaml` or `recipe.yml` in the same directory as the package manifest
2. `recipe/recipe.yaml` or `recipe/recipe.yml` in a subdirectory of the package manifest

If the package is defined in the same location as the workspace, it is heavily encouraged to place the recipe file in its own directory `recipe`.
Learn more about the `rattler-build`, and its recipe format in its [high level overview](https://rattler.build/latest/highlevel).


## Configuration Options

The rattler-build backend supports the following TOML configuration options:

### `debug-dir`

- **Type**: `String` (path)
- **Default**: Not set

If specified, internal build state and debug information will be written to this directory. Useful for troubleshooting build issues.

```toml
[package.build.configuration]
debug-dir = "debug-output"
```

### `extra-input-globs`

- **Type**: `Array<String>`
- **Default**: `[]`

Additional glob patterns to include as input files for the build process. These patterns are added to the default input globs that are determined from the recipe sources and package directory structure.

```toml
[package.build.configuration]
extra-input-globs = [
    "patches/**/*",
    "scripts/*.sh",
    "*.md"
]
```

## Build Process

The rattler-build backend follows this build process:

1. **Recipe Discovery**: Locates the `recipe.yaml` file in standard locations
2. **Dependency Resolution**: Resolves build, host, and run dependencies from conda channels
3. **Virtual Package Detection**: Automatically detects system virtual packages
4. **Build Execution**: Runs the build script specified in the recipe
5. **Package Creation**: Creates conda packages according to the recipe specification


## Limitations

- Requires an existing rattler-build recipe file - cannot infer build instructions automatically
- Build configuration is primarily controlled through the recipe file rather than `pixi.toml`
- Cannot specify dependencies in the manifest — all dependencies are handled by the recipe
