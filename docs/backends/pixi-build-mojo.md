# pixi-build-mojo

The `pixi-build-mojo` backend is designed for building Mojo projects. It provides seamless integration with Pixi's package management workflow.

!!! warning
    `pixi-build` is a preview feature, and will change until it is stabilized.
    This is why we require users to opt in to that feature by adding "pixi-build" to `workspace.preview`.

    ```toml
    [workspace]
    preview = ["pixi-build"]
    ```

## Overview

This backend automatically generates conda packages from Mojo projects.

The generated packages can be installed into local envs for devlopment, or packaged for distribution.

## Basic Usage

To use the Mojo backend in your `pixi.toml`, add it to your package's build configuration:


```txt
# Example project layout for combined binary/library.
.
├── greetings
│   ├── __init__.mojo
│   └── lib.mojo
├── main.mojo
├── pixi.lock
├── pixi.toml
├── README.md
├── src
└── structure.txt
```

Commented out sections represent optional params that may be useful.

```toml
[workspace]
authors = ["J. Doe <john@mail.com>"]
platforms = ["linux-64"]
preview = ["pixi-build"]
channels = [
    "conda-forge",
    "https://conda.modular.com/max-nightly",
    "https://prefix.dev/pixi-build-backends",
    "https://repo.prefix.dev/modular-community"
]

[package]
name = "greetings"
version = "0.1.0"

[package.build]
backend = { name = "pixi-build-mojo", version = "0.1.*" }

[tasks]

[package.build.configuration]
# dist-dir = "./target"

[[package.build.configuration.bins]]
name = "greet"
path = "./main.mojo"
# extra-args = ["-I", "special-thing"]
# extra-input-globs = ["**/.c"]

[package.build.configuration.pkg]
name = "greetings"
path = "greetings"
# extra-args = ["-I", "special-thing"]
# extra-input-globs = ["**/.c"]

[package.host-dependencies]
max = "=25.4.0"

[package.build-dependencies]
max = "=25.4.0"
small_time = ">=25.4.1,<26"
extramojo = ">=0.16.0,<0.17"

[package.run-dependencies]
max = "=25.4.0"

[dependencies]
# For running `mojo test` while developing add all dependencies under
# `[package.build-dependencies]` here as well.
greetings = { path = "." }
```

### Required Dependencies

- `max` package for both the compiler and linked runtime

## Configuration Options

You can customize the Mojo backend behavior using the `[package.build.configuration]` section in your `pixi.toml`. The backend supports the following configuration options:

#### `env`

- **Type**: `Map<String, String>`
- **Default**: `{}`

Environment variables to set during the build process.

```toml
[package.build.configuration]
env = { ASSERT = "all" }
```

#### `dist-dir`

- **Type**: `String` (path)
- **Default**: Not set

A directory to create relative to the manifest pixi.toml to copy build artifacts into. `pixi install` will install the artifacts into your local enviroment and as such the `dist-dir` is mostly for debugging.

```toml
[package.build.configuration]
dist-dir = "./target"
```

#### `debug-dir`

- **Type**: `String` (path)
- **Default**: Not set

Directory to place internal pixi debug information into.

```toml
[package.build.configuration]
debug-dir = ".build-debug"
```

#### `extra-input-globs`

- **Type**: `Array<String>`
- **Default**: `[]`

Additional globs to pass to pixi for including more than just mojo files in the build.

```toml
[package.build.configuration]
extra-input-globs = ["**/*.c", "assets/**/*", "*.md"]
```

### `bins`

- **Type**: `Array<BinConfig>`
- **Default**: Not set

List of binary configurations to build. The created binary will be placed in the `$PREFIX/bin` dir and will be in the path after running `pixi install` assuming the package is listed as a dependency as in the example above. `pixi build` will create a conda package that includes the binary.

#### `bins[].name`

- **Type**: `String`
- **Default**: Required field (no default)

The name of the binary executable to create.

```toml
[[package.build.configuration.bins]]
name = "greet"
```

#### `bins[].path`

- **Type**: `String` (path)
- **Default**: Required field (no default)

The path to the Mojo file that contains a `main` function.

```toml
[[package.build.configuration.bins]]
path = "./main.mojo"
```

#### `bins[].extra-args`

- **Type**: `Array<String>`
- **Default**: `[]`

Additional command-line arguments to pass to the Mojo compiler when building this binary.

```toml
[[package.build.configuration.bins]]
extra-args = ["-I", "special-thing"]
```

### `pkg`

- **Type**: `PkgConfig`
- **Default**: Not set

Package configuration for creating Mojo package. The created Mojo package will be placed in the `$PREFIX/lib/mojo` dir, which will make it discoverable to anything that depends on the package.

#### `pkg.name`

- **Type**: `String`
- **Default**: Required field (no default)

The name to give the Mojo package. The `.mojopkg` suffix will be added automatically.

```toml
[package.build.configuration.pkg]
name = "greetings"
```

#### `pkg.path`

- **Type**: `String` (path)
- **Default**: Required field (no default)

The path to the directory that constitutes the package.

```toml
[package.build.configuration.pkg]
path = "greetings"
```

#### `pkg.extra-args`

- **Type**: `Array<String>`
- **Default**: `[]`

Additional command-line arguments to pass to the Mojo compiler when building this package.

```toml
[package.build.configuration.pkg]
extra-args = ["-I", "special-thing"]
```

## See Also

- [Mojo Pixi Basic](https://docs.modular.com/pixi/)
- [Modular Community Packages](https://github.com/modular/modular-community)
