# Release Notes

## Overview
`rattler-build.yml` workflow automates the process of building and publishing pixi build backends as conda packages.
The workflow is triggered by:

- A `push` event with tags matching:
  - `pixi-build-cmake-vX.Y.Z`
  - `pixi-build-python-vX.Y.Z`
  - `pixi-build-rattler-build-vX.Y.Z`
- A `pull_request` event


## Usage Instructions

### Triggering a Release
1. Create a new tag following the pattern `pixi-build-<backend>-vX.Y.Z` (e.g., `pixi-build-cmake-v1.2.3`)
2. Push the tag to the repository:
   ```sh
   git tag pixi-build-cmake-v1.2.3
   git push origin pixi-build-cmake-v1.2.3
   ```
3. The workflow will automatically build and upload the package.

### Adding a new backend
When adding a new backend, you will need to add a new backend tag to the `rattler-build.yml` workflow.
