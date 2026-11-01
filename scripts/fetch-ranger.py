#!/usr/bin/env python3
"""Acquire selected entries of both live rolling Ranger figure releases; keep each local copy while its release is unchanged."""
import json
from pathlib import Path
import shutil
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
URL = "https://github.com/moritzbrantner/asset-tooling/releases/download/{release}/"
# The male figure's selected entries by key and variants: the default character and equipment, the class weapons
# (asset-tooling #375) and props (#379), the Greyhaven residents' outfit variants, armour, straw hat and beards (#418), the class
# starter outfits' armour (#379), and the Redbrand Crossbowmen's crossbow with an arrow standing in for its bolt (#607).
CLASSES = ("warden", "ranger", "arcanist", "necromancer", "rogue", "cleric", "druid")
MALE = {key: {"default"} for key in (
    "humanoid.ranger.male", "equipment.sword.arming", "equipment.shield.heater", "equipment.bow.long",
    "equipment.quiver.back", "equipment.cap.ranger", "equipment.helm.ironcrest", "equipment.dagger.blackened",
    "equipment.staff.bone", "equipment.staff.crystal", "equipment.staff.gnarled", "equipment.book.tome",
    "equipment.mace.flanged", "equipment.hat.straw", "equipment.crossbow", "equipment.arrow", "facial-hair.beards", "armor.knight", "armor.soldier",
    "armor.farmer", "armor.innkeeper", "armor.monk", *(f"armor.class.{name}" for name in CLASSES))}
# The full-detail outfit variant of each resident and class preset's outfit and skin, and the
# guards' and the varied enemies' outfits in both skins (each draws his own look,
# asset-tooling #380; the enemy presets, #39, wear the soldier, knight and farmer outfits).
MALE["humanoid.ranger.male"] |= {"outfit.knight", "outfit.knight.skin.light", "outfit.soldier", "outfit.soldier.skin.light",
                                 "outfit.innkeeper.skin.light",
                                 "outfit.farmer", "outfit.farmer.skin.light", "outfit.monk.skin.light",
                                 "outfit.class.warden.skin.light", "outfit.class.ranger",
                                 "outfit.class.arcanist.skin.light", "outfit.class.necromancer.skin.grey",
                                 "outfit.class.rogue", "outfit.class.cleric.skin.light", "outfit.class.druid"}
# The female figure (asset-tooling #378): her default entry (sockets, clips and appearance) and
# Tanner Hilda Brook's `resident.tanner` outfit in the light skin with its armour (#493).
FEMALE = {"humanoid.ranger.female": {"default", "outfit.tanner.skin.light"}, "armor.tanner": {"default"}}
# Plus every hairstyle a release offers, in its tintable variant: the viewer's styles and the
# varied guards' (#376, asset-tooling #380), and the painted head skins when the release has them
# (asset-tooling #380).
HAIR_VARIANT = "hair-1"
HEAD_SKINS = ("skin.head", "default")


class Figure:
    """One Ranger figure's rolling release (`imported::Figure`), acquired into its own directory.

    The release is read at whatever it currently holds, unpinned (#507): the local copy is keyed
    on the live release's export.ref.json, so a new upload refreshes it.
    """

    def __init__(self, character, selected):
        self.release = character.replace(".", "-")
        self.dest = ROOT / "web/public/characters" / character
        self.url = URL.format(release=self.release)
        self.pairs = {(key, variant) for key, variants in selected.items() for variant in variants}


FIGURES = (Figure("humanoid.ranger.male", MALE), Figure("humanoid.ranger.female", FEMALE))


def selected(figure, index):
    """The selected (key, variant) pairs of a release index."""
    return figure.pairs | {(entry.get("key"), entry.get("variant")) for entry in index.get("entries", [])
                           if (str(entry.get("key")).startswith("hair.") and entry.get("variant") == HAIR_VARIANT)
                           or (entry.get("key"), entry.get("variant")) == HEAD_SKINS}


def live_ref(figure):
    """The live release's export.ref.json, or None when the release cannot be reached."""
    try:
        with urllib.request.urlopen(figure.url + "export.ref.json", timeout=60) as response:
            return response.read()
    except OSError:
        return None


def complete(figure, ref):
    """The local copy holds export `ref` (any export when None) and every file of the selected entries."""
    dest = figure.dest
    if not (dest / "RELEASE").is_file() or (dest / "RELEASE").read_text().strip() != figure.release:
        return False
    if not (dest / "export.ref.json").is_file() or (ref is not None and (dest / "export.ref.json").read_bytes() != ref):
        return False
    try:
        index = json.loads((dest / (figure.release + ".index.json")).read_text())
    except (OSError, ValueError):
        return False
    pairs = selected(figure, index)
    entries = [entry for entry in index.get("entries", []) if (entry.get("key"), entry.get("variant")) in pairs]
    return {(entry["key"], entry["variant"]) for entry in entries} == pairs and all((dest / name).is_file() for entry in entries for name in entry["files"])


def acquire(figure):
    # A copy of an older upload, or missing a selected entry, is fetched again. Offline, a complete
    # copy of any upload is kept.
    ref = live_ref(figure)
    if complete(figure, ref):
        return
    release, dest = figure.release, figure.dest
    if ref is None:
        raise OSError(f"could not reach the {release} release")
    dest.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".ranger-", dir=dest.parent) as temporary:
        directory = Path(temporary)

        def download(name):
            if Path(name).name != name or not name or name in {".", ".."}:
                raise ValueError(f"unsafe release filename: {name}")
            with urllib.request.urlopen(figure.url + name, timeout=300) as response:
                (directory / name).write_bytes(response.read())

        index_name = release + ".index.json"
        download(index_name)
        index = json.loads((directory / index_name).read_text())
        if index["schema"] != "asset-downloads-v1" or index["release"] != release:
            raise ValueError(f"unsupported {release} index")
        pairs = selected(figure, index)
        entries = [entry for entry in index["entries"] if (entry["key"], entry["variant"]) in pairs]
        if len(entries) != len(pairs) or {(entry["key"], entry["variant"]) for entry in entries} != pairs:
            raise ValueError("index must declare every selected entry exactly once")
        (directory / "export.ref.json").write_bytes(ref)
        files = set()
        for entry in entries:
            files.update(entry["files"])
            files.update(entry["textures"])
            if entry.get("clips"):
                files.add(entry["clips"])
        for name in sorted(files):
            download(name)
        (directory / "RELEASE").write_text(release + "\n")
        if dest.exists():
            shutil.rmtree(dest)
        directory.rename(dest)
    print(f"Fetched selected {release} entries into {dest}")


if __name__ == "__main__":
    for figure in FIGURES:
        acquire(figure)
