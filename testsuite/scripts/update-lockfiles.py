#!/usr/bin/env python3
"""
Script to update pixi.lock files in test directories.

This script:
1. Recursively searches for pixi.lock files in tests/data/pixi_build (or specified directory)
2. Runs `pixi lock` in each directory containing a pixi.lock file
3. Stops execution on the first error encountered
4. Accepts optional command-line argument to specify a specific subdirectory to process
"""

import argparse
import os
import platform
import subprocess
import sys
from pathlib import Path

from dotenv import load_dotenv


class PixiLockError(Exception):
    """Raised when pixi lock fails."""

    pass


def exec_extension(exe_name: str) -> str:
    if platform.system() == "Windows":
        return exe_name + ".exe"
    else:
        return exe_name


def pixi() -> Path:
    """
    Returns the path to the Pixi executable.

    Uses the PIXI_BIN_DIR environment variable to locate the Pixi directory.
    Falls back to binaries downloaded into the artifacts directory.
    """
    pixi_bin_dir = os.getenv("PIXI_BIN_DIR")

    if pixi_bin_dir:
        pixi_bin_path = Path(pixi_bin_dir)
    else:
        project_root = Path(__file__).parent.parent
        candidates = [
            project_root / "artifacts",
            project_root / "artifacts" / "pixi",
        ]
        pixi_bin_path = None
        for candidate in candidates:
            executable_candidate = candidate / exec_extension("pixi")
            if candidate.is_dir() and executable_candidate.is_file():
                pixi_bin_path = candidate
                os.environ["PIXI_BIN_DIR"] = str(candidate)
                pixi_bin_dir = os.environ["PIXI_BIN_DIR"]
                break

    if pixi_bin_dir is None and pixi_bin_path is None:
        raise ValueError(
            "Could not determine Pixi binary location. Set PIXI_BIN_DIR or run "
            "'pixi run download-artifacts --repo pixi'."
        )

    if pixi_bin_path is None or not pixi_bin_path.is_dir():
        raise ValueError(
            f"PIXI_BIN_DIR points to '{pixi_bin_dir}' which is not a valid directory. "
            "Please set it to a directory that exists and contains the Pixi executable."
        )

    pixi_executable = pixi_bin_path / exec_extension("pixi")

    if not pixi_executable.is_file():
        raise FileNotFoundError(
            f"Pixi executable not found at '{pixi_executable}'. Set PIXI_BIN_DIR or run "
            "'pixi run download-artifacts --repo pixi'."
        )

    return pixi_executable


def run_command(
    cmd: list[str], cwd: Path | None = None, capture_output: bool = True
) -> tuple[int, str, str]:
    """Run a command and return exit code, stdout, and stderr."""
    result = subprocess.run(cmd, cwd=cwd, capture_output=capture_output, text=True)
    return result.returncode, result.stdout, result.stderr


def pixi_lock(directory: Path) -> None:
    """Run pixi lock in the specified directory."""
    print(f"🔄 Running pixi lock in {directory}")
    returncode, stdout, stderr = run_command([str(pixi()), "lock"], cwd=directory)

    if returncode == 0:
        print(f"✅ Successfully updated lockfile in {directory}")
        if stdout.strip():
            print(f"   {stdout.strip()}")
    else:
        error_msg = f"Failed to run pixi lock in {directory}"
        if stderr:
            error_msg += f": {stderr}"
        if stdout:
            error_msg += f" (Output: {stdout})"
        raise PixiLockError(error_msg)


def find_and_process_lockfiles(base_path: Path) -> None:
    """Recursively find directories with pixi.lock files and run pixi lock."""
    if not base_path.exists():
        print(f"❌ Directory {base_path} does not exist")
        sys.exit(1)

    if not base_path.is_dir():
        print(f"❌ Path {base_path} is not a directory")
        sys.exit(1)

    print(f"🔍 Searching for pixi.lock files in {base_path}")

    # Find all pixi.lock files recursively
    lockfiles = list(base_path.rglob("pixi.lock"))

    if not lockfiles:
        print(f"⚠️  No pixi.lock files found in {base_path}")
        return

    print(f"📋 Found {len(lockfiles)} pixi.lock file(s)")

    # Process each directory containing a pixi.lock file
    for lockfile in lockfiles:
        directory = lockfile.parent
        print(f"\n{'=' * 60}")
        print(f"Processing: {directory}")
        print(f"{'=' * 60}")

        try:
            pixi_lock(directory)
        except PixiLockError as e:
            print(f"❌ {e}")
            sys.exit(1)


def main() -> None:
    """Main function to process lockfiles."""

    # Load environment variables from .env file
    env_file = Path(__file__).parent.parent / ".env"
    if env_file.exists():
        load_dotenv(env_file, override=True)
        print(f"✅ Loaded environment variables from {env_file}")
    else:
        print(f"⚠️  No .env file found at {env_file}")

    parser = argparse.ArgumentParser(
        description="Update pixi.lock files by running pixi lock",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s                    # Process all directories in tests/data/pixi_build
  %(prog)s rich_example       # Process only the rich_example subdirectory
  %(prog)s maturin            # Process only the maturin subdirectory
        """,
    )
    parser.add_argument(
        "folder",
        nargs="?",
        help="Specific folder to process (relative to tests/data/pixi_build). If not specified, processes all directories.",
    )

    args = parser.parse_args()

    # Determine base path
    script_dir = Path(__file__).parent
    base_data_dir = script_dir.parent / "tests" / "data" / "pixi_build"

    if args.folder:
        target_path = base_data_dir / args.folder
        print(f"🎯 Target directory: {target_path}")
    else:
        target_path = base_data_dir
        print(f"🎯 Target directory: {target_path} (all subdirectories)")

    try:
        find_and_process_lockfiles(target_path)
        print(f"\n{'=' * 60}")
        print("🎉 All lockfiles processed successfully!")
    except KeyboardInterrupt:
        print("\n❌ Operation cancelled by user")
        sys.exit(1)


if __name__ == "__main__":
    main()
