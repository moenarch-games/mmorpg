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
# Selected entries by key and variant: the default character and equipment, and the hairstyles'
# tintable `hair-1` texture variant (#376).
SELECTED = {key: "default" for key in ("humanoid.ranger.male", "equipment.sword.arming", "equipment.shield.heater",
                                       "equipment.bow.long", "equipment.quiver.back", "equipment.cap.ranger",
                                       "equipment.helm.ironcrest")}
SELECTED.update({key: "hair-1" for key in ("hair.buzzed", "hair.simple-parted", "hair.long")})
KEYS = set(SELECTED)
# The rolling release is pinned by the consumer's copy of its export.ref.json (#411: the bundle
# with face morph targets); a replaced release fails acquisition until this copy is updated.
EXPORT_REF = ROOT / "web/assets/ranger/export.ref.json"


def complete():
    """The local copy is the pinned export of this release and holds every file of the selected entries."""
    if not (DEST / "RELEASE").is_file() or (DEST / "RELEASE").read_text().strip() != RELEASE:
        return False
    if not (DEST / "export.ref.json").is_file() or (DEST / "export.ref.json").read_bytes() != EXPORT_REF.read_bytes():
        return False
    try:
        index = json.loads((DEST / (RELEASE + ".index.json")).read_text())
    except (OSError, ValueError):
        return False
    entries = [entry for entry in index.get("entries", []) if SELECTED.get(entry.get("key")) == entry.get("variant")]
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
        entries = [entry for entry in index["entries"] if SELECTED.get(entry["key"]) == entry["variant"]]
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
        if (directory / "export.ref.json").read_bytes() != EXPORT_REF.read_bytes():
            raise ValueError(f"{RELEASE} is not the pinned export ({EXPORT_REF.relative_to(ROOT)})")
        (directory / "RELEASE").write_text(RELEASE + "\n")
        if DEST.exists():
            shutil.rmtree(DEST)
        directory.rename(DEST)
    print(f"Fetched selected {RELEASE} entries into {DEST}")


if __name__ == "__main__":
    acquire()
