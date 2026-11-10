"""Blender adapter for the continent atlas (development only, Blender 5.2.2 LTS).

    blender --python content/world/authoring/atlas_blender.py -- import [--blend artifacts/world-atlas/continent.blend]
    blender --python content/world/authoring/atlas_blender.py -- region r041 [r042 ...] [--blend <file>]
    blender --background --python content/world/authoring/atlas_blender.py -- all-regions
    blender --background <file>.blend --python content/world/authoring/atlas_blender.py -- export

`region` builds one region's file (artifacts/world-atlas/regions/r041.blend), or one file for
several neighbours (regions/r041+r042.blend): their terrain pieces from the last build, which fit
together at the shared borders, every border and junction of theirs as editable curves and
markers draped over the ground, their seeds, and the surrounding regions as greyed context. To
change a border, open the two regions it separates, reshape or redraw its curve (keep one spline
per border), or move a junction to move every border ending there. `all-regions` writes one file
per region. Like every .blend here, these are disposable working files, never committed: the
JSON is the source. Export writes only what this file changed, and refuses if the source was
changed since the file was made, so an old file never undoes a newer edit.

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


def poly(name, points, coll, closed=False, props=None, height=lambda x, z: 1200):
    curve = bpy.data.curves.new(name, "CURVE")
    curve.dimensions = "3D"
    spline = curve.splines.new("POLY")
    spline.points.add(len(points) - 1)
    for p, (x, z) in zip(spline.points, points):
        p.co = (*to_blender(x, z, height(x, z)), 1.0)
    spline.use_cyclic_u = closed
    obj = bpy.data.objects.new(name, curve)
    for k, v in (props or {}).items():
        obj[k] = v
    coll.objects.link(obj)
    obj["imported"] = obj.name  # a Shift+D copy gets a new name but keeps this
    return obj


def empty(name, at, coll, kind="PLAIN_AXES", size=0.15, props=None, height=1200):
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_type = kind
    obj.empty_display_size = size
    obj.location = to_blender(at[0], at[1], height)
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
    if (WORLD / "borders.json").exists():
        add_borders(load("borders.json")["borders"], load("borders.json")["junctions"], collection("Borders", root))
    blend.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(blend))
    print(f"wrote {blend}")


def add_borders(borders, junctions, coll, ground=None):
    """Borders as poly curves and their junctions as markers, draped over `ground` when given."""
    lift = (lambda x, z: ground(x, z) + 4) if ground else (lambda x, z: 1200)
    used = {bd.get(e) for bd in borders for e in ("from", "to")}
    for j in junctions:
        if j["id"] in used:
            empty(f"junction {j['id']}", j["at"], coll, "SPHERE", 0.03, props={"atlas": "junction", "id": j["id"]},
                  height=lift(*j["at"]))
    for bd in borders:
        o = poly(f"border {bd['id']} ({' | '.join(bd['regions'])})", bd["points"], coll,
                 props={"atlas": "border", "id": bd["id"]}, height=lift)
        o["source"] = json.dumps(bd["points"])  # what this file started from, to catch edits made elsewhere since
        o.data.bevel_depth = 0.004


def raster():
    """The last build's elevation and region labels, or None before the first build."""
    if not (OUT / "regions.npy").exists():
        print("no artifacts/world-atlas/regions.npy; run atlas.py build first for the region terrain")
        return None
    import numpy as np

    meta = json.loads((OUT / "elevation.json").read_text())
    elev = np.maximum(np.load(OUT / "elevation.npy"), -20.0)
    p = np.pad(elev, 1, mode="edge")
    corners = (p[:-1, :-1] + p[1:, :-1] + p[:-1, 1:] + p[1:, 1:]) / 4  # heights at the cell corners
    return meta, elev, corners, np.load(OUT / "regions.npy")


def region_mesh(name, k, grid, coll, colour):
    """One region's land as a mesh of its 40 m cells: neighbouring regions share corner heights, so
    the pieces fit together without seams."""
    import numpy as np

    meta, _, corners, lab = grid
    cell = meta["cell"]
    verts, index, faces = [], {}, []

    def corner(i, j):
        if (i, j) not in index:
            index[(i, j)] = len(verts)
            verts.append(to_blender(meta["minX"] + j * cell, meta["minZ"] + i * cell, float(corners[i, j])))
        return index[(i, j)]

    for r, c in zip(*np.nonzero(lab == k)):
        r, c = int(r), int(c)
        faces.append((corner(r, c), corner(r + 1, c), corner(r + 1, c + 1), corner(r, c + 1)))
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*colour, 1.0)
    mesh.materials.append(mat)
    obj = bpy.data.objects.new(name, mesh)
    obj.hide_select = True
    coll.objects.link(obj)
    return obj


