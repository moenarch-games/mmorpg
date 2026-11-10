"""Headless round-trip test of the atlas Blender adapter, on a copy of content/world/.

    blender --background --python-exit-code 1 --python content/world/authoring/test_atlas_blender.py
"""

import importlib.util
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

import bpy

HERE = Path(__file__).resolve().parent
SOURCES = HERE.parent
work = Path(tempfile.mkdtemp(prefix="atlas-blender-"))
world = work / "world"
shutil.copytree(SOURCES, world, ignore=shutil.ignore_patterns("authoring"))
os.environ["ATLAS_WORLD"] = str(world)
spec = importlib.util.spec_from_file_location("atlas_blender", HERE / "atlas_blender.py")
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


def snapshot():
    return {p.name: p.read_bytes() for p in sorted(world.glob("*.json"))}


def data(name):
    return json.loads((world / name).read_text())


def expect(ok, message):
    if not ok:
        raise AssertionError(message)
    print("ok  ", message)


try:
    blend = work / "continent.blend"
    original = snapshot()
    adapter.cmd_import(blend)
    adapter.cmd_export()
    expect(snapshot() == original, "an untouched round trip leaves every source byte-identical")

    before_regions = data("regions.json")
    before_terrain = data("terrain.json")
    before_water = data("water.json")
    objs = bpy.data.objects
    objs["region r010"].location.x += 0.25
    copy = objs["region r020"].copy()
    copy.location.y -= 0.3
    objs["region r020"].users_collection[0].objects.link(copy)
    objs["river vess"].location.x += 0.1
    objs["crown centre"].location.x += 0.1
    adapter.cmd_export()

    regions = data("regions.json")
    seeds = {r["id"]: r["seed"] for r in regions["regions"]}
    old = {r["id"]: r["seed"] for r in before_regions["regions"]}
    expect(seeds["r010"] == [old["r010"][0] + 250, old["r010"][1]], "a moved seed moves by exactly its offset")
    new = [rid for rid in seeds if rid not in old]
    expect(len(new) == 1 and seeds[new[0]] == [old["r020"][0], old["r020"][1] + 300] and seeds["r020"] == old["r020"],
           f"a copied seed becomes a new region ({new}) and leaves the original in place")
    vess = next(r for r in data("water.json")["rivers"] if r["id"] == "vess")["points"]
    vess0 = next(r for r in before_water["rivers"] if r["id"] == "vess")["points"]
    expect(vess == [[x + 100, z] for x, z in vess0], "moving a whole river object moves every point")
    terrain = data("terrain.json")
    expect(terrain["crown"]["center"] == [before_terrain["crown"]["center"][0] + 100, before_terrain["crown"]["center"][1]]
           and [g["bearing"] for g in terrain["crown"]["gates"]] == [g["bearing"] for g in before_terrain["crown"]["gates"]],
           "moving the Crown keeps its gates' bearings")
    expect(snapshot()["polities.json"] == original["polities.json"], "untouched files stay byte-identical")

    dup = objs["settlement carrow"].copy()
    objs["settlement carrow"].users_collection[0].objects.link(dup)
    try:
        adapter.cmd_export()
        refused = False
    except SystemExit:
        refused = True
    expect(refused, "a copied settlement is refused instead of silently overwriting the original")
finally:
    shutil.rmtree(work)
