import subprocess
import json
import re

def get_git_tag():
    try:
        result = subprocess.run(["git", "describe", "--tags", "--exact-match"], capture_output=True, text=True, check=True)
        return result.stdout.strip()
    except subprocess.CalledProcessError:
        # if there are not tags in the git history, return None
        return None

def extract_name_and_version_from_tag(tag):
    match = re.match(r"(pixi-build-[a-zA-Z-]+)-v(\d+\.\d+\.\d+)", tag)
    if match:
        return match.group(1), match.group(2)
    raise ValueError(f"Invalid Git tag format: {tag}. Expected format: pixi-build-[name]-v[version]")

def verify_name_and_version(tag, cargo_name, cargo_version):
    tag_name, tag_version = extract_name_and_version_from_tag(tag)

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

    git_tag = get_git_tag()
    tagged_package = False

    # Extract bin names, versions, and generate env and recipe names
    matrix = []

    if not "packages" in metadata:
        raise ValueError("No packages found using cargo metadata")

    for package in metadata["packages"]:
        if any(target["kind"][0] == "bin" for target in package.get("targets", [])):
            if git_tag:
                # verify that the git tag matches the package version
                tag_name, tag_version = extract_name_and_version_from_tag(git_tag)
                if package["name"] != tag_name:
                    continue  # Skip packages that do not match the tag

                if package["version"] != tag_version:
                    raise ValueError(f"Version mismatch: Git tag version {tag_version} does not match Cargo version {package["version"]} for {package["name"]}")

                tagged_package = True


            for target in targets:
                matrix.append({
                    "bin": package["name"],
                    "version": package["version"],
                    "env_name": package["name"].replace("-", "_").upper() + "_VERSION",
                    "recipe_name": package["name"].replace("-", "_"),
                    "target": target["target"],
                    "os": target["os"]
                })

    if git_tag and not tagged_package:
        raise ValueError(f"Git tag {git_tag} does not match any package in Cargo.toml")


    matrix_json = json.dumps(matrix)


    print(matrix_json)

if __name__ == "__main__":
    generate_matrix()
