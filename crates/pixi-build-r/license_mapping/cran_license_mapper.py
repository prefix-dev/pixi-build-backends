#!/usr/bin/env python3
"""
Script to analyze CRAN package licenses and map them to SPDX identifiers.

This script:
1. Fetches the CRAN package database
2. Extracts all unique license strings
3. Maps them to proper SPDX identifiers
"""

import gzip
import re
from collections import Counter
from io import BytesIO
from urllib.request import urlopen

# Known mappings from CRAN license strings to SPDX identifiers
# See https://spdx.org/licenses/ for the full list
CRAN_TO_SPDX = {
    # GPL variants
    "GPL": "GPL-1.0-or-later",
    "GPL-2": "GPL-2.0-only",
    "GPL-3": "GPL-3.0-only",
    "GPL (>= 2)": "GPL-2.0-or-later",
    "GPL (>= 3)": "GPL-3.0-or-later",
    "GPL (> 2)": "GPL-3.0-or-later",  # Greater than 2 means 3+
    "GPL (<= 2)": "GPL-2.0-only",
    "GPL (== 2)": "GPL-2.0-only",
    "GPL (== 3)": "GPL-3.0-only",
    "GPL (== 3.0)": "GPL-3.0-only",
    "GPL-2 | GPL-3": "GPL-2.0-only OR GPL-3.0-only",
    "GPL (>= 2.0)": "GPL-2.0-or-later",
    "GPL (>= 2.0.0)": "GPL-2.0-or-later",
    "GPL (>= 2.1)": "GPL-2.0-or-later",  # 2.1 rounds to 2.0-or-later
    "GPL (>= 3.0)": "GPL-3.0-or-later",
    "GNU General Public License": "GPL-1.0-or-later",
    "GNU General Public License (>= 2)": "GPL-2.0-or-later",
    "GNU General Public License (>= 3)": "GPL-3.0-or-later",
    "GNU General Public License version 2": "GPL-2.0-only",
    "GNU General Public License version 3": "GPL-3.0-only",
    # LGPL variants
    "LGPL": "LGPL-2.0-or-later",
    "LGPL-2": "LGPL-2.0-only",
    "LGPL-2.1": "LGPL-2.1-only",
    "LGPL-3": "LGPL-3.0-only",
    "LGPL (>= 2)": "LGPL-2.0-or-later",
    "LGPL (>= 2.0)": "LGPL-2.0-or-later",
    "LGPL (>= 2.0, < 3)": "LGPL-2.0-only OR LGPL-2.1-only",
    "LGPL (>= 2.1)": "LGPL-2.1-or-later",
    "LGPL (>= 3)": "LGPL-3.0-or-later",
    "LGPL (>= 3.0)": "LGPL-3.0-or-later",
    # AGPL variants
    "AGPL": "AGPL-3.0-only",
    "AGPL-3": "AGPL-3.0-only",
    "AGPL (>= 3)": "AGPL-3.0-or-later",
    # MIT
    "MIT": "MIT",
    "MIT + file LICENSE": "MIT",
    "MIT + file LICENCE": "MIT",
    "MIT License": "MIT",
    "MIT License + file LICENSE": "MIT",
    # BSD variants
    "BSD_2_clause": "BSD-2-Clause",
    "BSD_2_clause + file LICENSE": "BSD-2-Clause",
    "BSD_2_clause + file LICENCE": "BSD-2-Clause",
    "BSD_3_clause": "BSD-3-Clause",
    "BSD_3_clause + file LICENSE": "BSD-3-Clause",
    "BSD_3_clause + file LICENCE": "BSD-3-Clause",
    "BSD 2 clause": "BSD-2-Clause",
    "BSD 3 clause": "BSD-3-Clause",
    "BSD-2-Clause": "BSD-2-Clause",
    "BSD-3-Clause": "BSD-3-Clause",
    "BSD 2-clause License + file LICENSE": "BSD-2-Clause",
    "BSD 3-clause License + file LICENSE": "BSD-3-Clause",
    "FreeBSD": "BSD-2-Clause-FreeBSD",
    # Apache
    "Apache License": "Apache-2.0",
    "Apache License 2.0": "Apache-2.0",
    "Apache License (== 2)": "Apache-2.0",
    "Apache License (== 2.0)": "Apache-2.0",
    "Apache License (>= 2)": "Apache-2.0",
    "Apache License (>= 2.0)": "Apache-2.0",
    "Apache-2.0": "Apache-2.0",
    # Artistic
    "Artistic-2.0": "Artistic-2.0",
    "Artistic License 2.0": "Artistic-2.0",
    # CC licenses
    "CC0": "CC0-1.0",
    "CC BY 4.0": "CC-BY-4.0",
    "CC BY-SA 4.0": "CC-BY-SA-4.0",
    "CC BY-NC 4.0": "CC-BY-NC-4.0",
    "CC BY-NC-SA 4.0": "CC-BY-NC-SA-4.0",
    "Creative Commons Attribution 4.0 International License": "CC-BY-4.0",
    # MPL
    "MPL": "MPL-2.0",
    "MPL-2.0": "MPL-2.0",
    "MPL (>= 2.0)": "MPL-2.0",
    "Mozilla Public License 2.0": "MPL-2.0",
    # Other common licenses
    "Unlimited": "LicenseRef-Unlimited",
    "Lucent Public License": "LPL-1.02",
    "CeCILL": "CECILL-2.1",
    "CeCILL-2": "CECILL-2.0",
    "CeCILL (>= 2)": "CECILL-2.0",
    "BSL": "BSL-1.0",
    "BSL-1.0": "BSL-1.0",
    "Boost Software License 1.0": "BSL-1.0",
    "EPL": "EPL-1.0",
    "CPL-1.0": "CPL-1.0",
    "Common Public License Version 1.0": "CPL-1.0",
    "EUPL": "EUPL-1.2",
    "EUPL-1.1": "EUPL-1.1",
    "EUPL-1.2": "EUPL-1.2",
    "EUPL (>= 1.1)": "EUPL-1.1",
    "EUPL (>= 1.2)": "EUPL-1.2",
    "ISC": "ISC",
    "Zlib": "Zlib",
    # Public domain (not strictly SPDX but commonly used)
    "Public domain": "LicenseRef-PublicDomain",
    # Part of R
    "Part of R": "LicenseRef-R",
    # ACM license
    "ACM": "LicenseRef-ACM",
    # Proprietary / restrictive
    "file LICENSE": "LicenseRef-file-LICENSE",
    "file LICENCE": "LicenseRef-file-LICENSE",
}


