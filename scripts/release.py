import subprocess
import os
import re
import tomllib
import questionary

# Configuration
AVAILABLE_BACKENDS = [
    "pixi-build-cmake",
    "pixi-build-python",
    "pixi-build-rattler-build",
    "pixi-build-rust",
]
TARGET_REPO_URL_SSH = "git@github.com:prefix-dev/pixi-build-backends.git"
TARGET_REPO_URL_HTTPS = "https://github.com/prefix-dev/pixi-build-backends.git"


def run_command(command_list, error_message_prefix="Command failed", cwd=None):
    """Executes a shell command and returns its stdout."""
    try:
        result = subprocess.run(
            command_list, capture_output=True, text=True, check=True, cwd=cwd
        )
        return result.stdout.strip()
    except subprocess.CalledProcessError as e:
        print(f"{error_message_prefix}: {e}")
        print(f"Stderr: {e.stderr.strip()}")
        raise
    except FileNotFoundError:
        print(
            f"Error: Command '{command_list[0]}' not found. Is it installed and in PATH?"
        )
        raise


def get_git_remotes():
    """Gets a dictionary of remote names and their push URLs."""
    try:
        output = run_command(["git", "remote", "-v"], "Failed to get git remotes")
        lines = output.strip().split("\n")
        remotes = {}
        for line in lines:
            if not line:
                continue
            parts = line.split()
            if len(parts) == 3 and parts[2] == "(push)":
                remotes[parts[0]] = parts[1]
        return remotes
    except Exception as e:
        print(f"Error parsing git remotes: {e}")
        return None


def find_target_remote(remotes):
    """Finds the remote name matching the target repository URLs."""
    if not remotes:
        return None
    for name, url in remotes.items():
        if url == TARGET_REPO_URL_SSH or url == TARGET_REPO_URL_HTTPS:
            return name
    for name, url in remotes.items():
        if url.replace(".git", "") == TARGET_REPO_URL_SSH.replace(
            ".git", ""
        ) or url.replace(".git", "") == TARGET_REPO_URL_HTTPS.replace(".git", ""):
            return name
    return None


def get_version_from_cargo_toml(backend_name):
    """Reads the version from the backend's Cargo.toml file."""
    cargo_toml_path = os.path.join(backend_name, "Cargo.toml")
    if not os.path.isfile(cargo_toml_path):
        return None
    try:
        with open(cargo_toml_path, "r") as f:
            data = tomllib.load(f)
        return data.get("package", {}).get("version")
    except Exception as e:
        print(f"Error reading version from {cargo_toml_path}: {e}")
        return None


def update_versions(backends_to_process):
    """Step 1: Update versions for selected backends."""
    for backend_name in backends_to_process:
        print(f"\n--- Updating version for backend: {backend_name} ---")
        current_cargo_version = get_version_from_cargo_toml(backend_name)
        version_prompt_message = f"Enter the version for '{backend_name}' (e.g., 0.1.0)"
        if current_cargo_version:
            version_prompt_message += (
                f" (current in Cargo.toml: {current_cargo_version})"
            )

        version = questionary.text(
            version_prompt_message + ":",
            default=current_cargo_version if current_cargo_version else "",
            validate=lambda text: True
            if re.fullmatch(r"\d+\.\d+\.\d+([-\w.]*)?", text)
            else "Please enter a valid version (e.g., 0.1.0 or 1.0.0-alpha.1)",
        ).ask()

        if not version:
            print(f"No version entered for {backend_name}. Skipping.")
            continue

        print(f"Version for {backend_name} updated to {version} (not implemented).")


def create_pr():
    """Step 2: Create a pull request."""
    print("\n--- Creating a pull request ---")
    try:
        run_command(["gh", "pr", "create", "--fill"], "Failed to create pull request")
        print("Pull request created successfully.")
    except Exception as e:
        print(f"Error creating pull request: {e}")


def wait_for_pr_merge():
    """Step 3: Wait for the pull request to be reviewed and merged."""
    print("\n--- Waiting for pull request to be reviewed and merged ---")
    print("Please monitor the pull request manually and press Enter once it is merged.")
    input("Press Enter to continue...")


def add_tags_and_push(backends_to_process):
    """Step 4: Add tags and push them."""
    print("\n--- Adding tags and pushing them ---")
    # Reuse the existing logic for tagging and pushing
    # ...


def main():
    print("Pixi Build Backends - Release Tagger Script")
    print("------------------------------------------")

    # Step selection
    steps = [
        "Update versions",
        "Create a pull request",
        "Wait for PR to be reviewed and merged",
        "Add tags and push them",
    ]
    selected_step = questionary.select(
        "Select the step to start from:", choices=steps
    ).ask()

    if not selected_step:
        print("No step selected. Exiting.")
        return

    # Check if in a git repository
    try:
        run_command(
            ["git", "rev-parse", "--is-inside-work-tree"],
            "Not a git repository check failed",
        )
    except Exception:
        print("Error: This script must be run from the root of a git repository.")
        return

    # Select backends to process
    choices = AVAILABLE_BACKENDS + [questionary.Separator(), "all"]
    selected_options = questionary.checkbox(
        "Select backends to process:", choices=choices
    ).ask()

    if not selected_options:
        print("No backends selected. Exiting.")
        return

    backends_to_process = (
        AVAILABLE_BACKENDS if "all" in selected_options else selected_options
    )

    # Execute steps based on selection
    if steps.index(selected_step) <= 0:
        update_versions(backends_to_process)
    if steps.index(selected_step) <= 1:
        create_pr()
    if steps.index(selected_step) <= 2:
        wait_for_pr_merge()
    if steps.index(selected_step) <= 3:
        add_tags_and_push(backends_to_process)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nOperation cancelled by user. Exiting.")
    except Exception as e:
        print(f"\nAn unexpected error occurred: {e}")
