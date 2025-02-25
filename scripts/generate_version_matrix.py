import subprocess
import json
import re

def get_git_tag():
    try:
        result = subprocess.run(["git", "describe", "--tags", "--exact-match"], capture_output=True, text=True, check=True)
        return result.stdout.strip()
    except subprocess.CalledProcessError:
        return None

def extract_name_and_version_from_tag(tag):
    match = re.match(r"(pixi-build-[a-zA-Z-]+)-v(\d+\.\d+\.\d+)", tag)
    if match:
        return match.group(1), match.group(2)
    return None, None

def verify_name_and_version(tag, cargo_name, cargo_version):
    tag_name, tag_version = extract_name_and_version_from_tag(tag)
    if not tag_name or not tag_version:
        raise ValueError(f"Invalid Git tag format: {tag}. Expected format: pixi-build-[name]-v[version]")

    if cargo_name == tag_name:
        if cargo_version != tag_version:
            raise ValueError(f"Version mismatch: Git tag version {tag_version} does not match Cargo version {cargo_version} for {cargo_name}")

def generate_matrix():
    # Run cargo metadata
    result = subprocess.run(
        ["cargo", "metadata", "--format-version=1", "--no-deps"],
        capture_output=True,
        text=True,
        check=True,
    )

    metadata = json.loads(result.stdout)
    # this is to overcome the issue of matrix generation from github actions side
    # https://github.com/orgs/community/discussions/67591
    targets = [
        {"target": "linux-64", "os": "ubuntu-20.04"},
        {"target": "linux-aarch64", "os": "ubuntu-latest"},
        {"target": "linux-ppc64le", "os": "ubuntu-latest"},
        {"target": "win-64", "os": "windows-latest"},
        {"target": "osx-64", "os": "macos-13"},
        {"target": "osx-arm64", "os": "macos-14"}
    ]

    # Extract bin names, versions, and generate env and recipe names
    matrix = []
    for package in metadata.get("packages", []):
        if any(target["kind"][0] == "bin" for target in package.get("targets", [])):
            if git_tag:
                tag_name, tag_version = extract_name_and_version_from_tag(git_tag)
                if package["name"] != tag_name or package["version"] != tag_version:
                    continue  # Skip packages that do not match the tag

            for target in targets:
                matrix.append({
                    "bin": package["name"],
                    "version": package["version"],
                    "env_name": re.sub("-", "_", package["name"]).upper() + "_VERSION",
                    "recipe_name": re.sub("-", "_", package["name"]),
                    "target": target["target"],
                    "os": target["os"]
                })

    matrix_json = json.dumps(matrix)

    git_tag = get_git_tag()
    if git_tag:
        # Verify name and version consistency
        for entry in matrix:
            verify_name_and_version(git_tag, entry["bin"], entry["version"])


    print(matrix_json)

if __name__ == "__main__":
    generate_matrix()