def fetch_cran_packages():
    """Fetch the PACKAGES.gz file from CRAN and parse it."""
    url = "https://cran.r-project.org/src/contrib/PACKAGES.gz"
    print(f"Fetching {url}...")

    with urlopen(url) as response:
        compressed = BytesIO(response.read())

    with gzip.open(compressed, "rt", encoding="utf-8") as f:
        content = f.read()

    return content


def parse_packages(content):
    """Parse the PACKAGES file and extract license information."""
    packages = []
    current_package = {}

    for line in content.split("\n"):
        if line == "":
            if current_package:
                packages.append(current_package)
                current_package = {}
        elif line.startswith(" ") or line.startswith("\t"):
            # Continuation of previous field
            if "last_field" in current_package and current_package["last_field"]:
                current_package[current_package["last_field"]] += " " + line.strip()
        elif ":" in line:
            key, value = line.split(":", 1)
            current_package[key.strip()] = value.strip()
            current_package["last_field"] = key.strip()

    if current_package:
        packages.append(current_package)

    return packages


def normalize_license(license_str):
    """Normalize a license string for comparison."""
    # Remove extra whitespace
    normalized = " ".join(license_str.split())
    return normalized


def extract_license_parts(license_str):
    """Extract individual license components from a compound license string."""
    # Handle common patterns like "GPL-2 | GPL-3" or "MIT + file LICENSE"
    parts = []

    # Split on | for OR conditions
    or_parts = re.split(r"\s*\|\s*", license_str)

    for part in or_parts:
        # Handle "+ file LICENSE" patterns
        file_match = re.match(r"^(.+?)\s*\+\s*file\s+(LICENSE|LICENCE)$", part, re.I)
        if file_match:
            base_license = file_match.group(1).strip()
            parts.append(f"{base_license} + file LICENSE")
        else:
            parts.append(part.strip())

    return parts


def map_to_spdx(license_str):
    """Map a CRAN license string to SPDX identifier(s)."""
    normalized = normalize_license(license_str)

    # Direct match
    if normalized in CRAN_TO_SPDX:
        return CRAN_TO_SPDX[normalized]

    # Try to handle compound licenses
    parts = extract_license_parts(normalized)
    if len(parts) > 1:
        spdx_parts = []
        for part in parts:
            if part in CRAN_TO_SPDX:
                spdx_parts.append(CRAN_TO_SPDX[part])
            else:
                # Unknown part
                spdx_parts.append(f"LicenseRef-{sanitize_license_ref(part)}")
        return " OR ".join(spdx_parts)

    # Handle file LICENSE pattern
    file_match = re.match(r"^(.+?)\s*\+\s*file\s+(LICENSE|LICENCE)$", normalized, re.I)
    if file_match:
        base = file_match.group(1).strip()
        if base in CRAN_TO_SPDX:
            return CRAN_TO_SPDX[base]
        base_with_file = f"{base} + file LICENSE"
        if base_with_file in CRAN_TO_SPDX:
            return CRAN_TO_SPDX[base_with_file]

    # Unknown license - create a LicenseRef
    return f"LicenseRef-{sanitize_license_ref(normalized)}"


