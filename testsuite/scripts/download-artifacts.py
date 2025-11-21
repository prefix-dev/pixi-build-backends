import argparse
import itertools
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import zipfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

import httpx
from dotenv import load_dotenv
from github import Github
from github.Artifact import Artifact
from github.PaginatedList import PaginatedList
from rich.console import Console
from rich.progress import track

console = Console()


@dataclass(frozen=True)
class ArtifactTarget:
    # Repo the target is located in
    repo: str
    # The workflow that we use for downloading
    workflow: str
    # The PR that contains the target
    pr_number: str | None


def get_current_platform() -> str:
    """Get the current platform string for pixi artifact naming."""
    system = platform.system().lower()
    machine = platform.machine().lower()

    if system == "linux":
        if machine in ["x86_64", "amd64"]:
            return "linux-x86_64"
        elif machine in ["aarch64", "arm64"]:
            return "linux-aarch64"
    elif system == "darwin":
        if machine in ["arm64", "aarch64"]:
            return "macos-aarch64"
        elif machine in ["x86_64", "amd64"]:
            return "macos-x86_64"
    elif system == "windows":
        if machine in ["x86_64", "amd64"]:
            return "windows-x86_64"

    raise ValueError(f"Unsupported platform: {system}-{machine}")


def download_and_extract_artifact(
    target_artifact: Artifact, github_token: str | None, output_dir: Path, repo: str
) -> None:
    """Download and extract artifact, return path to extracted binary."""
    # Download the artifact
    console.print("[blue]Downloading artifact...")
    download_url = target_artifact.archive_download_url

    # Use httpx to download with authentication and follow redirects
    headers = {}
    if github_token:
        headers["Authorization"] = f"token {github_token}"

    with httpx.stream(
        "GET", download_url, headers=headers, follow_redirects=True, timeout=30.0
    ) as response:
        response.raise_for_status()

        # Save to temporary file and extract
        with tempfile.NamedTemporaryFile(suffix=".zip", delete=False) as temp_file:
            total_size = int(response.headers.get("content-length", 0))

            if total_size > 0:
                for chunk in track(
                    response.iter_bytes(chunk_size=8192),
                    total=total_size // 8192,
                    description="Downloading...",
                ):
                    temp_file.write(chunk)
            else:
                for chunk in response.iter_bytes(chunk_size=8192):
                    temp_file.write(chunk)

            temp_zip_path = temp_file.name

    console.print("[blue]Extracting artifact...")

    # Extract the zip file
    with zipfile.ZipFile(temp_zip_path, "r") as zip_ref:
        # List contents
        file_list = zip_ref.namelist()
        console.print(f"[blue]Archive contents: {file_list}")

        if repo == "prefix-dev/pixi":
            # Find the pixi binary
            pixi_binary = None
            for file_name in file_list:
                if (
                    file_name == "pixi"
                    or file_name.endswith("/pixi")
                    or file_name.endswith("pixi.exe")
                ):
                    pixi_binary = file_name
                    break

            if not pixi_binary:
                console.print("[red]Could not find pixi binary in archive")
                raise FileNotFoundError(
                    f"Could not find pixi binary in archive. Archive contents: {file_list}"
                )

            final_path = output_dir / Path(pixi_binary).name
            if final_path.exists():
                if final_path.is_dir():
                    shutil.rmtree(final_path)
                else:
                    final_path.unlink()

            # Extract the binary
            zip_ref.extract(pixi_binary, output_dir)

            # Move to correct location if it was in a subdirectory
            extracted_path = output_dir / pixi_binary

            if extracted_path != final_path:
                extracted_path.rename(final_path)

            # Make executable on Unix systems
            if not sys.platform.startswith("win"):
                final_path.chmod(0o755)

            console.print(f"[green]Successfully downloaded pixi binary to: {final_path}")

        elif repo == "prefix-dev/pixi-build-backends":
            # Find the pixi binary
            is_channel = None
            for file_name in file_list:
                if file_name.endswith("repodata.json"):
                    is_channel = True
                    break

            if not is_channel:
                console.print("[red]Could not locate a channel directory inside the artifact.")
                raise FileNotFoundError("Could not locate a channel directory inside the artifact.")

            console.print("[blue]Detected backend channel artifact")
            final_channel_path = output_dir / "pixi-build-backends"
            if final_channel_path.exists():
                console.print(f"[yellow]Removing existing channel at {final_channel_path}")
                shutil.rmtree(final_channel_path)

            final_channel_path.parent.mkdir(parents=True, exist_ok=True)
            zip_ref.extractall(final_channel_path)

            console.print(f"[green]Channel is ready at: {final_channel_path}")
        else:
            raise ValueError(f"Unsupported repository: {repo}")

    # Clean up temporary file
    os.unlink(temp_zip_path)


