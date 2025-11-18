#!/usr/bin/env python3
import argparse
import sys
from pathlib import Path
from shutil import rmtree


def main():
    parser = argparse.ArgumentParser(
        description="Remove {bld, build_cache, src_cache, test} inside OUTPUT_DIR."
    )
    parser.add_argument("output_dir", help="Path to the output directory")
    args = parser.parse_args()

    base = Path(args.output_dir).resolve()
    if not base.exists() or not base.is_dir():
        print("refusing: OUTPUT_DIR does not exist or is not a directory", file=sys.stderr)
        sys.exit(1)
    if base == Path(base.anchor):
        print("refusing: OUTPUT_DIR resolves to filesystem root", file=sys.stderr)
        sys.exit(1)

    for name in ("bld", "build_cache", "src_cache", "test"):
        p = base / name
        if p.exists():
            rmtree(p, ignore_errors=True)

    print("no problems what")


if __name__ == "__main__":
    main()
