#!/usr/bin/env python3
"""Package the built native game and optional host art without fetching or building assets."""
import argparse
from pathlib import Path
import shutil


def main():
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--bin-dir", type=Path, default=root / "target/debug")
    parser.add_argument("--without-optional-art", action="store_true",
                        help="omit optional animal files; invited guests receive verified detail from the host")
    args = parser.parse_args()
    destination = args.destination.resolve()
    if destination.exists():
        raise SystemExit("Choose a new destination; existing installations are not overwritten.")
    binaries = [args.bin_dir / name for name in ("mmorpg-client", "mmorpg-zone-host")]
    for binary in binaries:
        if not binary.is_file():
            raise SystemExit(f"Build both native binaries before packaging: {binary}")
    destination.mkdir(parents=True)
    for binary in binaries:
        shutil.copy2(binary, destination / binary.name)
    if not args.without_optional_art:
        shutil.copytree(root / "web/public/scenery/greyhaven.ambient-animals",
                        destination / "assets/ambient")
    print(destination)


if __name__ == "__main__":
    main()
