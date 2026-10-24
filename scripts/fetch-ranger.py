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
# Selected entries by key and variants: the default character and equipment, the class weapons
# (asset-tooling #375), the hairstyles' tintable `hair-1` texture variant (#376) and the Greyhaven
# residents' outfit variants, armour, straw hat and beards (#418).
SELECTED = {key: {"default"} for key in (
    "humanoid.ranger.male", "equipment.sword.arming", "equipment.shield.heater", "equipment.bow.long",
    "equipment.quiver.back", "equipment.cap.ranger", "equipment.helm.ironcrest", "equipment.dagger.blackened",
    "equipment.staff.bone", "equipment.staff.gilded", "equipment.staff.gnarled", "equipment.hat.straw",
    "facial-hair.beards", "armor.knight", "armor.soldier", "armor.farmer", "armor.innkeeper", "armor.monk")}
SELECTED.update({key: {"hair-1"} for key in ("hair.buzzed", "hair.simple-parted", "hair.long")})
# The full-detail outfit variant of each resident preset's outfit and skin.
SELECTED["humanoid.ranger.male"] |= {"outfit.knight.skin.light", "outfit.soldier", "outfit.innkeeper.skin.light",
                                     "outfit.farmer", "outfit.monk.skin.light"}
PAIRS = {(key, variant) for key, variants in SELECTED.items() for variant in variants}
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
    entries = [entry for entry in index.get("entries", []) if (entry.get("key"), entry.get("variant")) in PAIRS]
    return {(entry["key"], entry["variant"]) for entry in entries} == PAIRS and all((DEST / name).is_file() for entry in entries for name in entry["files"])


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
        entries = [entry for entry in index["entries"] if (entry["key"], entry["variant"]) in PAIRS]
        if len(entries) != len(PAIRS) or {(entry["key"], entry["variant"]) for entry in entries} != PAIRS:
            raise ValueError("index must declare every selected entry exactly once")
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
