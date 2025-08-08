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

The generated packages can be installed into local envs for development, or packaged for distribution.

### Auto-derive of pkg and bin

The Mojo backend includes auto-discovery of your project structure and will derive the following:

- **Binaries**: Automatically searches for `main.mojo` or `main.🔥` in:
  - `<project_root>/main.mojo`
- **Packages**: Automatically searches for directories with `__init__.mojo` or `__init__.🔥` in:
  - `<project_root>/<project_name>/`
  - `<project_root>/src/`

This means in most cases, you don't need to explicitly configure the `bins` or `pkg` fields.

**Caveats**:
- If both a `bin` and a `pkg` are auto-derived, only the `bin` will be created, you must manually specify the pkg.
- If the user specifies a `pkg` a `bin` will not be auto-derived.
- If the user specifies a `bin` a `pkg` will not be auto-derived.


## Basic Usage

To use the Mojo backend in your `pixi.toml`, add it to your package's build configuration. The backend will automatically discover your project structure:


```txt
# Example project layout for combined binary/library.
.
├── greetings
│   ├── __init__.mojo
│   └── lib.mojo
├── main.mojo
├── pixi.lock
├── pixi.toml
└── README.md
```

With the project structure above, pixi-build-mojo will automatically discover:
- The binary from `main.mojo`
- The package from `greetings/__init__.mojo`

Here's a minimal configuration that leverages auto-derive:

```toml
[workspace]
authors = ["J. Doe <jdoe@mail.com>"]
platforms = ["linux-64"]
preview = ["pixi-build"]
channels = [
    "https://prefix.dev/conda-forge",
    "https://conda.modular.com/max-nightly",
    "https://prefix.dev/modular-community"
]

[package]
name = "greetings"
version = "0.1.0"

[package.build]
backend = { name = "pixi-build-mojo", version = "0.1.*" }

[tasks]

[package.host-dependencies]
mojo-compiler = "=25.5.0"

[package.build-dependencies]
mojo-compiler = "=25.5.0"
small_time = ">=25.4.1,<26"
extramojo = ">=0.16.0,<0.17"

[package.run-dependencies]
mojo-compiler = "=25.5.0"

[dependencies]
# For running `mojo test` while developing add all dependencies under
# `[package.build-dependencies]` here as well.
greetings = { path = "." }
```

### Project Structure Examples

The auto-derive feature supports various common project layouts:

#### Binary-only project
```txt
.
├── main.mojo           # Auto-derive as binary
├── pixi.toml
└── README.md
```

#### Package-only project
```txt
.
├── mypackage/          # Auto-derive if matches project name
│   ├── __init__.mojo
│   └── utils.mojo
├── pixi.toml
└── README.md
```

#### Source directory layout
```txt
.
├── src/
│   ├── __init__.mojo   # Auto-derive as package
│   └── lib.mojo
├── pixi.toml
└── README.md
```

#### Combined project (shown earlier)
```txt
.
├── greetings/
│   ├── __init__.mojo   # NOT auto-derived as package
│   └── lib.mojo
├── main.mojo           # Auto-derived as binary
├── pixi.toml
└── README.md
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

Additional globs to pass to pixi to discover if the package should be rebuilt.

```toml
[package.build.configuration]
extra-input-globs = ["**/*.c", "assets/**/*", "*.md"]
```

### `bins`

- **Type**: `Array<BinConfig>`
- **Default**: Auto-derived if not specified

List of binary configurations to build. The created binary will be placed in the `$PREFIX/bin` dir and will be in the path after running `pixi install` assuming the package is listed as a dependency as in the example above. `pixi build` will create a conda package that includes the binary.

**Auto-derive behavior:**
- If `bins` is not specified, pixi-build-mojo will search for a `main.mojo` or `main.🔥` file in the project root
- If found, it creates a binary with the name set to the project name
- If a pkg has been manually configured, a bin will not be auto-derived and must be manually configured.

#### `bins[].name`

- **Type**: `String`
- **Default**: Project name (with dashes converted to underscores) for the first binary

The name of the binary executable to create. If not specified:
- For the first binary in the list, defaults to the project name
- For additional binaries, this field is required

```toml
[[package.build.configuration.bins]]
# name = "greet"  # Optional for first binary, defaults to project name
```

#### `bins[].path`

- **Type**: `String` (path)
- **Default**: Auto-derived for the first binary

The path to the Mojo file that contains a `main` function. If not specified:
- For the first binary, searches for `main.mojo` or `main.🔥` in the project root
- For additional binaries, this field is required

```toml
[[package.build.configuration.bins]]
# path = "./main.mojo"  # Optional if main.mojo exists in project root
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
- **Default**: Auto-derived if not specified

Package configuration for creating Mojo package. The created Mojo package will be placed in the `$PREFIX/lib/mojo` dir, which will make it discoverable to anything that depends on the package.

**Auto-derive behavior:**
- If `pkg` is not specified, pixi-build-mojo will search for a directory containing `__init__.mojo` or `__init__.🔥` in the following order:
  1. `<project_root>/<project_name>/`
  2. `<project_root>/src/`
- If found, it creates a package with the name set to the project name
- If no valid package directory is found, no package is built
- If a binary is manually configured, a pkg will not be auto-derived and must be manually specified.
- If a binary is also auto-derive, a pkg will not be generated and must be manually specified

#### `pkg.name`

- **Type**: `String`
- **Default**: Project name (with dashes converted to underscores)

The name to give the Mojo package. The `.mojopkg` suffix will be added automatically. If not specified, defaults to the project name.

```toml
[package.build.configuration.pkg]
name = "greetings"
```

#### `pkg.path`

- **Type**: `String` (path)
- **Default**: Auto-derive

The path to the directory that constitutes the package. If not specified, searches for a directory with `__init__.mojo` or `__init__.🔥` as described above.

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
