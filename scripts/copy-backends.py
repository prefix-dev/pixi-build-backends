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

def main() -> None:
    prefix = os.environ["PREFIX"]

    src_root = Path("./target/channel-build-cache")
    if not src_root.exists():
        return

    dest_dir = Path(prefix) / "bin"
    dest_dir.mkdir(parents=True, exist_ok=True)

    for p in src_root.rglob("pixi-build*"):
        if is_executable(p):
            shutil.copy2(p, dest_dir / p.name)

if __name__ == "__main__":
    main()
