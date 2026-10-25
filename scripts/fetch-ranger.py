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
# (asset-tooling #375) and props (#379), the Greyhaven residents' outfit variants, armour, straw hat and beards (#418) and the class
# starter outfits' armour (#379).
CLASSES = ("warden", "ranger", "arcanist", "necromancer", "rogue", "cleric", "druid")
SELECTED = {key: {"default"} for key in (
    "humanoid.ranger.male", "equipment.sword.arming", "equipment.shield.heater", "equipment.bow.long",
    "equipment.quiver.back", "equipment.cap.ranger", "equipment.helm.ironcrest", "equipment.dagger.blackened",
    "equipment.staff.bone", "equipment.staff.crystal", "equipment.staff.gnarled", "equipment.book.tome",
    "equipment.mace.flanged", "equipment.hat.straw", "facial-hair.beards", "armor.knight", "armor.soldier",
    "armor.farmer", "armor.innkeeper", "armor.monk", *(f"armor.class.{name}" for name in CLASSES))}
# The full-detail outfit variant of each resident and class preset's outfit and skin, and the
# guards' and the varied enemies' outfits in both skins (each draws his own look,
# asset-tooling #380; the enemy presets, #39, wear the soldier, knight and farmer outfits).
SELECTED["humanoid.ranger.male"] |= {"outfit.knight", "outfit.knight.skin.light", "outfit.soldier", "outfit.soldier.skin.light",
                                     "outfit.innkeeper.skin.light",
                                     "outfit.farmer", "outfit.farmer.skin.light", "outfit.monk.skin.light",
                                     "outfit.class.warden.skin.light", "outfit.class.ranger",
                                     "outfit.class.arcanist.skin.light", "outfit.class.necromancer.skin.grey",
                                     "outfit.class.rogue", "outfit.class.cleric.skin.light", "outfit.class.druid"}
PAIRS = {(key, variant) for key, variants in SELECTED.items() for variant in variants}
# Plus every hairstyle the release offers, in its tintable variant: the viewer's styles and the
# varied guards' (#376, asset-tooling #380), and the painted head skins when the release has them
# (asset-tooling #380).
HAIR_VARIANT = "hair-1"
HEAD_SKINS = ("skin.head", "default")


def selected(index):
    """The selected (key, variant) pairs of a release index."""
    return PAIRS | {(entry.get("key"), entry.get("variant")) for entry in index.get("entries", [])
                    if (str(entry.get("key")).startswith("hair.") and entry.get("variant") == HAIR_VARIANT)
                    or (entry.get("key"), entry.get("variant")) == HEAD_SKINS}
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
    pairs = selected(index)
    entries = [entry for entry in index.get("entries", []) if (entry.get("key"), entry.get("variant")) in pairs]
    return {(entry["key"], entry["variant"]) for entry in entries} == pairs and all((DEST / name).is_file() for entry in entries for name in entry["files"])


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
        pairs = selected(index)
        entries = [entry for entry in index["entries"] if (entry["key"], entry["variant"]) in pairs]
        if len(entries) != len(pairs) or {(entry["key"], entry["variant"]) for entry in entries} != pairs:
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
