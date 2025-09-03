---
title: Home
template: home.html
---

![pixi logo](assets/banner.svg)

# Pixi Build Backends

**Backend Implementations for Building Pixi Projects from Source**

Pixi Build Backends is a collection of specialized build backend implementations designed to facilitate building [Pixi](https://pixi.sh) packages directly from their source code.
These backends enable seamless integration with the Pixi ecosystem, supporting multiple programming languages and build systems while maintaining the conda ecosystem's cross-platform compatibility.

## 🚀 What Are Build Backends?

Build backends are executables that follow a specific protocol to decouple the building of conda packages from Pixi itself. This architecture allows for:

- **Language-specific optimization**: Each backend is tailored for specific programming languages and build tools
- **Modular design**: Backends can be developed, updated, and distributed independently
- **Extensibility**: New backends can be added without modifying Pixi core
- **Standardization**: All backends follow the same protocol and manifest specifications

## 📦 Available Backends

The repository currently provides the following specialized build backends:

| Backend   | Use Case |
|---------|----------|
| [**`pixi-build-cmake`**](./backends/pixi-build-cmake.md) |  Projects using CMake |
| [**`pixi-build-python`**](./backends/pixi-build-python.md) | Building Python packages |
| [**`pixi-build-rattler-build`**](./backends/pixi-build-rattler-build.md) | Direct `recipe.yaml` builds with full control |
| [**`pixi-build-ros`**](./backends/pixi-build-ros.md) | ROS (Robot Operating System) packages |
| [**`pixi-build-rust`**](./backends/pixi-build-rust.md) |  Cargo-based Rust applications and libraries |
| [**`pixi-build-mojo`**](./backends/pixi-build-mojo.md) |  Mojo applications and packages |

All backends are available through the [prefix.dev/conda-forge](https://prefix.dev/channels/conda-forge) conda channel and work across multiple platforms (Linux, macOS, Windows).
For the latest backend versions, you can extend the channel list with the [prefix.dev/pixi-build-backends](https://prefix.dev/channels/pixi-build-backends) conda channel, here we push the latest versions of the backends.

## 🛠️ Getting Started

Check out our [tutorial series](https://pixi.sh/latest/build/getting_started/) to learn how to use `pixi build` in practice.

## 📚 Key Concepts

-  [Compilers](./key_concepts/compilers.md)

    Learn how pixi-build integrates with conda-forge's compiler infrastructure to provide cross-platform, ABI-compatible builds. Covers compiler configuration, platform-specific behavior, and available compiler options for supported backends.

## 🔗 Useful Links

- [GitHub](https://github.com/prefix-dev/pixi): Pixi source code, feel free to leave a star!
- [Discord](https://discord.gg/kKV8ZxyzY4): Join our community and ask questions.
- [Prefix.dev](https://prefix.dev/): The company behind Pixi, building the future of package management.
- [conda-forge](https://conda-forge.org/): Community-driven collection of recipes for the conda package manager.
- [Rattler](https://github.com/conda/rattler): Everything conda but built in Rust. Backend of Pixi.
- [rattler-build](https://rattler.build): A blazing fast build system for conda packages.