def cmd_import_regions(ids, blend: Path):
    """A region's own file, or one file for several neighbours: their terrain pieces, every border and
    junction of theirs (editable), their seeds, and the regions around them as greyed context."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    root = bpy.data.collections.new(COLLECTION)
    bpy.context.scene.collection.children.link(root)
    regions, borders = load("regions.json"), load("borders.json")
    known = {r["id"]: r for r in regions["regions"]}
    missing = [rid for rid in ids if rid not in known]
    if missing:
        sys.exit(f"unknown region {', '.join(missing)}")
    chosen = set(ids)
    mine = [bd for bd in borders["borders"] if chosen & set(bd["regions"])]
    around = sorted({rid for bd in mine for rid in bd["regions"]} - chosen)
    grid = raster()
    ground = None
    if grid:
        meta, elev, _, _ = grid
        if meta["regions"] != list(known):
            print("regions.json changed since the last build; run atlas.py build for current terrain pieces")
        index = {rid: k for k, rid in enumerate(meta["regions"])}

        def ground(x, z):
            r = min(max(int((z - meta["minZ"]) // meta["cell"]), 0), meta["rows"] - 1)
            c = min(max(int((x - meta["minX"]) // meta["cell"]), 0), meta["columns"] - 1)
            return float(elev[r, c])

        palette = [(0.85, 0.62, 0.3), (0.35, 0.65, 0.45), (0.45, 0.55, 0.85), (0.8, 0.45, 0.35)]
        terrain = collection("Terrain", root)
        for n, rid in enumerate(ids):
            if rid in index:
                region_mesh(f"terrain {rid}", index[rid], grid, terrain, palette[n % len(palette)])
        context = collection("Neighbours (context, not exported)", root)
        for rid in around:
            if rid in index:
                region_mesh(f"terrain {rid} (neighbour)", index[rid], grid, context, (0.45, 0.45, 0.45))
    add_borders(mine, borders["junctions"], collection("Borders", root), ground)
    seeds = collection("Region seeds", root)
    for rid in ids:
        seed = known[rid]["seed"]
        empty(f"region {rid}", seed, seeds, "SINGLE_ARROW", 0.15, props={"atlas": "region", "id": rid},
              height=(ground(*seed) + 30) if ground else 1200)
    blend.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(blend))
    print(f"wrote {blend}: {', '.join(ids)} with {len(mine)} borders; neighbours {', '.join(around)}")


def spline_points(obj):
    """World-space control points of a curve's single spline, so moving the whole object counts. A
    redrawn line may be poly, NURBS or Bézier; Bézier handles are ignored."""
    splines = obj.data.splines
    pts = (splines[0].bezier_points if splines[0].type == "BEZIER" else splines[0].points) if len(splines) == 1 else []
    if len(pts) < 2:
        sys.exit(f"{obj.name}: expected one spline with at least two points (after redrawing a line, delete the old "
                 f"spline); refusing to export")
    return [from_blender(obj.matrix_world @ p.co.xyz) for p in pts]


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

    rebase = []  # after a successful export, this file is in step with the sources again

    def moved(now, was, current, what):
        """Whether this file changed a value; refuses when the source changed since the file was made
        (or last exported), so an old file never undoes an edit exported from another one."""
        if now == was or now == current:
            return False
        if current != was:
            sys.exit(f"{what} was changed elsewhere after this file was made; regenerate the file and redo the edit")
        return True

    def regions(d):
        for r in d["regions"]:
            o = objs.get("region", {}).get(r["id"])
            if o and moved(position(o), list(o["at"]), r["seed"], f"region {r['id']}'s seed"):
                r["seed"] = position(o)
            if o:
                rebase.append((o, "at", r["seed"]))
        # new regions continue the id sequence the generator uses, never reusing a deleted id
        used = [int(r["id"][1:]) for r in d["regions"] if r["id"][:1] == "r" and r["id"][1:].isdigit()]
        n = max(used + [d.get("lastGeneratedId", 0)]) + 1
        for o in sorted(extra_regions, key=lambda o: o.name):
            d["regions"].append({"id": f"r{n:03d}", "seed": position(o)})
            d["lastGeneratedId"] = n
            n += 1

    def borders(d):
        for j in d["junctions"]:
            o = objs.get("junction", {}).get(j["id"])
            if o and moved(position(o), list(o["at"]), j["at"], f"junction {j['id']}"):
                j["at"] = position(o)
            if o:
                rebase.append((o, "at", j["at"]))
        at = {j["id"]: j["at"] for j in d["junctions"]}
        for bd in d["borders"]:
            o = objs.get("border", {}).get(bd["id"])
            if o and moved(spline_points(o), json.loads(o["source"]), bd["points"], f"border {bd['id']}"):
                bd["points"] = spline_points(o)
            # a border ends exactly at its junctions: moving a junction moves every border ending there,
            # including borders of regions not in this file
            if bd.get("from"):
                bd["points"][0] = list(at[bd["from"]])
            if bd.get("to"):
                bd["points"][-1] = list(at[bd["to"]])
            if o:
                rebase.append((o, "source", json.dumps(bd["points"])))

    update("terrain.json", terrain)
    update("water.json", water)
    update("polities.json", polities)
    update("regions.json", regions)
    if (WORLD / "borders.json").exists():
        update("borders.json", borders)
    for o, key, value in rebase:
        o[key] = value
    if bpy.data.filepath:
        bpy.ops.wm.save_mainfile()  # keep the new baseline with the file, for the next edit and export
    print("changed: " + (", ".join(changed) if changed else "nothing"))


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if not argv:
        sys.exit(__doc__)
    if argv[0] == "import":
        blend = Path(argv[argv.index("--blend") + 1]) if "--blend" in argv else OUT / "continent.blend"
        cmd_import(blend)
    elif argv[0] == "region":
        ids = [a for a in argv[1:] if not a.startswith("--")]
        if "--blend" in argv:
            ids.remove(argv[argv.index("--blend") + 1])
        blend = Path(argv[argv.index("--blend") + 1]) if "--blend" in argv else OUT / "regions" / f"{'+'.join(ids)}.blend"
        cmd_import_regions(ids, blend)
    elif argv[0] == "all-regions":
        for r in load("regions.json")["regions"]:
            cmd_import_regions([r["id"]], OUT / "regions" / f"{r['id']}.blend")
    elif argv[0] == "export":
        cmd_export()
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
