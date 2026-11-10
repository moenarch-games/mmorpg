"""Blender adapter for the continent atlas (development only, Blender 5.2.2 LTS).

    blender --python content/world/authoring/atlas_blender.py -- import [--blend artifacts/world-atlas/continent.blend]
    blender --background artifacts/world-atlas/continent.blend --python content/world/authoring/atlas_blender.py -- export

`import` builds an editable scene from content/world/: the coast, ridges and rivers as poly
curves, the Crown centre, its gates, hill countries, settlements, sinks, springs and region seeds as empties,
and the last generated terrain (artifacts/world-atlas/elevation.npy) as a locked reference
mesh. Blender X is atlas x (east), Blender Y is atlas -z (north), Blender Z is up.

`export` writes moved control points back into the same JSON files. Only geometry the scene
owns changes (points, seeds, positions, gate bearings); names, heights and every other field
stay as authored. A file is rewritten only when a value actually changed, so an untouched
round trip leaves the sources byte-identical. Run atlas.py check afterwards.
"""

import json
import math
import os
import sys
from pathlib import Path

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atlas_json import write_json  # noqa: E402

REPO = Path(__file__).resolve().parents[3]
WORLD = Path(os.environ.get("ATLAS_WORLD", REPO / "content" / "world"))  # tests point this at a copy
OUT = REPO / "artifacts" / "world-atlas"
COLLECTION = "Atlas"


def load(name):
    return json.loads((WORLD / name).read_text())


def to_blender(x, z, y=0.0):
    return (x / 1000.0, -z / 1000.0, y / 1000.0)  # kilometres keep the scene inside Blender's comfortable range


def from_blender(v):
    return [int(round(v[0] * 1000.0)), int(round(-v[1] * 1000.0))]


def collection(name, parent):
    c = bpy.data.collections.new(name)
    parent.children.link(c)
    return c


def poly(name, points, coll, closed=False, props=None):
    curve = bpy.data.curves.new(name, "CURVE")
    curve.dimensions = "3D"
    spline = curve.splines.new("POLY")
    spline.points.add(len(points) - 1)
    for p, (x, z) in zip(spline.points, points):
        p.co = (*to_blender(x, z, 1200), 1.0)
    spline.use_cyclic_u = closed
    obj = bpy.data.objects.new(name, curve)
    for k, v in (props or {}).items():
        obj[k] = v
    coll.objects.link(obj)
    obj["imported"] = obj.name  # a Shift+D copy gets a new name but keeps this
    return obj


def empty(name, at, coll, kind="PLAIN_AXES", size=0.15, props=None):
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_type = kind
    obj.empty_display_size = size
    obj.location = to_blender(at[0], at[1], 1200)
    for k, v in (props or {}).items():
        obj[k] = v
    coll.objects.link(obj)
    obj["imported"] = obj.name  # a Shift+D copy gets a new name but keeps this
    obj["at"] = [int(round(at[0])), int(round(at[1]))]
    return obj


def terrain_mesh(coll):
    meta_path, grid_path = OUT / "elevation.json", OUT / "elevation.npy"
    if not grid_path.exists():
        print("no artifacts/world-atlas/elevation.npy; run atlas.py build first for the reference terrain")
        return
    import numpy as np

    meta = json.loads(meta_path.read_text())
    elev = np.load(grid_path)[::4, ::4]
    rows, cols = elev.shape
    step = meta["cell"] * 4
    verts = []
    for r in range(rows):
        for c in range(cols):
            x = meta["minX"] + (c + 0.5) * step
            z = meta["minZ"] + (r + 0.5) * step
            verts.append(to_blender(x, z, max(float(elev[r, c]), -20.0)))
    faces = [(r * cols + c, r * cols + c + 1, (r + 1) * cols + c + 1, (r + 1) * cols + c)
             for r in range(rows - 1) for c in range(cols - 1)]
    mesh = bpy.data.meshes.new("Reference terrain")
    mesh.from_pydata(verts, [], faces)
    obj = bpy.data.objects.new("Reference terrain (generated, not exported)", mesh)
    obj.hide_select = True
    coll.objects.link(obj)


def cmd_import(blend: Path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    root = bpy.data.collections.new(COLLECTION)
    bpy.context.scene.collection.children.link(root)
    terrain, water, pol, regions, anchors = (load(n) for n in
                                             ("terrain.json", "water.json", "polities.json", "regions.json", "anchors.json"))
    ref = collection("Reference", root)
    terrain_mesh(ref)
    for a in anchors["anchors"]:
        for part in a["parts"]:
            x0, z0, x1, z1 = part["rect"]
            o = poly(f"anchor {a['id']}: {part['name']}", [[x0, z0], [x1, z0], [x1, z1], [x0, z1]], ref, closed=True)
            o.hide_select = True
    land = collection("Terrain", root)
    poly("coast", terrain["coast"]["points"], land, closed=True, props={"atlas": "coast"})
    for kind in ("seas", "islands"):
        for s in terrain["coast"][kind]:
            o = empty(f"{kind[:-1]} {s['id']}", s["center"], land, "SPHERE", props={"atlas": kind, "id": s["id"]})
            o.empty_display_size = s["radius"] / 1000.0
    for r in terrain["ridges"]:
        poly(f"ridge {r['id']}", r["points"], land, props={"atlas": "ridge", "id": r["id"]})
    for u in terrain.get("uplands", []):
        o = empty(f"uplands {u['id']}", u["center"], land, "SPHERE", props={"atlas": "uplands", "id": u["id"]})
        o.empty_display_size = max(u["radius"]) / 1000.0
    crown = terrain["crown"]
    empty("crown centre", crown["center"], land, "CIRCLE", size=crown["radius"] / 1000.0, props={"atlas": "crown"})
    for gate in crown["gates"]:
        b = math.radians(gate["bearing"])
        at = [crown["center"][0] + crown["radius"] * math.sin(b), crown["center"][1] - crown["radius"] * math.cos(b)]
        empty(f"gate {gate['id']}", at, land, "CONE", 0.3, props={"atlas": "gate", "id": gate["id"]})
    wet = collection("Water", root)
    for r in water["rivers"]:
        poly(f"river {r['id']}", r["points"], wet, props={"atlas": "river", "id": r["id"]})
    for kind in ("lakes", "sinks", "springs"):
        for s in water[kind]:
            empty(f"{kind[:-1]} {s['id']}", s["at"], wet, "SPHERE", 0.12, props={"atlas": kind, "id": s["id"]})
    people = collection("Settlements", root)
    for s in pol["settlements"]:
        empty(f"settlement {s['id']}", s["at"], people, "CUBE", 0.12, props={"atlas": "settlement", "id": s["id"]})
    seeds = collection("Region seeds", root)
    for r in regions["regions"]:
        empty(f"region {r['id']}", r["seed"], seeds, "SINGLE_ARROW", 0.25, props={"atlas": "region", "id": r["id"]})
    blend.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(blend))
    print(f"wrote {blend}")


