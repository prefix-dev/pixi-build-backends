#!/usr/bin/env python3
"""
Validate pixi-build-r dependency detection against conda-forge feedstocks.

This script:
1. Downloads DESCRIPTION files from CRAN for selected packages
2. Parses dependencies using our logic
3. Compares against conda-forge recipe requirements
"""

import json
import re
import subprocess
import tarfile
import tempfile
from io import BytesIO
from pathlib import Path
from urllib.request import urlopen

# R built-in packages (base + recommended)
R_BUILTIN_PACKAGES = [
    # Base packages
    "base", "compiler", "datasets", "graphics", "grDevices",
    "grid", "methods", "parallel", "splines", "stats",
    "stats4", "tcltk", "tools", "utils",
    # Recommended packages
    "KernSmooth", "MASS", "Matrix", "boot", "class",
    "cluster", "codetools", "foreign", "lattice", "mgcv",
    "nlme", "nnet", "rpart", "spatial", "survival",
]

# Packages to validate (popular R packages with conda-forge recipes)
TEST_PACKAGES = [
    "ggplot2",
    "dplyr",
    "tidyr",
    "stringr",
    "Rcpp",
    "data.table",
    "jsonlite",
    "httr",
    "xml2",
    "curl",
    "testthat",
    "knitr",
    "rmarkdown",
    "shiny",
    "devtools",
]


def fetch_cran_description(package_name: str) -> str:
    """Fetch DESCRIPTION file from CRAN for a package."""
    # Get package info from CRAN
    url = f"https://cran.r-project.org/web/packages/{package_name}/DESCRIPTION"
    try:
        with urlopen(url, timeout=10) as response:
            return response.read().decode("utf-8")
    except Exception as e:
        print(f"  Warning: Could not fetch from CRAN web: {e}")
        return None


def parse_description(content: str) -> dict:
    """Parse DESCRIPTION file in DCF format."""
    data = {}
    current_key = None
    current_value = ""

    for line in content.split("\n"):
        if not line:
            continue

        # Continuation line
        if line.startswith(" ") or line.startswith("\t"):
            if current_key:
                current_value += " " + line.strip()
        elif ":" in line:
            # Save previous
            if current_key:
                data[current_key] = current_value.strip()

            colon_pos = line.index(":")
            current_key = line[:colon_pos].strip()
            current_value = line[colon_pos + 1 :].strip()

    if current_key:
        data[current_key] = current_value.strip()

    return data


def parse_r_dependencies(dep_string: str) -> list:
    """Parse R dependency string into list of (name, version) tuples."""
    if not dep_string:
        return []

    deps = []
    # Split on comma, handling parentheses
    parts = re.split(r",\s*(?![^()]*\))", dep_string)

    for part in parts:
        part = part.strip()
        if not part:
            continue

        # Match package name and optional version
        match = re.match(r"^(\S+)\s*(?:\(([^)]+)\))?", part)
        if match:
            name = match.group(1)
            version = match.group(2)
            deps.append((name, version))

    return deps


def r_package_to_conda(name: str) -> str:
    """Convert R package name to conda package name."""
    if name == "R":
        return "r-base"
    return f"r-{name.lower()}"


def is_builtin(name: str) -> bool:
    """Check if package is built into R."""
    return name.lower() in [p.lower() for p in R_BUILTIN_PACKAGES]


def get_our_dependencies(description: dict) -> dict:
    """Extract dependencies using our logic."""
    result = {
        "host": set(),
        "run": set(),
        "build": set(),
    }

    # Always add r-base
    result["host"].add("r-base")
    result["run"].add("r-base")

    # Process Imports
    imports = parse_r_dependencies(description.get("Imports", ""))
    for name, version in imports:
        if name == "R" or is_builtin(name):
            continue
        conda_name = r_package_to_conda(name)
        result["host"].add(conda_name)
        result["run"].add(conda_name)

    # Process Depends (excluding R)
    depends = parse_r_dependencies(description.get("Depends", ""))
    for name, version in depends:
        if name == "R" or is_builtin(name):
            continue
        conda_name = r_package_to_conda(name)
        result["host"].add(conda_name)
        result["run"].add(conda_name)

    # Process LinkingTo (host only)
    linking_to = parse_r_dependencies(description.get("LinkingTo", ""))
    for name, version in linking_to:
        if name == "R" or is_builtin(name):
            continue
        conda_name = r_package_to_conda(name)
        result["host"].add(conda_name)

    # Check for native code
    has_native = bool(description.get("NeedsCompilation", "").lower() == "yes")
    has_linking = bool(description.get("LinkingTo", ""))

    if has_native or has_linking:
        result["build"].add("${{ compiler('c') }}")
        result["build"].add("${{ compiler('cxx') }}")
        result["build"].add("${{ compiler('fortran') }}")

    return result


