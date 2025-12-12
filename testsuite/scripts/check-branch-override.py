#!/usr/bin/env python3
"""
Script to check for CI override files that shouldn't be merged to main.

This script ensures that .env.ci files used for testing PR combinations
don't accidentally get merged to the main branch.
"""

import sys
from pathlib import Path


def main() -> None:
    """Check if CI override files exist and exit with error if found."""
    repo_root = Path(__file__).parent.parent
    override_file = repo_root / ".env.ci"

    if override_file.exists():
        print("❌ ERROR: .env.ci file detected")
        print("This file is used for testing PR combinations and should not be merged to main")
        print("Please remove .env.ci from your branch")
        sys.exit(1)
    else:
        print("✅ No CI override files detected - safe to merge")
        sys.exit(0)


if __name__ == "__main__":
    main()