def spline_points(obj):
    """World-space points of a curve's single poly spline, so moving the whole object counts."""
    splines = obj.data.splines
    if len(splines) != 1 or splines[0].type != "POLY" or len(splines[0].points) < 2:
        sys.exit(f"{obj.name}: expected one poly spline with at least two points; refusing to export")
    return [from_blender(obj.matrix_world @ p.co.xyz) for p in splines[0].points]


def position(obj):
    return from_blender(obj.matrix_world.translation)


def cmd_export():
    bpy.context.view_layer.update()  # world matrices of objects moved by a script are stale until then
    objs, extra_regions = {}, []
    for o in bpy.data.objects:
        if "atlas" not in o:
            continue
        kind, oid = o["atlas"], o.get("id", o.name)
        if o.name != o.get("imported", o.name):
            # Shift+D copies custom properties: a copied region seed is a new region, anything else is refused
            if kind == "region":
                extra_regions.append(o)
                continue
            sys.exit(f"{o.name} duplicates {o['imported']}; only region seeds can be duplicated")
        objs.setdefault(kind, {})[oid] = o
    changed = []

    def update(name, mutate):
        path = WORLD / name
        data = json.loads(path.read_text())
        before = json.dumps(data, sort_keys=True)
        mutate(data)
        if json.dumps(data, sort_keys=True) != before:
            write_json(path, data)
            changed.append(name)

    def terrain(d):
        if "coast" in objs.get("coast", {}):
            d["coast"]["points"] = spline_points(objs["coast"]["coast"])
        for kind in ("seas", "islands"):
            for s in d["coast"][kind]:
                o = objs.get(kind, {}).get(s["id"])
                if o:
                    s["center"] = position(o)
        for r in d["ridges"]:
            o = objs.get("ridge", {}).get(r["id"])
            if o:
                r["points"] = spline_points(o)
        for u in d.get("uplands", []):
            o = objs.get("uplands", {}).get(u["id"])
            if o:
                u["center"] = position(o)
        crown = d["crown"]
        o = next(iter(objs.get("crown", {}).values()), None)
        if o:
            crown["center"] = position(o)
        for gate in crown["gates"]:
            o = objs.get("gate", {}).get(gate["id"])
            # a gate keeps its bearing unless the gate itself was moved, so moving the Crown carries its gates along
            if o and position(o) != list(o["at"]):
                x, z = position(o)
                bearing = math.degrees(math.atan2(x - crown["center"][0], -(z - crown["center"][1]))) % 360
                gate["bearing"] = int(round(bearing))

    def water(d):
        for r in d["rivers"]:
            o = objs.get("river", {}).get(r["id"])
            if o:
                r["points"] = spline_points(o)
        for kind in ("lakes", "sinks", "springs"):
            for s in d[kind]:
                o = objs.get(kind, {}).get(s["id"])
                if o:
                    s["at"] = position(o)

    def polities(d):
        for s in d["settlements"]:
            o = objs.get("settlement", {}).get(s["id"])
            if o:
                s["at"] = position(o)

    def regions(d):
        for r in d["regions"]:
            o = objs.get("region", {}).get(r["id"])
            if o:
                r["seed"] = position(o)
        # new regions continue the id sequence the generator uses, never reusing a deleted id
        used = [int(r["id"][1:]) for r in d["regions"] if r["id"][:1] == "r" and r["id"][1:].isdigit()]
        n = max(used + [d.get("lastGeneratedId", 0)]) + 1
        for o in sorted(extra_regions, key=lambda o: o.name):
            d["regions"].append({"id": f"r{n:03d}", "seed": position(o)})
            d["lastGeneratedId"] = n
            n += 1

    update("terrain.json", terrain)
    update("water.json", water)
    update("polities.json", polities)
    update("regions.json", regions)
    print("changed: " + (", ".join(changed) if changed else "nothing"))


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if not argv:
        sys.exit(__doc__)
    if argv[0] == "import":
        blend = Path(argv[argv.index("--blend") + 1]) if "--blend" in argv else OUT / "continent.blend"
        cmd_import(blend)
    elif argv[0] == "export":
        cmd_export()
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