def sanitize_license_ref(s):
    """Sanitize a string for use in LicenseRef-."""
    # SPDX LicenseRef can only contain alphanumeric, -, and .
    sanitized = re.sub(r"[^a-zA-Z0-9\-.]", "-", s)
    # Remove consecutive dashes
    sanitized = re.sub(r"-+", "-", sanitized)
    # Remove leading/trailing dashes
    sanitized = sanitized.strip("-")
    return sanitized or "Unknown"


def cran_to_spdx(license_str: str) -> str:
    """
    Convert a CRAN license string to an SPDX identifier.

    Args:
        license_str: The license string from a CRAN package DESCRIPTION file.

    Returns:
        The SPDX license identifier, or a LicenseRef- identifier for unknown licenses.

    Example:
        >>> cran_to_spdx("GPL-2")
        'GPL-2.0-only'
        >>> cran_to_spdx("MIT + file LICENSE")
        'MIT'
        >>> cran_to_spdx("LGPL-2.1")
        'LGPL-2.1-only'
    """
    return map_to_spdx(license_str)


def get_mapping() -> dict:
    """Return the CRAN to SPDX mapping dictionary."""
    return CRAN_TO_SPDX.copy()


def output_json_mapping():
    """Output the mapping as JSON."""
    import json
    print(json.dumps(CRAN_TO_SPDX, indent=2, sort_keys=True))


def output_rust_code():
    """Output Rust HashMap code for the mapping."""
    print("use std::collections::HashMap;")
    print("")
    print("pub fn cran_to_spdx_map() -> HashMap<&'static str, &'static str> {")
    print("    let mut map = HashMap::new();")

    for cran, spdx in sorted(CRAN_TO_SPDX.items()):
        print(f'    map.insert("{cran}", "{spdx}");')

    print("    map")
    print("}")


def main():
    import argparse

    parser = argparse.ArgumentParser(description="Analyze CRAN licenses and map to SPDX")
    parser.add_argument("--json", action="store_true", help="Output mapping as JSON")
    parser.add_argument("--rust", action="store_true", help="Output Rust HashMap code only")
    args = parser.parse_args()

    if args.json:
        output_json_mapping()
        return

    if args.rust:
        output_rust_code()
        return

    content = fetch_cran_packages()
    packages = parse_packages(content)

    print(f"\nFound {len(packages)} packages\n")

    # Extract and count licenses
    license_counter = Counter()
    for pkg in packages:
        if "License" in pkg:
            license_str = normalize_license(pkg["License"])
            license_counter[license_str] += 1

    # Sort by frequency
    sorted_licenses = license_counter.most_common()

    print("=" * 80)
    print("CRAN License Analysis")
    print("=" * 80)
    print(f"\nTotal unique license strings: {len(sorted_licenses)}\n")

    # Print mapping table
    print("-" * 80)
    print(f"{'CRAN License':<45} | {'SPDX':<30} | Count")
    print("-" * 80)

    unmapped = []
    for license_str, count in sorted_licenses[:100]:  # Top 100
        spdx = map_to_spdx(license_str)
        is_mapped = not spdx.startswith("LicenseRef-") or spdx in [
            "LicenseRef-Unlimited",
            "LicenseRef-PublicDomain",
            "LicenseRef-R",
            "LicenseRef-ACM",
            "LicenseRef-file-LICENSE",
        ]

        if not is_mapped:
            unmapped.append((license_str, count))

        marker = "" if is_mapped else " *"
        print(f"{license_str[:44]:<45} | {spdx[:29]:<30} | {count}{marker}")

    print("-" * 80)
    print("\n* = No standard SPDX mapping found\n")

    # Generate Rust code for the mapping
    print("\n" + "=" * 80)
    print("Rust HashMap for CRAN -> SPDX mapping")
    print("=" * 80 + "\n")
    output_rust_code()

    # Print statistics
    print("\n" + "=" * 80)
    print("Statistics")
    print("=" * 80)

    total_mapped = sum(count for lic, count in sorted_licenses if not map_to_spdx(lic).startswith("LicenseRef-") or map_to_spdx(lic) in ["LicenseRef-Unlimited", "LicenseRef-PublicDomain", "LicenseRef-R", "LicenseRef-ACM", "LicenseRef-file-LICENSE"])
    total_packages = sum(count for _, count in sorted_licenses)

    print(f"\nPackages with mapped licenses: {total_mapped}/{total_packages} ({100*total_mapped/total_packages:.1f}%)")
    print(f"Unique unmapped license strings: {len(unmapped)}")

    if unmapped:
        print("\nTop unmapped licenses (may need manual mapping):")
        for lic, count in unmapped[:20]:
            print(f"  - {lic} ({count} packages)")


if __name__ == "__main__":
    main()
