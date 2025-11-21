import os
import platform
import re
import shutil
import subprocess
from contextlib import contextmanager
from dataclasses import dataclass
from enum import IntEnum
from pathlib import Path
from typing import Any, Generator

import tomli_w
import tomllib
import yaml
from rattler import Platform

PIXI_VERSION = "0.47.0"


ALL_PLATFORMS = '["linux-64", "osx-64", "osx-arm64", "win-64", "linux-ppc64le", "linux-aarch64"]'

CURRENT_PLATFORM = str(Platform.current())

EMPTY_BOILERPLATE_PROJECT = f"""
[workspace]
name = "test"
channels = []
platforms = ["{CURRENT_PLATFORM}"]
"""


def get_local_backend_channel() -> str:
    env_repo = os.environ.get("BUILD_BACKENDS_REPO")
    if env_repo:
        repo_path = Path(env_repo).expanduser().joinpath("artifacts-channel")
        if repo_path.is_dir() and any(repo_path.rglob("repodata.json")):
            return repo_path.as_uri()

    channel_dir = repo_root().joinpath("artifacts", "pixi-build-backends")
    if channel_dir.is_dir() and any(channel_dir.rglob("repodata.json")):
        return channel_dir.as_uri()

    raise Exception("No BUILD_BACKENDS_REPO defined, can't find artifacts-channel dir")


def copy_manifest(
    src: os.PathLike[str],
    dst: os.PathLike[str],
) -> Path:
    """
    Copy file with special handling for pixi manifest.

    It will override backends channel with local backends channel.
    """
    copied_path = Path(shutil.copy(src, dst))
    local_uri = get_local_backend_channel()

    if copied_path.suffix != ".toml":
        return copied_path

    content = copied_path.read_text(encoding="utf-8")

    changed = False
    if copied_path.name == "pixi.toml":
        data = tomllib.loads(content)

        package = data.get("package")
        if isinstance(package, dict):
            build = package.get("build")
            if isinstance(build, dict):
                backend = build.get("backend")
                if isinstance(backend, dict):
                    channels = backend.get("channels")
                    new_channels: list[str] = []
                    if not channels:
                        new_channels = [local_uri, "https://prefix.dev/conda-forge"]
                    else:
                        for channel in channels:
                            if "pixi-build-backends" in channel:
                                new_channels.append(local_uri)
                            else:
                                new_channels.append(channel)
                        # Handle case where channels is not defined
                        if local_uri not in new_channels:
                            new_channels.append(local_uri)

                    backend["channels"] = new_channels
                    changed = new_channels != channels
        if changed:
            content = tomli_w.dumps(data)

    if changed:
        copied_path.write_text(content, encoding="utf-8")

    return copied_path


def copytree_with_local_backend(
    src: os.PathLike[str],
    dst: os.PathLike[str],
    **kwargs: Any,
) -> Path:
    kwargs.setdefault("copy_function", copy_manifest)

    # Copy tree while ignoring .pixi directories
    return Path(
        shutil.copytree(src, dst, ignore=shutil.ignore_patterns(".pixi", "*.conda"), **kwargs)
    )


@dataclass
class Workspace:
    recipe: dict[str, Any]
    workspace_manifest: dict[str, Any]
    workspace_dir: Path
    package_manifest: dict[str, Any]
    package_dir: Path
    recipe_path: Path
    debug_dir: Path

    def write_files(self) -> None:
        self.recipe_path.write_text(yaml.dump(self.recipe))
        workspace_manifest_path = self.workspace_dir.joinpath("pixi.toml")
        workspace_manifest_path.write_text(tomli_w.dumps(self.workspace_manifest))
        package_manifest_path = self.package_dir.joinpath("pixi.toml")
        package_manifest_path.write_text(tomli_w.dumps(self.package_manifest))


class ExitCode(IntEnum):
    SUCCESS = 0
    FAILURE = 1
    INCORRECT_USAGE = 2
    COMMAND_NOT_FOUND = 127


class Output:
    command: list[Path | str]
    stdout: str
    stderr: str
    returncode: int

    def __init__(self, command: list[Path | str], stdout: str, stderr: str, returncode: int):
        self.command = command
        self.stdout = stdout
        self.stderr = stderr
        self.returncode = returncode

    def __str__(self) -> str:
        return f"command: {self.command}"


