#!/usr/bin/env python3
"""Acquire selected rolling Ranger inputs; retain the local copy until explicitly removed."""
import json
from pathlib import Path
import shutil
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "web/public/characters/humanoid.ranger.male"
RELEASE = "humanoid-ranger-male"
URL = f"https://github.com/moritzbrantner/asset-tooling/releases/download/{RELEASE}/"
KEYS = {"humanoid.ranger.male", "equipment.sword.arming", "equipment.shield.heater",
        "equipment.bow.long", "equipment.quiver.back"}


def acquire():
    if (DEST / "RELEASE").is_file() and (DEST / "RELEASE").read_text().strip() == RELEASE:
        return
    DEST.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".ranger-", dir=DEST.parent) as temporary:
        directory = Path(temporary)

        def download(name):
            if Path(name).name != name or not name or name in {".", ".."}:
                raise ValueError(f"unsafe release filename: {name}")
            with urllib.request.urlopen(URL + name, timeout=300) as response:
                (directory / name).write_bytes(response.read())

        index_name = RELEASE + ".index.json"
        download(index_name)
        index = json.loads((directory / index_name).read_text())
        if index["schema"] != "asset-downloads-v1" or index["release"] != RELEASE:
            raise ValueError("unsupported Ranger index")
        entries = [entry for entry in index["entries"] if entry["key"] in KEYS and entry["variant"] == "default"]
        if len(entries) != len(KEYS) or {entry["key"] for entry in entries} != KEYS:
            raise ValueError("index must declare every selected default entry exactly once")
        files = {"export.ref.json"}
        for entry in entries:
            files.update(entry["files"])
            files.update(entry["textures"])
            if entry.get("clips"):
                files.add(entry["clips"])
        for name in sorted(files):
            download(name)
        (directory / "RELEASE").write_text(RELEASE + "\n")
        if DEST.exists():
            shutil.rmtree(DEST)
        directory.rename(DEST)
    print(f"Fetched selected {RELEASE} entries into {DEST}")


if __name__ == "__main__":
    acquire()
