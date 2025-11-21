from pathlib import Path

import pytest

from .common import copytree_with_local_backend, get_manifest, repo_root, verify_cli_command


@pytest.mark.slow
@pytest.mark.parametrize(
    "pixi_project",
    [
        pytest.param(example_path, id=example_path.name)
        for example_path in repo_root().joinpath("examples").iterdir()
        if example_path.is_dir()
    ],
)
def test_pixi_install_examples(pixi_project: Path, pixi: Path, tmp_pixi_workspace: Path) -> None:
    """
    Test that pixi install succeeds for all example projects in the examples directory.

    This test iterates through all folders in the examples directory and verifies
    that `pixi install` completes successfully for each project.
    """
    # Copy to workspace
    copytree_with_local_backend(pixi_project, tmp_pixi_workspace, dirs_exist_ok=True)

    # Get manifest
    manifest = get_manifest(tmp_pixi_workspace)

    # Install the environment
    verify_cli_command([pixi, "install", "-v", "--locked", "--manifest-path", manifest])
