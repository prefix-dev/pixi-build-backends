# Compilers in pixi-build

Some `pixi-build` backends support configurable compiler selection through the `compilers` configuration option. This feature integrates with conda-forge's compiler infrastructure to provide cross-platform, ABI-compatible builds.

!!! warning
    `pixi-build` is a preview feature, and will change until it is stabilized.
    This is why we require users to opt in to that feature by adding "pixi-build" to `workspace.preview`.

    ```toml
    [workspace]
    preview = ["pixi-build"]
    ```

## How Conda-forge Compilers Work

Understanding conda-forge's compiler system is essential for effectively using `pixi-build` compiler configuration.

### Compiler Selection and Platform Resolution

When you specify `compilers = ["c", "cxx"]` in your `pixi-build` configuration, the backend automatically selects the appropriate platform-specific compiler packages based on your target platform and build variants

Using the conda-forge infrastructure, this will result in the following packages to be selected by default.

| Compiler | Linux | macOS | Windows |
|----------|-------|--------|---------|
| `c` | `gcc_linux-64` | `clang_osx-64` | `vs2019_win-64` |
| `cxx` | `gxx_linux-64` | `clangxx_osx-64` | `vs2019_win-64` |
| `fortran` | `gfortran_linux-64` | `gfortran_osx-64` | `vs2019_win-64` |

### Build Variants and Compiler Selection

Compiler selection works through a build variant system. Build variants allow you to specify different versions or types of compilers for your builds, creating a build matrix that can target multiple compiler configurations.

#### How Variants Work

When you specify `compilers = ["c"]` in your pixi-build configuration, the system doesn't directly install a package named "c". Instead, it uses a **variant system** to determine the exact compiler package for your platform.

Here's how it works step by step:

1. **You specify a compiler**: `compilers = ["c"]`

2. **The system builds a package specification** using variants with the following pattern:
   ```
   {compiler_type}_{host_platform} {compiler_version}
   ```
   The variant names follow the pattern `{language}_compiler` and `{language}_compiler_version`:

3. **For the "c" compiler, this becomes**:
   ```
   {c_compiler}_{host_platform} {c_compiler_version}
   ```
   - `c_compiler` - determines the compiler type (constructed as "c" + "_compiler")
   - `host_platform` - your target platform 
   - `c_compiler_version` - the compiler version (constructed as "c" + "_compiler_version")

4. **The variants resolve to actual values**:
   - `c_compiler` → `gcc` (on Linux), `clang` (on macOS), `vs2019` (on Windows)
   - `host_platform` → `linux-64`, `osx-64`, `win-64`, etc.
   - `c_compiler_version` → `11.4`, `14.0`, `19.29`, etc.

5. **Final result**: A concrete package like `gcc_linux-64 11.4`

This variant system allows pixi-build to use sensible defaults while giving you precise control to override specific compilers or versions when needed.

### Overriding Compilers in Pixi Workspaces

Pixi workspaces provide powerful mechanisms to override compiler variants through build variant configuration. 
This allows users to customize compiler selection without modifying individual package recipes.

To overwrite the default C compiler you can modify your `pixi.toml` file in the workspace root:

```toml
# pixi.toml
[workspace.build-variants]
c_compiler = ["clang"]
c_compiler_version = ["11.4"]
```

To overwrite the c/cxx compiler specifically for Windows you can use the `workspace.target` section to specify platform-specific compiler variants:

```toml
# pixi.toml
[workspace.target.win.build-variants]
c_compiler = ["vs2022"]
cxx_compiler = ["vs2022"]
```

Or

```toml
[workspace.target.win.build-variants]
c_compiler = ["vs"]
cxx_compiler = ["vs"]
c_compiler_version = ["2022"]
cxx_compiler_version = ["2022"]
```

## Available Compilers

Which compilers are available depends on the channels you target but through the conda-forge infrastructure the following compilers are generally available across all platforms. 
The table below lists the core compilers, specialized compilers, and some backend language-specific compilers that can be configured in `pixi-build`.

### Core Compilers

| Compiler | Description | Platforms |
|----------|-------------|-----------|
| `c` | C compiler | Linux (gcc), macOS (clang), Windows (vs2019) |
| `cxx` | C++ compiler | Linux (gxx), macOS (clangxx), Windows (vs2019) |
| `fortran` | Fortran compiler | Linux (gfortran), macOS (gfortran), Windows (vs2019) |
| `rust` | Rust compiler | All platforms |
| `go` | Go compiler | All platforms |

### Specialized Compilers

| Compiler | Description | Platforms |
|----------|-------------|-----------|
| `cuda` | NVIDIA CUDA compiler | Linux, Windows (limited macOS) |

### Backend-Specific Compilers

| Compiler | Description | Backend | Special Behavior |
|----------|-------------|---------|------------------|
| `mojo` | Mojo compiler | pixi-build-mojo | Uses `mojo-compiler` package instead of template |

## Backend-Specific Defaults

Only certain `pixi-build` backends support the `compilers` configuration option. Each supporting backend has sensible defaults based on the typical requirements for that language ecosystem:

| Backend | Compiler Support | Default Compilers | Rationale |
|---------|------------------|-------------------|-----------|
| **[pixi-build-cmake](../backends/pixi-build-cmake.md#compilers)** | ✅ **Supported** | `["cxx"]` | Most CMake projects are C++ |
| **[pixi-build-rust](../backends/pixi-build-rust.md#compilers)** | ✅ **Supported** | `["rust"]` | Rust projects need the Rust compiler |
| **[pixi-build-python](../backends/pixi-build-python.md#compilers)** | ✅ **Supported** | `[]` | Pure Python packages typically don't need compilers |
| **[pixi-build-mojo](../backends/pixi-build-mojo.md#compilers)** | ✅ **Supported** | `["mojo"]` | Mojo projects need the Mojo compiler |
| **pixi-build-rattler-build** | ❌ **Not Supported** | N/A | Uses direct `recipe.yaml` - configure compilers directly in recipe |

!!! info "Adding Compiler Support to Other Backends"
    Backend developers can add compiler configuration support by implementing the `compilers` field in their backend configuration and integrating with the shared compiler infrastructure in `pixi-build-backend`.

## Configuration Examples

To configure compilers in your `pixi-build` project, you can use the `compilers` configuration option in your `pixi.toml` file. Below are some examples of how to set up compiler configurations for different scenarios.

!!! note "Backend Support"
Compiler configuration is only available in backends that have specifically implemented this feature. Not all backends support the `compilers` configuration option. Check your backend's documentation to see if it supports compiler configuration.

### Basic Compiler Configuration

```toml
# Use default compilers for the backend
[package.build.configuration]
# No compilers specified - uses backend defaults

# Override with specific compilers
[package.build.configuration]
compilers = ["c", "cxx", "fortran"]
```

### Platform-Specific Compiler Configuration

```toml
# Base configuration for most platforms
[package.build.configuration]
compilers = ["cxx"]

# Linux needs additional CUDA support
[package.build.configuration.targets.linux-64]
compilers = ["cxx", "cuda"]

# Windows needs additional C compiler for some dependencies
[package.build.configuration.targets.win-64]  
compilers = ["c", "cxx"]
```