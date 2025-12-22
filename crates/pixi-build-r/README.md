# pixi-build-r

R language build backend for Pixi.

## Features

- Automatic metadata extraction from DESCRIPTION files
- Native code detection and compiler auto-configuration
- Standard R package directory structure support
- Cross-platform build support (Unix/Windows)

## Configuration

### Basic Configuration

```toml
[build-system]
build-backend = "pixi-build-r"
```

### Advanced Configuration

```toml
[tool.pixi-build-r]
# Extra arguments for R CMD INSTALL
extra-args = ["--no-multiarch", "--no-test-load"]

# Explicit compiler specification (overrides auto-detection)
compilers = ["c", "cxx", "fortran"]

# Custom conda channels
channels = ["conda-forge", "r"]

# Extra input globs for cache invalidation
extra-input-globs = ["inst/**/*"]

# Environment variables
[tool.pixi-build-r.env]
R_LIBS_USER = "$PREFIX/lib/R/library"
```

## Compiler Auto-Detection

The backend automatically detects native code by:
1. Checking for `src/` directory existence
2. Checking for `LinkingTo` field in DESCRIPTION

If native code is detected, defaults to: `["c", "cxx", "fortran"]`

## R Package Structure

Expected directory structure:
```
package/
├── DESCRIPTION     # Required: package metadata
├── NAMESPACE       # Required: exported functions
├── R/              # Required: R source code
├── src/            # Optional: native code (triggers compiler detection)
├── man/            # Optional: documentation
├── tests/          # Optional: tests
└── data/           # Optional: data files
```

## System Dependencies

Some R packages require system libraries (e.g., `libcurl`, `libxml2`, `openssl`). These are typically listed in the `SystemRequirements` field of the DESCRIPTION file, but the format is not standardized and varies widely between packages.

To add system dependencies, use the `[package.host-dependencies]` table in your `pixi.toml`:

```toml
[package]
name = "r-curl"
version = "1.0.0"

[package.host-dependencies]
curl = "*"
```

## Installation Path

Packages are installed to:
- Unix: `$PREFIX/lib/R/library/`
- Windows: `%LIBRARY_PREFIX%\R\library\`

## Usage

1. Create a standard R package with a DESCRIPTION file
2. Add `pixi.toml` with the build backend configuration
3. Build your package:
   ```bash
   pixi build
   ```

The backend will:
- Parse metadata from DESCRIPTION
- Auto-detect compilers (if `src/` directory or `LinkingTo` present)
- Add r-base to host and run dependencies
- Generate platform-specific build scripts
- Execute `R CMD INSTALL` to build the package
