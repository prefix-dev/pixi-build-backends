#!/usr/bin/env python3
"""
Cross-platform equivalent of:
  find ./target/ -type f -executable -name 'pixi-build*' -exec cp {} $PREFIX/bin \;

- Recursively scans ./target for files named 'pixi-build*'
- Keeps only executables (POSIX: x bit; Windows: PATHEXT)
- Copies matches into $PREFIX/bin
"""
from pathlib import Path
import os
import sys
import shutil

def is_executable(path: Path) -> bool:
    if not path.is_file():
        return False
    if os.name == "nt":
        pathext = os.environ.get("PATHEXT", ".EXE;.BAT;.CMD;.COM;.PS1").lower().split(";")
        return path.suffix.lower() in pathext
    return os.access(str(path), os.X_OK)

def main() -> int:
    prefix = os.environ.get("PREFIX")
    if not prefix:
        print("error: $PREFIX is not set", file=sys.stderr)
        return 1

    src_root = Path("target")
    if not src_root.exists():
        # match `find` behavior: no matches -> silent, but here we inform and exit 0
        return 0

    dest_dir = Path(prefix) / "bin"
    dest_dir.mkdir(parents=True, exist_ok=True)

    copied_any = False
    for p in src_root.rglob("pixi-build*"):
        if is_executable(p):
            shutil.copy2(p, dest_dir / p.name)
            copied_any = True

    # Exit 0 even if nothing matched, mirroring `find -exec cp …` behavior
    return 0 if copied_any or True else 1  # pragma: no cover

if __name__ == "__main__":
    raise SystemExit(main())