def fetch_conda_forge_recipe(package_name: str) -> dict:
    """Fetch recipe from conda-forge feedstock."""
    conda_name = f"r-{package_name.lower()}"
    url = f"https://raw.githubusercontent.com/conda-forge/{conda_name}-feedstock/main/recipe/meta.yaml"

    try:
        with urlopen(url, timeout=10) as response:
            content = response.read().decode("utf-8")
            return parse_conda_recipe(content)
    except Exception as e:
        print(f"  Warning: Could not fetch conda-forge recipe: {e}")
        return None


def parse_conda_recipe(content: str) -> dict:
    """Parse conda recipe requirements (simplified)."""
    result = {
        "host": set(),
        "run": set(),
        "build": set(),
    }

    current_section = None
    in_requirements = False

    for line in content.split("\n"):
        stripped = line.strip()

        if stripped == "requirements:":
            in_requirements = True
            continue

        if in_requirements:
            if stripped.startswith("build:"):
                current_section = "build"
            elif stripped.startswith("host:"):
                current_section = "host"
            elif stripped.startswith("run:"):
                current_section = "run"
            elif stripped.startswith("- ") and current_section:
                dep = stripped[2:].strip()
                # Remove version constraints and jinja
                dep = re.sub(r"\s+.*", "", dep)
                dep = re.sub(r"\{\{.*\}\}", "", dep).strip()
                if dep and not dep.startswith("#"):
                    result[current_section].add(dep)
            elif not stripped.startswith("-") and not stripped.startswith("#") and stripped and not stripped.startswith("{"):
                # End of requirements section
                if current_section and not stripped.endswith(":"):
                    in_requirements = False
                    current_section = None

    return result


def compare_dependencies(ours: dict, theirs: dict, package_name: str):
    """Compare our dependencies with conda-forge."""
    print(f"\n{'='*60}")
    print(f"Package: {package_name}")
    print(f"{'='*60}")

    for section in ["build", "host", "run"]:
        our_deps = ours.get(section, set())
        their_deps = theirs.get(section, set()) if theirs else set()

        # Normalize for comparison (lowercase, remove version specs)
        our_normalized = {d.lower().split()[0] for d in our_deps}
        their_normalized = {d.lower().split()[0] for d in their_deps}

        missing = their_normalized - our_normalized
        extra = our_normalized - their_normalized

        print(f"\n{section.upper()}:")
        print(f"  Ours:   {sorted(our_normalized)}")
        print(f"  Theirs: {sorted(their_normalized)}")

        if missing:
            # Filter out some expected differences
            missing = {m for m in missing if not m.startswith("cross-") and m not in ["sed", "coreutils", "make", "pkg-config", "posix"]}
            if missing:
                print(f"  MISSING: {sorted(missing)}")

        if extra:
            # Filter compiler macros
            extra = {e for e in extra if not e.startswith("$")}
            if extra:
                print(f"  EXTRA:   {sorted(extra)}")


def main():
    print("Validating pixi-build-r against conda-forge feedstocks")
    print("=" * 60)

    results = {"matches": 0, "mismatches": 0, "errors": 0}

    for package in TEST_PACKAGES:
        print(f"\nProcessing {package}...")

        # Fetch DESCRIPTION from CRAN
        description_content = fetch_cran_description(package)
        if not description_content:
            results["errors"] += 1
            continue

        description = parse_description(description_content)

        # Get our dependencies
        our_deps = get_our_dependencies(description)

        # Get conda-forge dependencies
        cf_deps = fetch_conda_forge_recipe(package)

        # Compare
        compare_dependencies(our_deps, cf_deps, package)

    print(f"\n{'='*60}")
    print("Summary")
    print(f"{'='*60}")
    print(f"Processed: {len(TEST_PACKAGES)} packages")


if __name__ == "__main__":
    main()