def verify_cli_command(
    command: list[Path | str],
    expected_exit_code: ExitCode = ExitCode.SUCCESS,
    stdout_contains: str | list[str] | None = None,
    stdout_excludes: str | list[str] | None = None,
    stderr_contains: str | list[str] | None = None,
    stderr_excludes: str | list[str] | None = None,
    env: dict[str, str] | None = None,
    cwd: str | Path | None = None,
    reset_env: bool = False,
    strip_ansi: bool = False,
) -> Output:
    base_env = {} if reset_env else dict(os.environ)
    complete_env = base_env if env is None else base_env | env
    # Set `PIXI_NO_WRAP` to avoid to have miette wrapping lines
    complete_env |= {"PIXI_NO_WRAP": "1"}

    process = subprocess.run(
        command,
        capture_output=True,
        env=complete_env,
        cwd=cwd,
    )
    # Decode stdout and stderr explicitly using UTF-8
    stdout = process.stdout.decode("utf-8", errors="replace")
    stderr = process.stderr.decode("utf-8", errors="replace")

    if strip_ansi:
        # sanitise coloured output to match plain strings
        ansi_escape = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")
        stdout = ansi_escape.sub("", stdout)
        stderr = ansi_escape.sub("", stderr)

    returncode = process.returncode
    output = Output(command, stdout, stderr, returncode)
    print(f"command: {command}, stdout: {stdout}, stderr: {stderr}, code: {returncode}")
    assert returncode == expected_exit_code, (
        f"Return code was {returncode}, expected {expected_exit_code}, stderr: {stderr}"
    )

    if stdout_contains:
        if isinstance(stdout_contains, str):
            stdout_contains = [stdout_contains]
        for substring in stdout_contains:
            assert substring in stdout, f"'{substring}'\n not found in stdout:\n {stdout}"

    if stdout_excludes:
        if isinstance(stdout_excludes, str):
            stdout_excludes = [stdout_excludes]
        for substring in stdout_excludes:
            assert substring not in stdout, (
                f"'{substring}'\n unexpectedly found in stdout:\n {stdout}"
            )

    if stderr_contains:
        if isinstance(stderr_contains, str):
            stderr_contains = [stderr_contains]
        for substring in stderr_contains:
            assert substring in stderr, f"'{substring}'\n not found in stderr:\n {stderr}"

    if stderr_excludes:
        if isinstance(stderr_excludes, str):
            stderr_excludes = [stderr_excludes]
        for substring in stderr_excludes:
            assert substring not in stderr, (
                f"'{substring}'\n unexpectedly found in stderr:\n {stderr}"
            )

    return output


def bat_extension(exe_name: str) -> str:
    if platform.system() == "Windows":
        return exe_name + ".bat"
    else:
        return exe_name


def exec_extension(exe_name: str) -> str:
    if platform.system() == "Windows":
        return exe_name + ".exe"
    else:
        return exe_name


def is_binary(path: Path) -> bool:
    textchars = bytearray({7, 8, 9, 10, 12, 13, 27} | set(range(0x20, 0x100)) - {0x7F})
    with open(path, "rb") as f:
        return bool(f.read(2048).translate(None, bytes(textchars)))


def pixi_dir(project_root: Path) -> Path:
    return project_root.joinpath(".pixi")


def default_env_path(project_root: Path) -> Path:
    return pixi_dir(project_root).joinpath("envs", "default")


def repo_root() -> Path:
    return Path(__file__).parents[2]


def current_platform() -> str:
    return str(Platform.current())


def get_manifest(directory: Path) -> Path:
    pixi_toml = directory / "pixi.toml"
    pyproject_toml = directory / "pyproject.toml"

    if pixi_toml.exists():
        return pixi_toml
    elif pyproject_toml.exists():
        return pyproject_toml
    else:
        raise ValueError("Neither pixi.toml nor pyproject.toml found")


@contextmanager
def cwd(path: str | Path) -> Generator[None, None, None]:
    oldpwd = os.getcwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(oldpwd)


def git_test_repo(source_dir: Path, repo_name: str, target_dir: Path) -> str:
    """Create a git repository from the source directory in a target directory."""
    repo_path: Path = target_dir / repo_name

    # Copy source directory to temp
    copytree_with_local_backend(source_dir, repo_path, copy_function=copy_manifest)

    # Initialize git repository in the copied source
    subprocess.run(
        ["git", "init"],
        cwd=repo_path,
        check=True,
        capture_output=True,
    )

    # Add all files and commit
    subprocess.run(
        ["git", "add", "."],
        cwd=repo_path,
        check=True,
        capture_output=True,
    )
    subprocess.run(
        ["git", "config", "user.email", "bot@prefix.dev"],
        cwd=repo_path,
        check=True,
        capture_output=True,
    )
    subprocess.run(
        ["git", "config", "user.name", "Bot"],
        cwd=repo_path,
        check=True,
        capture_output=True,
    )
    subprocess.run(
        ["git", "commit", "--message", "Initial commit"],
        cwd=repo_path,
        check=True,
        capture_output=True,
    )

    return f"file://{repo_path}"
