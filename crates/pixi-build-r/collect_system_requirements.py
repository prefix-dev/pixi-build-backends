#!/usr/bin/env python3
"""
Collect SystemRequirements fields from all CRAN packages.

This script fetches DESCRIPTION files from CRAN and extracts SystemRequirements
to help build a mapping to conda packages.

Usage:
    python collect_system_requirements.py              # Print summary
    python collect_system_requirements.py --json       # Output JSON
    python collect_system_requirements.py --csv        # Output CSV
    python collect_system_requirements.py --fetch      # Fetch from CRAN (slow)
"""

import argparse
import csv
import json
import re
import sys
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import urlopen

# Cache file for collected data
CACHE_FILE = Path(__file__).parent / "system_requirements_cache.json"


def fetch_cran_packages_db() -> str:
    """Fetch the PACKAGES file from CRAN."""
    url = "https://cran.r-project.org/src/contrib/PACKAGES"
    print("Fetching CRAN package database...", file=sys.stderr)
    with urlopen(url, timeout=30) as response:
        return response.read().decode("utf-8")


def fetch_description(package_name: str) -> str | None:
    """Fetch DESCRIPTION file for a single package."""
    url = f"https://cran.r-project.org/web/packages/{package_name}/DESCRIPTION"
    try:
        with urlopen(url, timeout=10) as response:
            return response.read().decode("utf-8", errors="replace")
    except (HTTPError, URLError, TimeoutError):
        return None


def parse_description(content: str) -> dict:
    """Parse a DESCRIPTION file."""
    data = {}
    current_key = None
    current_value = ""

    for line in content.split("\n"):
        if not line:
            continue
        if line.startswith(" ") or line.startswith("\t"):
            if current_key:
                current_value += " " + line.strip()
        elif ":" in line:
            if current_key:
                data[current_key] = current_value.strip()
            colon_pos = line.index(":")
            current_key = line[:colon_pos].strip()
            current_value = line[colon_pos + 1 :].strip()

    if current_key:
        data[current_key] = current_value.strip()

    return data


def parse_packages_db(content: str) -> list[dict]:
    """Parse PACKAGES file in DCF format."""
    packages = []
    current = {}
    current_key = None
    current_value = ""

    for line in content.split("\n"):
        # Empty line marks end of package entry
        if not line.strip():
            if current_key and current_value:
                current[current_key] = current_value.strip()
            if current:
                packages.append(current)
            current = {}
            current_key = None
            current_value = ""
            continue

        # Continuation line (starts with whitespace)
        if line.startswith(" ") or line.startswith("\t"):
            if current_key:
                current_value += " " + line.strip()
        elif ":" in line:
            # Save previous field
            if current_key:
                current[current_key] = current_value.strip()

            colon_pos = line.index(":")
            current_key = line[:colon_pos].strip()
            current_value = line[colon_pos + 1 :].strip()

    # Don't forget last entry
    if current_key and current_value:
        current[current_key] = current_value.strip()
    if current:
        packages.append(current)

    return packages


def extract_system_requirements(packages: list[dict]) -> list[dict]:
    """Extract packages that have SystemRequirements."""
    results = []
    for pkg in packages:
        if "SystemRequirements" in pkg:
            results.append(
                {
                    "package": pkg.get("Package", ""),
                    "version": pkg.get("Version", ""),
                    "system_requirements": pkg["SystemRequirements"],
                }
            )
    return results


def normalize_requirement(req: str) -> str:
    """Normalize a system requirement string for grouping."""
    # Lowercase
    req = req.lower().strip()
    # Remove version constraints
    req = re.sub(r"\s*[\(\[].*?[\)\]]", "", req)
    req = re.sub(r"\s*>=?\s*[\d.]+", "", req)
    req = re.sub(r"\s*<=?\s*[\d.]+", "", req)
    # Remove package manager hints
    req = re.sub(r"\s*:\s*.*?(deb|rpm|brew).*", "", req, flags=re.IGNORECASE)
    # Clean up whitespace
    req = " ".join(req.split())
    return req


def analyze_requirements(results: list[dict]) -> dict:
    """Analyze and categorize system requirements."""
    # Count raw requirements
    raw_counts = Counter()
    normalized_counts = Counter()

    for r in results:
        raw_counts[r["system_requirements"]] += 1

        # Split by comma and normalize each part
        parts = re.split(r",\s*(?![^()]*\))", r["system_requirements"])
        for part in parts:
            part = part.strip()
            if part:
                normalized = normalize_requirement(part)
                if normalized:
                    normalized_counts[normalized] += 1

    return {
        "total_packages_with_sysreqs": len(results),
        "unique_raw_requirements": len(raw_counts),
        "unique_normalized_requirements": len(normalized_counts),
        "top_normalized": normalized_counts.most_common(50),
        "raw_counts": raw_counts,
    }


