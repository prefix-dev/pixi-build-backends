import subprocess
import json
import re
import sys
import tomllib
from datetime import datetime


def get_git_tags():
    try:
        result = subprocess.run(
            ["git", "tag", "--points-at", "HEAD"],
            capture_output=True,
            text=True,
            check=True,
        )
        return result.stdout.strip().splitlines()
    except subprocess.CalledProcessError:
        # if no tags are found, return an empty list
        return []


def get_git_short_hash():
    """Get the short git hash of the current HEAD"""
    result = subprocess.run(
        ["git", "rev-parse", "--short=7", "HEAD"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


def get_current_date():
    """Get current date in yyyymmdd format"""
    return datetime.now().strftime("%Y%m%d")


def extract_name_and_version_from_tag(tag):
    # Handle both Rust packages and Python package
    rust_match = re.match(r"(pixi-build-[a-zA-Z-]+)-v(\d+\.\d+\.\d+)", tag)
    python_match = re.match(r"(py-pixi-build-backend)-v(\d+\.\d+\.\d+)", tag)

    if rust_match:
        return rust_match.group(1), rust_match.group(2)
    elif python_match:
        return python_match.group(1), python_match.group(2)

    raise ValueError(
        f"Invalid Git tag format: {tag}. Expected format: pixi-build-[name]-v[version]"
    )


def generate_matrix(filter_package_name=None):
    # Run cargo metadata
    result = subprocess.run(
        ["cargo", "metadata", "--format-version=1", "--no-deps"],
        capture_output=True,
        text=True,
        check=True,
    )
    cargo_metadata = json.loads(result.stdout)
    
    # Get all packages with binary or cdylib targets
    all_packages = []
    
    if "packages" in cargo_metadata:
        for package in cargo_metadata["packages"]:
            # Include packages with binary targets (Rust binaries)
            has_binary = any(target["kind"][0] == "bin" for target in package.get("targets", []))
            
            if has_binary:
                all_packages.append({
                    "name": package["name"],
                    "version": package["version"],
                    "type": "rust"
                })
    
    # Add py-pixi-build-backend manually since it's outside the workspace
    with open("py-pixi-build-backend/Cargo.toml", "rb") as f:
        cargo_toml = tomllib.load(f)
        all_packages.append({
            "name": "py-pixi-build-backend",
            "version": cargo_toml["package"]["version"],
            "type": "python"
        })
    
    # Filter packages by name if specified
    if filter_package_name:
        available_packages = [pkg["name"] for pkg in all_packages]
        filtered_packages = [pkg for pkg in all_packages if pkg["name"] == filter_package_name]
        if not filtered_packages:
            raise ValueError(f"Package '{filter_package_name}' not found. Available packages: {', '.join(available_packages)}")
        all_packages = filtered_packages
        print(f"Filtering to package: {filter_package_name}", file=sys.stderr)

    # this is to overcome the issue of matrix generation from github actions side
    # https://github.com/orgs/community/discussions/67591
    targets = [
        {"target": "linux-64", "os": "ubuntu-latest"},
        {"target": "linux-aarch64", "os": "ubuntu-latest"},
        {"target": "linux-ppc64le", "os": "ubuntu-latest"},
        {"target": "win-64", "os": "windows-latest"},
        {"target": "osx-64", "os": "macos-14"},
        {"target": "osx-arm64", "os": "macos-14"},
    ]

    git_tags = get_git_tags()
    is_untagged_build = len(git_tags) == 0

    # Extract bin names, versions, and generate env and recipe names
    matrix = []

    if not all_packages:
        raise ValueError("No packages found")

    if is_untagged_build:
        # Untagged build - include all packages with auto-versioning
        date_suffix = get_current_date()
        git_hash = get_git_short_hash()
        
        print(f"Building all packages for untagged build with date suffix: {date_suffix}, git hash: {git_hash}", file=sys.stderr)
        
        package_names = []
        for package in all_packages:
            package_names.append(package["name"])
            # Create auto-version: original_version.ddmmyyyy.git_hash
            auto_version = f"{package['version']}.{date_suffix}.{git_hash}"
            
            # Generate environment variable name
            if package["type"] == "python":
                env_name = "PY_PIXI_BUILD_BACKEND_VERSION"
            else:
                env_name = f"{package['name'].replace('-', '_').upper()}_VERSION"
            
            for target in targets:
                matrix.append(
                    {
                        "bin": package["name"],
                        "target": target["target"],
                        "version": auto_version,
                        "env_name": env_name,
                        "os": target["os"],
                    }
                )

        if not package_names:
            raise RuntimeError("No packages found for untagged build")
        
        print(f"Found {len(package_names)} packages: {', '.join(package_names)}", file=sys.stderr)
    else:
        # Tag-based build - only include tagged packages
        # verify that the tags match the package versions
        tagged_packages = {tag: False for tag in git_tags}

        for package in all_packages:
            package_tagged = False
            for git_tag in git_tags:
                # verify that the git tag matches the package version
                tag_name, tag_version = extract_name_and_version_from_tag(git_tag)
                if package["name"] != tag_name:
                    continue  # Skip packages that do not match the tag

                if package["version"] != tag_version:
                    raise ValueError(
                        f"Version mismatch: Git tag version {tag_version} does not match package version {package['version']} for {package['name']}"
                    )

                tagged_packages[git_tag] = package
                package_tagged = True

            # verify that tags exist for this HEAD
            # and that the package has been tagged
            if tagged_packages and not package_tagged:
                continue

            # Generate environment variable name
            if package["type"] == "python":
                env_name = "PY_PIXI_BUILD_BACKEND_VERSION"
            else:
                env_name = f"{package['name'].replace('-', '_').upper()}_VERSION"

            for target in targets:
                matrix.append(
                    {
                        "bin": package["name"],
                        "target": target["target"],
                        "version": package["version"],
                        "env_name": env_name,
                        "os": target["os"],
                    }
                )

    # Only validate tags for tag-based builds
    if not is_untagged_build and git_tags:
        for git_tag, has_a_package in tagged_packages.items():
            if not has_a_package:
                raise ValueError(
                    f"Git tag {git_tag} does not match any package in Cargo.toml"
                )

    if not matrix:
        if is_untagged_build:
            raise RuntimeError("No packages found to build for untagged build")
        else:
            raise RuntimeError("No tagged packages found to build")
    
    matrix_json = json.dumps(matrix)
    
    # Debug output to stderr so it doesn't interfere with matrix JSON
    print(f"Generated matrix with {len(matrix)} entries", file=sys.stderr)
    
    print(matrix_json)


if __name__ == "__main__":
    import argparse
    
    parser = argparse.ArgumentParser(description="Generate build matrix for packages")
    parser.add_argument("--package", help="Filter to specific package name")
    args = parser.parse_args()
    
    generate_matrix(args.package)