def get_matching_artifact(
    artifacts: PaginatedList[Artifact], artifact_name_pattern: str
) -> Artifact | None:
    for artifact in artifacts:
        if artifact_name_pattern in artifact.name:
            return artifact
    return None


def load_env_files() -> None:
    """Load both .env and .env.ci files if they exist."""
    project_root = Path(__file__).parent.parent
    env_file = project_root / ".env"
    env_ci_file = project_root / ".env.ci"

    for env in [env_file, env_ci_file]:
        if env.exists():
            load_dotenv(env)
            console.print(f"[green]Loaded environment variables from {env}")


def get_token_from_gh() -> str | None:
    """Attempt to obtain a GitHub token via the GitHub CLI."""
    gh_executable = shutil.which("gh")
    if not gh_executable:
        console.print("[yellow]GitHub CLI not found; skipping GH auth token lookup")
        return None

    try:
        result = subprocess.run(
            [gh_executable, "auth", "token"],
            check=True,
            capture_output=True,
            text=True,
        )
    except subprocess.CalledProcessError as exc:
        console.print(
            f"[yellow]Failed to obtain token via GitHub CLI. Return code: {exc.returncode}"
        )
        return None

    token = result.stdout.strip()
    if not token:
        console.print("[yellow]GitHub CLI returned an empty token")
        return None

    console.print("[green]Using token from GitHub CLI authentication")
    return token


