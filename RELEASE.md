# Release Notes

## Overview
`rattler-build.yml` workflow automates the process of building and publishing pixi build backends as conda packages.
The workflow is triggered by:

- A `push` event with tags matching:
  - `pixi-build-cmake-vX.Y.Z`
  - `pixi-build-python-vX.Y.Z`
  - `pixi-build-rattler-build-vX.Y.Z`
- A `pull_request` event


Note: Actual releases are only triggered by git tags matching the patterns above.
Pull requests will build the packages but not publish them


## Usage Instructions

### Triggering a Release
- Bump the version in the `Cargo.toml` for the backend you want to release.
- Open a pull request
- After the pull request is merged, create a new tag following the pattern `pixi-build-<backend>-vX.Y.Z` (e.g., `pixi-build-cmake-v1.2.3`)
- Push the tag to the repository:
   ```sh
   git tag pixi-build-cmake-v1.2.3
   git push origin pixi-build-cmake-v1.2.3
   ```
- The workflow will automatically build and upload the package.

### Adding a new backend
When adding a new backend, you will need to add a new backend tag to the `rattler-build.yml` workflow.
