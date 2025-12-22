# Pixi Build Testsuite

This repo contains the testsuite that is used by both [Pixi] CI and [pixi-build-backends] CI to verify that they work properly in combination.


## Local development

The tests can use the pre-built binaries produced by the Pixi and pixi-build-backends CI workflows. Download the latest artifacts for your platform:

```shell
pixi run download-artifacts
```

The binaries are stored in `artifacts/`, alongside a `download-metadata.json` file that records which branch or PR each artifact originated from. When running locally the script will reuse the active `gh auth` session; if `gh` is unavailable, set `GITHUB_TOKEN` or pass `--token`. Use `--repo pixi` or `--repo pixi-build-backends` to fetch artifacts for a single project.

With the artifacts in place you can run the fast subset of the tests (or any other Pixi task):

```shell
pixi run test
```

### Using local builds instead of artifacts

If you prefer to use local checkouts, create a `.env` file with the paths to your repositories:

```shell
PIXI_REPO="/path/to/pixi-repository"
PIXI_BIN_DIR="${PIXI_REPO}/target/pixi/release"

BUILD_BACKENDS_REPO="/path/to/pixi-build-backends-repository"
```

Then build the binaries with:

```shell
pixi run build-repos
```

## Testing PR combinations

To test a combination of PRs from this testsuite with PRs from [Pixi] or [pixi-build-backends]:

1. Create a `.env.ci` or modify your local `.env` file with PR numbers:
   ```shell
   # Test with specific PR from pixi repository
   PIXI_PR_NUMBER="123"

   # Test with specific PR from pixi-build-backends repository
   BUILD_BACKENDS_PR_NUMBER="456"
   ```
2. `pixi run download-artifacts` (locally or in CI) will download artifacts from these PRs instead of main
3. **Important**: Remove `.env.ci` before merging to main (CI will prevent merge if present)

This allows you to test how your testsuite changes work with specific PRs from the other repositories.

[Pixi]: https://github.com/prefix-dev/pixi
[pixi-build-backends]: https://github.com/prefix-dev/pixi-build-backends
