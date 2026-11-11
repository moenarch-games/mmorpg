"""Headless round-trip test of the atlas Blender adapter, on a copy of content/world/.

    blender --background --python-exit-code 1 --python content/world/authoring/test_atlas_blender.py
"""

import importlib.util
import json
import math
import subprocess
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
    objs["region r010"].location.x += 0.05
    objs["river vess"].location.x += 0.1
    objs["crown centre"].location.x += 0.1
    adapter.cmd_export()

    regions = data("regions.json")
    seeds = {r["id"]: r["seed"] for r in regions["regions"]}
    old = {r["id"]: r["seed"] for r in before_regions["regions"]}
    expect(seeds["r010"] == [old["r010"][0] + 50, old["r010"][1]], "a moved seed moves by exactly its offset")
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

    def fresh():
        shutil.rmtree(world)
        shutil.copytree(SOURCES, world, ignore=shutil.ignore_patterns("authoring"))

    def geography_ok():
        """The generator's own checks on the edited copy."""
        run = subprocess.run(["uv", "run", str(HERE / "atlas.py"), "build", "--checks", "--out", str(work / "out")],
                             env={**os.environ, "ATLAS_WORLD": str(world)}, capture_output=True, text=True)
        failed = [line for line in run.stdout.splitlines() if line.startswith("FAIL")]
        return run.returncode == 0, failed or run.stderr[-400:]

    def mid(points):
        return points[len(points) // 2]

    # redraw: Blender's Draw tool makes a Bézier curve with few control points and long handles
    fresh()
    adapter.cmd_import_regions(["r100", "r113"], work / "redraw.blend")
    old = next(bd for bd in data("borders.json")["borders"] if bd["id"] == "b292")
    curve = find("border", "b292")
    curve.data.splines.remove(curve.data.splines[0])
    spline = curve.data.splines.new("BEZIER")
    spline.bezier_points.add(2)
    (ax, az), (bx, bz) = old["points"][0], old["points"][-1]
    for p, (x, z) in zip(spline.bezier_points, [(ax + 25, az - 15), ((ax + bx) / 2, (az + bz) / 2), (bx - 20, bz + 25)]):
        p.co = adapter.to_blender(x, z, 300)
        p.handle_left_type = p.handle_right_type = "FREE"
        p.handle_left = adapter.to_blender(x - (bx - ax) / 4, z - (bz - az) / 4 + 260, 300)
        p.handle_right = adapter.to_blender(x + (bx - ax) / 4, z + (bz - az) / 4 + 260, 300)
    adapter.cmd_export()
    new = next(bd for bd in data("borders.json")["borders"] if bd["id"] == "b292")["points"]

    bow = max(abs((mx - ax) * (bz - az) - (mz - az) * (bx - ax)) for mx, mz in new) / math.dist((ax, az), (bx, bz))
    expect(len(new) > 10 and bow > 60 and new[0] == old["points"][0] and new[-1] == old["points"][-1],
           f"a redrawn Bézier border follows the curve ({len(new)} points, bowing {bow:.0f} m), its ends on the junctions")

    # merge: delete the border between two regions and one of their seeds
    fresh()
    pair = ["r100", "r113"]
    adapter.cmd_import_regions(pair, work / "merge.blend")
    between = [bd for bd in data("borders.json")["borders"] if sorted(bd["regions"]) == pair]
    for bd in between:
        bpy.data.objects.remove(find("border", bd["id"]))
    bpy.data.objects.remove(find("region", "r113"))
    adapter.cmd_export()
    merged = data("borders.json")
    ok, failed = geography_ok()
    expect("r113" not in {r["id"] for r in data("regions.json")["regions"]}
           and not any("r113" in bd["regions"] for bd in merged["borders"])
           and not any(bd["id"] in {b["id"] for b in merged["borders"]} for bd in between) and ok,
           f"merging r100 and r113 deletes their border, renames r113's other borders, and the atlas checks pass {failed or ''}")
    try:
        adapter.cmd_export()
        refused = False
    except SystemExit:
        refused = True
    expect(refused, "after a merge the file is out of date and refuses further exports until it is made again")

    # split: draw a new border across a region and copy its seed into the new piece
    fresh()
    adapter.cmd_import_regions(["r113"], work / "split.blend")
    sb = {bd["id"]: bd for bd in data("borders.json")["borders"]}
    data_before = data("regions.json")
    a, b = mid(sb["b294"]["points"]), mid(sb["b325"]["points"])  # east and west sides of r113
    seed = find("region", "r113")
    copy = seed.copy()
    seed.users_collection[0].objects.link(copy)
    sx, sz = adapter.from_blender(seed.location)
    nx, nz = -(b[1] - a[1]), b[0] - a[0]  # the new seed goes to the side of the line away from the old one
    side = 1 if (sx - a[0]) * nx + (sz - a[1]) * nz < 0 else -1
    n = math.hypot(nx, nz)
    mx, mz = (a[0] + b[0]) / 2, (a[1] + b[1]) / 2
    copy.location = adapter.to_blender(mx + side * nx / n * 450, mz + side * nz / n * 450, 400)
    before = snapshot()
    try:
        adapter.cmd_export()
        refused = False
    except SystemExit:
        refused = True
    expect(refused and snapshot() == before, "a copied seed with no border around it is refused, and nothing is written")
    line = bpy.data.curves.new("new border", "CURVE")
    spline = line.splines.new("POLY")
    spline.points.add(4)
    for n, p in enumerate(spline.points):  # roughly drawn: the ends land near, not on, the borders
        t = n / 4
        p.co = (*adapter.to_blender(a[0] + (b[0] - a[0]) * t + 30, a[1] + (b[1] - a[1]) * t - 20, 400), 1.0)
    drawn = bpy.data.objects.new("new border", line)
    next(c for c in bpy.data.collections if c.name.startswith("Borders")).objects.link(drawn)
    adapter.cmd_export()
    split = data("borders.json")
    new_region = [r["id"] for r in data("regions.json")["regions"] if r["id"] not in {x["id"] for x in data_before["regions"]}]
    newest = [bd for bd in split["borders"] if bd["id"] not in sb]
    ok, failed = geography_ok()
    expect(len(new_region) == 1 and any(sorted(bd["regions"]) == sorted(["r113", new_region[0]]) for bd in newest)
           and len(newest) == 3 and ok,
           f"drawing a border across r113 splits it into r113 and {new_region}, splitting the two borders it meets, "
           f"and the atlas checks pass {failed or ''}")
finally:
    shutil.rmtree(work)
