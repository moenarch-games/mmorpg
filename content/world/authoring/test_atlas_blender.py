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

    # region files: two neighbours in one file, sharing the border between them
    borders = data("borders.json")
    shared = next(bd for bd in borders["borders"] if bd.get("from") and bd.get("to") and len(bd["points"]) > 4)
    adapter.cmd_import_regions(shared["regions"], work / "pair.blend")
    before = snapshot()
    adapter.cmd_export()
    expect(snapshot() == before, "an untouched region file leaves every source byte-identical")

    def find(kind, oid):
        return next(o for o in bpy.data.objects if o.get("atlas") == kind and o.get("id") == oid)

    curve = find("border", shared["id"])
    curve.data.splines[0].points[2].co.x += 0.05
    adapter.cmd_export()
    after = data("borders.json")
    moved = {bd["id"]: bd for bd in after["borders"]}
    expect(moved[shared["id"]]["points"][2] == [shared["points"][2][0] + 50, shared["points"][2][1]]
           and all(moved[bd["id"]] == bd for bd in borders["borders"] if bd["id"] != shared["id"])
           and [n for n in snapshot() if snapshot()[n] != before[n]] == ["borders.json"],
           "reshaping a border changes that border only, once, for both regions")

    j = shared["from"]
    find("junction", j).location.y -= 0.03
    adapter.cmd_export()
    at = next(x["at"] for x in data("borders.json")["junctions"] if x["id"] == j)
    ends = [bd["points"][0] if bd.get("from") == j else bd["points"][-1]
            for bd in data("borders.json")["borders"] if j in (bd.get("from"), bd.get("to"))]
    expect(len(ends) >= 3 and all(e == at for e in ends),
           f"moving junction {j} moves the ends of all {len(ends)} borders meeting there, also outside the file")

    newer = data("borders.json")  # someone edits the same border from another file
    next(bd for bd in newer["borders"] if bd["id"] == shared["id"])["points"][3][0] += 10
    adapter.write_json(world / "borders.json", newer)
    curve.data.splines[0].points[3].co.x += 0.05
    try:
        adapter.cmd_export()
        refused = False
    except SystemExit:
        refused = True
    expect(refused, "an outdated file refuses to overwrite a border edited elsewhere since it was made")
finally:
    shutil.rmtree(work)
