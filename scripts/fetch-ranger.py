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
        "equipment.bow.long", "equipment.quiver.back", "equipment.cap.ranger", "equipment.helm.ironcrest"}


def complete():
    """The local copy is this release and holds every file of the selected entries."""
    if not (DEST / "RELEASE").is_file() or (DEST / "RELEASE").read_text().strip() != RELEASE:
        return False
    try:
        index = json.loads((DEST / (RELEASE + ".index.json")).read_text())
    except (OSError, ValueError):
        return False
    entries = [entry for entry in index.get("entries", []) if entry.get("key") in KEYS and entry.get("variant") == "default"]
    return {entry["key"] for entry in entries} == KEYS and all((DEST / name).is_file() for entry in entries for name in entry["files"])


def acquire():
    # The rolling release gains entries; a copy missing a selected one is fetched again.
    if complete():
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