def fetch_all_system_requirements(packages: list[dict], max_workers: int = 10) -> list[dict]:
    """Fetch DESCRIPTION files for packages with NeedsCompilation=yes."""
    # Filter to packages that might have system requirements
    candidates = [
        p["Package"]
        for p in packages
        if p.get("NeedsCompilation", "no").lower() == "yes"
    ]
    print(f"Found {len(candidates)} packages with NeedsCompilation=yes", file=sys.stderr)

    results = []
    failed = 0

    def process_package(pkg_name: str) -> dict | None:
        content = fetch_description(pkg_name)
        if content:
            desc = parse_description(content)
            if "SystemRequirements" in desc:
                return {
                    "package": pkg_name,
                    "version": desc.get("Version", ""),
                    "system_requirements": desc["SystemRequirements"],
                }
        return None

    with ThreadPoolExecutor(max_workers=max_workers) as executor:
        futures = {executor.submit(process_package, pkg): pkg for pkg in candidates}
        for i, future in enumerate(as_completed(futures)):
            if (i + 1) % 100 == 0:
                print(
                    f"  Processed {i + 1}/{len(candidates)} packages...",
                    file=sys.stderr,
                )
            try:
                result = future.result()
                if result:
                    results.append(result)
            except Exception:
                failed += 1

    print(f"Found {len(results)} packages with SystemRequirements", file=sys.stderr)
    if failed:
        print(f"Failed to fetch {failed} packages", file=sys.stderr)

    return results


def save_cache(results: list[dict], total_packages: int):
    """Save results to cache file."""
    cache_data = {
        "fetched_at": time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()),
        "total_cran_packages": total_packages,
        "packages_with_system_requirements": len(results),
        "packages": results,
    }
    with open(CACHE_FILE, "w") as f:
        json.dump(cache_data, f, indent=2)
    print(f"Saved cache to {CACHE_FILE}", file=sys.stderr)


def load_cache() -> dict | None:
    """Load results from cache file."""
    if CACHE_FILE.exists():
        with open(CACHE_FILE) as f:
            return json.load(f)
    return None


def main():
    parser = argparse.ArgumentParser(
        description="Collect SystemRequirements from CRAN packages"
    )
    parser.add_argument("--json", action="store_true", help="Output full JSON")
    parser.add_argument("--csv", action="store_true", help="Output CSV")
    parser.add_argument(
        "--raw", action="store_true", help="Show raw requirements (not normalized)"
    )
    parser.add_argument(
        "--fetch",
        action="store_true",
        help="Fetch fresh data from CRAN (slow, uses cache otherwise)",
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=10,
        help="Number of parallel workers for fetching (default: 10)",
    )
    args = parser.parse_args()

    # Check for cached data
    cache = load_cache()

    if args.fetch or cache is None:
        # Fetch fresh data
        db_content = fetch_cran_packages_db()
        packages = parse_packages_db(db_content)
        print(f"Parsed {len(packages)} packages from CRAN", file=sys.stderr)

        results = fetch_all_system_requirements(packages, max_workers=args.workers)
        save_cache(results, len(packages))
        total_packages = len(packages)
    else:
        print(f"Using cached data from {cache['fetched_at']}", file=sys.stderr)
        results = cache["packages"]
        total_packages = cache["total_cran_packages"]

    if args.json:
        # Full JSON output
        output = {
            "total_cran_packages": total_packages,
            "packages_with_system_requirements": len(results),
            "packages": results,
        }
        print(json.dumps(output, indent=2))
    elif args.csv:
        # CSV output
        writer = csv.DictWriter(
            sys.stdout, fieldnames=["package", "version", "system_requirements"]
        )
        writer.writeheader()
        writer.writerows(results)
    else:
        # Summary analysis
        analysis = analyze_requirements(results)

        print(f"\n{'='*70}")
        print("CRAN SystemRequirements Analysis")
        print(f"{'='*70}")
        print(f"Total CRAN packages: {total_packages}")
        print(f"Packages with SystemRequirements: {analysis['total_packages_with_sysreqs']}")
        print(f"Unique raw requirement strings: {analysis['unique_raw_requirements']}")
        print(
            f"Unique normalized requirements: {analysis['unique_normalized_requirements']}"
        )

        print(f"\n{'='*70}")
        print("Top 50 System Requirements (normalized)")
        print(f"{'='*70}")
        for req, count in analysis["top_normalized"]:
            print(f"  {count:4d}  {req}")

        if args.raw:
            print(f"\n{'='*70}")
            print("All unique raw SystemRequirements strings")
            print(f"{'='*70}")
            for req, count in sorted(
                analysis["raw_counts"].items(), key=lambda x: -x[1]
            ):
                print(f"  {count:4d}  {req[:100]}{'...' if len(req) > 100 else ''}")


if __name__ == "__main__":
    main()
