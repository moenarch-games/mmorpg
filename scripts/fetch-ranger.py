#!/usr/bin/env python3
"""Acquire selected entries of both live rolling Ranger figure releases; keep each local copy while its release is unchanged."""
import hashlib
import http.client
import json
import os
from pathlib import Path
import shutil
import ssl
import sys
import tempfile
import time
import urllib.error
import urllib.request
from lib.ranger_assets import CLASS_NAMES, valid_male_appearance

ROOT = Path(__file__).resolve().parents[1]
URL = "https://github.com/moritzbrantner/asset-tooling/releases/download/{release}/"
API = "https://api.github.com/repos/moritzbrantner/asset-tooling/releases/"
# The male figure's selected entries by key and variants: the default character and equipment, the class weapons
# (asset-tooling #375) and props (#379), the Greyhaven residents' outfit variants, armour, straw hat and beards (#418), the class
# starter outfits' armour (#379), the Redbrand Crossbowmen's crossbow with an arrow standing in for its bolt (#607), and
# the Rogue's throwing knife, venom flask and blasting bomb (asset-tooling #467, #611).
MALE = {key: {"default"} for key in (
    "humanoid.ranger.male", "equipment.sword.arming", "equipment.shield.heater", "equipment.bow.long",
    "equipment.quiver.back", "equipment.cap.ranger", "equipment.helm.ironcrest", "equipment.dagger.blackened",
    "equipment.staff.bone", "equipment.staff.crystal", "equipment.staff.gnarled", "equipment.book.tome",
    "equipment.mace.flanged", "equipment.hat.straw", "equipment.crossbow", "equipment.arrow",
    "equipment.throwing-knife", "equipment.venom-flask", "equipment.blasting-bomb", "facial-hair.beards", "armor.knight", "armor.soldier",
    "armor.farmer", "armor.innkeeper", "armor.monk", *(f"armor.class.{name}" for name in CLASS_NAMES))}
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
# Tanner Hilda Brook's `resident.tanner` outfit in the light skin with its armour (#493),
# plus the base outfit requested by the figure loader before the skin variant is applied.
FEMALE = {"humanoid.ranger.female": {"default", "outfit.tanner", "outfit.tanner.skin.light"}, "armor.tanner": {"default"}}
# Female players use the same real carried-item/socket path, from their own release.
FEMALE.update({key: {"default"} for key in MALE if key.startswith("equipment.")
               and key not in {"equipment.cap.ranger", "equipment.helm.ironcrest", "equipment.hat.straw"}})
# Plus every hairstyle a release offers, in its tintable variant: the viewer's styles and the
# varied guards' (#376, asset-tooling #380), and the painted head skins when the release has them
# (asset-tooling #380).
HAIR_VARIANT = "hair-1"
HEAD_SKINS = ("skin.head", "default")
# The wait before each retry of a transient release failure (REL-003).
RETRY_DELAYS = (1, 2, 4, 8, 16)


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
        self.asset_ids = None


FIGURES = (Figure("humanoid.ranger.male", MALE), Figure("humanoid.ranger.female", FEMALE))


def selected(figure, index):
    """The selected (key, variant) pairs of a release index."""
    # Female knight entries are selected only when the rolling catalog actually provides them.
    knight = {(entry.get("key"), entry.get("variant")) for entry in index.get("entries", [])
              if figure.release == "humanoid-ranger-female" and
              ((entry.get("key") == "humanoid.ranger.female" and entry.get("variant") in {"outfit.knight", "outfit.knight.skin.light"})
               or (entry.get("key"), entry.get("variant")) == ("armor.knight", "default"))}
    return figure.pairs | knight | {(entry.get("key"), entry.get("variant")) for entry in index.get("entries", [])
                           if (str(entry.get("key")).startswith("hair.") and entry.get("variant") == HAIR_VARIANT)
                           or (entry.get("key"), entry.get("variant")) == HEAD_SKINS}


def github_token():
    """The token selecting authenticated release downloads, or None for the public URLs."""
    return os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN") or None


def read_url(url, timeout, accept=None, token=None):
    """The body at `url`, retrying 5xx responses, timeouts and connection errors; never a 4xx."""
    request = urllib.request.Request(url)
    if accept:
        request.add_header("Accept", accept)
    if token:
        # Unredirected: release assets redirect to a storage host, which must not see the token.
        request.add_unredirected_header("Authorization", f"Bearer {token}")
    for delay in (*RETRY_DELAYS, None):
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                return response.read()
        except urllib.error.HTTPError as error:
            error.close()
            if error.code < 500 or delay is None:
                raise
            failure = error
        except (OSError, http.client.HTTPException) as error:  # Includes URLError, timeouts and truncated bodies.
            if delay is None or isinstance(getattr(error, "reason", None), ssl.SSLCertVerificationError):
                raise
            failure = error
        print(f"Retrying {url} in {delay} s after: {failure}", file=sys.stderr)
        time.sleep(delay)
    raise AssertionError("unreachable: the final attempt returns or raises")


def read_asset(figure, name, timeout):
    """Release asset `name`: through the REST API with a token, and through the public URL
    without one or when the API fails, for example on a rate limit."""
    token = github_token()
    if token:
        try:
            if figure.asset_ids is None:
                release = json.loads(read_url(f"{API}tags/{figure.release}", 60, "application/vnd.github+json", token))
                figure.asset_ids = {asset["name"]: asset["id"] for asset in release["assets"]}
            return read_url(f"{API}assets/{figure.asset_ids[name]}", timeout, "application/octet-stream", token)
        except (OSError, http.client.HTTPException, KeyError, ValueError) as error:
            print(f"Authenticated download of {figure.release}/{name} failed ({error!r}); trying the public URL.", file=sys.stderr)
    return read_url(figure.url + name, timeout)


def live_ref(figure):
    """The live release's export.ref.json, or None when the release cannot be reached."""
    try:
        return read_asset(figure, "export.ref.json", 60)
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
    if {(entry["key"], entry["variant"]) for entry in entries} != pairs or not all(
        (dest / name).is_file() for entry in entries for name in entry["files"]
    ):
        return False
    if figure.release == "humanoid-ranger-male":
        default = next((entry for entry in entries if
                        (entry["key"], entry["variant"]) == ("humanoid.ranger.male", "default")), None)
        appearances = ([name for name in default["files"] if name.endswith(".humanoid-appearance.json")]
                       if default else [])
        if len(appearances) != 1 or not valid_male_appearance(dest / appearances[0]):
            return False
    return True


def acquire(figure):
    # A copy of an older upload, or missing a selected entry, is fetched again. Offline, a complete
    # copy of any upload is kept, except in CI, where it may be a restored cache of an older upload.
    ref = live_ref(figure)
    if ref is None and os.environ.get("GITHUB_ACTIONS") == "true":
        raise OSError(f"could not reach the {figure.release} release")
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
            (directory / name).write_bytes(read_asset(figure, name, 300))

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


def print_identity(figure):
    """Print the figure's cache identity line (`scripts/ci-asset-cache.sh`), failing when the release cannot be reached."""
    ref = live_ref(figure)
    if ref is None:
        raise OSError(f"could not reach the {figure.release} release")
    print(figure.release, hashlib.sha256(ref).hexdigest(), figure.dest.relative_to(ROOT))


if __name__ == "__main__":
    print(f"Ranger downloads: {'authenticated GitHub API' if github_token() else 'anonymous public URLs'}", file=sys.stderr)
    for figure in FIGURES:
        if sys.argv[1:] == ["--identity"]:
            print_identity(figure)
        else:
            acquire(figure)