def write_metadata(
    output_dir: Path,
    repo: str,
    metadata: dict[str, object],
) -> None:
    metadata_path = output_dir / "download-metadata.json"
    existing: dict[str, object] = {}
    if metadata_path.exists():
        try:
            existing = json.loads(metadata_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            console.print(
                f"[yellow]Existing metadata file at {metadata_path} is invalid JSON; overwriting"
            )
    existing[repo] = metadata
    metadata_path.write_text(json.dumps(existing, indent=2, sort_keys=True), encoding="utf-8")


def download_github_artifact(
    github_token: str | None,
    output_dir: Path,
    repo: str,
    workflow: str,
    run_id: int | None = None,
    pr_number: int | None = None,
) -> None:
    # Get current platform
    current_platform = get_current_platform()

    # Initialize GitHub client
    gh = Github(github_token)

    # Get the repository
    repository = gh.get_repo(repo)
    console.print(f"[green]Connected to repository: {repository.full_name}")

    # Find the artifact for our platform
    if repo.endswith("/pixi"):
        artifact_name_pattern = f"pixi-{current_platform}"
    elif repo.endswith("/pixi-build-backends"):
        artifact_name_pattern = f"pixi-build-backends-{current_platform}"
    else:
        raise ValueError(f"Unsupported repository: {repo}")

    # Get the target_artifact
    target_artifact = None
    pr = None
    selected_run = None

    if run_id:
        # Use specific run ID - no need to find workflow first
        console.print(f"[blue]Using specified run ID: {run_id}")
        selected_run = repository.get_workflow_run(run_id)
        artifacts = selected_run.get_artifacts()
        target_artifact = get_matching_artifact(artifacts, artifact_name_pattern)
    elif pr_number:
        # Get workflow run from PR
        console.print(f"[blue]Finding workflow run for PR #{pr_number}")
        pr = repository.get_pull(pr_number)
        console.print(f"[blue]PR #{pr_number}: {pr.title} (head: {pr.head.sha})")

        # Get workflow runs for the PR's head commit
        workflows = repository.get_workflows()
        target_workflow = None
        for wf in workflows:
            if wf.name == workflow:
                target_workflow = wf
                break

        if not target_workflow:
            console.print(f"[red]Could not find workflow: {workflow}")
            raise ValueError(f"Could not find workflow: {workflow}")

        console.print(f"[blue]Found workflow: {target_workflow.name}")

        # Get workflow runs for the PR head commit
        runs = target_workflow.get_runs(head_sha=pr.head.sha)
        for selected_run in itertools.islice(runs, 3):
            artifacts = selected_run.get_artifacts()
            target_artifact = get_matching_artifact(artifacts, artifact_name_pattern)
            if target_artifact:
                break
    else:
        # Get the latest workflow run for the specified workflow
        workflows = repository.get_workflows()
        target_workflow = None
        for wf in workflows:
            if wf.name == workflow:
                target_workflow = wf
                break

        if not target_workflow:
            console.print(f"[red]Could not find workflow: {workflow}")
            raise ValueError(f"Could not find workflow: {workflow}")

        console.print(f"[blue]Found workflow: {target_workflow.name}")

        # Get latest workflow run from main branch
        console.print("[blue]Finding latest workflow run from main branch")
        runs = target_workflow.get_runs(branch="main", event="push")
        # Check the past five runs until a suitable candidate is found
        for selected_run in itertools.islice(runs, 3):
            artifacts = selected_run.get_artifacts()
            target_artifact = get_matching_artifact(artifacts, artifact_name_pattern)
            if target_artifact:
                break

    if selected_run is None:
        raise ValueError("Could not find a suitable workflow run")

    console.print(f"[blue]Selected run: {selected_run.id} from {selected_run.created_at}")

    if not target_artifact:
        console.print(f"[red]Could not find artifact matching pattern '{artifact_name_pattern}'")
        console.print("[yellow]Available artifacts:")
        available_artifacts = [artifact.name for artifact in artifacts]
        for artifact in artifacts:
            console.print(f"  - {artifact.name}")
        raise FileNotFoundError(
            f"Could not find artifact matching pattern '{artifact_name_pattern}'. "
            f"Available artifacts: {available_artifacts}"
        )

    console.print(f"[green]Found artifact: {target_artifact.name}")

    # Set up output directory
    output_dir.mkdir(parents=True, exist_ok=True)
    console.print(f"[blue]Output directory: {output_dir}")

    # Download and extract the artifact
    download_and_extract_artifact(target_artifact, github_token, output_dir, repo)

    metadata: dict[str, object] = {
        "artifact": target_artifact.name,
        "downloaded_at": datetime.now(timezone.utc).isoformat(),
        "run_id": selected_run.id,
        "head_sha": selected_run.head_sha,
        "workflow": workflow,
    }

    if pr_number is not None:
        metadata["source"] = "pr"
        metadata["pr_number"] = pr_number
        if pr is not None:
            metadata["pr_title"] = pr.title
            metadata["head_ref"] = pr.head.ref
            metadata["head_label"] = pr.head.label
    else:
        metadata["source"] = "branch"
        metadata["branch"] = getattr(selected_run, "head_branch", None) or "main"

    write_metadata(output_dir, repo, metadata)


def main() -> None:
    # Load environment files
    load_env_files()

    parser = argparse.ArgumentParser(description="Download artifacts from GitHub Actions")
    parser.add_argument(
        "--token",
        help="GitHub token for authentication (can also use GITHUB_TOKEN env var or .env file)",
    )
    parser.add_argument(
        "--run-id",
        type=int,
        help="Specific workflow run ID to download from (optional). Requires --repo.",
    )
    parser.add_argument(
        "--repo",
        choices=["pixi", "pixi-build-backends"],
        help="Restrict download to a single repository. By default both pixi and pixi-build-backends artifacts are fetched.",
    )

    args = parser.parse_args()

    if args.repo is None and args.run_id is not None:
        console.print("[red][ERROR] --run-id can only be used together with --repo")
        sys.exit(1)

    targets: list[ArtifactTarget]
    if args.repo == "pixi":
        targets = [ArtifactTarget("prefix-dev/pixi", "CI", os.getenv("PIXI_PR_NUMBER"))]
    elif args.repo == "pixi-build-backends":
        targets = [
            ArtifactTarget(
                "prefix-dev/pixi-build-backends",
                "Testsuite",
                os.getenv("BUILD_BACKENDS_PR_NUMBER"),
            )
        ]
    else:
        targets = [
            ArtifactTarget("prefix-dev/pixi", "CI", os.getenv("PIXI_PR_NUMBER")),
            ArtifactTarget(
                "prefix-dev/pixi-build-backends",
                "Testsuite",
                os.getenv("BUILD_BACKENDS_PR_NUMBER"),
            ),
        ]

    # Store binaries directly under "artifacts" for compatibility with older tooling
    output_dir = Path("artifacts")

    # Get GitHub token from argument or environment
    github_token = args.token or os.getenv("GITHUB_TOKEN") or get_token_from_gh()
    if not github_token:
        console.print("[red][ERROR] No GitHub token provided")
        console.print(
            "[red]  Set GITHUB_TOKEN environment variable, use --token argument, or create a .env file"
        )
        sys.exit(1)

    overall_success = True
    for target in targets:
        pr_number = target.pr_number
        pr_number_int = int(pr_number) if pr_number and pr_number.isdigit() else None
        if pr_number_int:
            console.print(f"[yellow]Using PR #{pr_number_int} from {target.repo}")

        try:
            download_github_artifact(
                github_token,
                output_dir,
                target.repo,
                target.workflow,
                args.run_id,
                pr_number_int,
            )
        except Exception as e:  # noqa: BLE001 - surface context rich message
            overall_success = False
            console.print(f"[red][ERROR] Download failed for {target.repo}: {e}")

    if not overall_success:
        sys.exit(1)

    console.print("[green][SUCCESS] Download completed successfully!")
    sys.exit(0)


if __name__ == "__main__":
    main()
