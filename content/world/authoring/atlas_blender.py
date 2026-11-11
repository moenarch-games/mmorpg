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
import atlas_regions  # noqa: E402
from atlas_json import write_json  # noqa: E402

REPO = Path(__file__).resolve().parents[3]
WORLD = Path(os.environ.get("ATLAS_WORLD", REPO / "content" / "world"))  # tests point this at a copy
OUT = Path(os.environ.get("ATLAS_OUT", REPO / "artifacts" / "world-atlas"))  # a build made with atlas.py --out
COLLECTION = "Atlas"


def load(name):
    return json.loads((WORLD / name).read_text())


ORIGIN = [0, 0]  # atlas x, z at the scene origin; a region file centres on its regions


def to_blender(x, z, y=0.0):
    # kilometres keep the scene inside Blender's comfortable range
    return ((x - ORIGIN[0]) / 1000.0, -(z - ORIGIN[1]) / 1000.0, y / 1000.0)


def from_blender(v):
    return [int(round(v[0] * 1000.0)) + ORIGIN[0], int(round(-v[1] * 1000.0)) + ORIGIN[1]]


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
    ORIGIN[:] = [0, 0]
    bpy.context.scene["atlas_origin"] = list(ORIGIN)
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
        bpy.context.scene["atlas_borders"] = json.dumps([b["id"] for b in load("borders.json")["borders"]])
    bpy.context.scene["atlas_regions"] = json.dumps([r["id"] for r in regions["regions"]])
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


def border_segments(borders):
    import numpy as np

    a = [p for bd in borders for p in bd["points"][:-1]]
    b = [p for bd in borders for p in bd["points"][1:]]
    return np.array(a, float).reshape(-1, 2), np.array(b, float).reshape(-1, 2)


def region_mesh(name, k, grid, coll, colour, segments=None):
    """One region's land as a mesh of its 40 m cells. The corners it shares with another region are
    moved onto the nearest border line, so its edge follows the smooth border rather than the grid.
    Every region computes those corners the same way from all borders, so neighbouring pieces keep
    meeting without gaps."""
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
    if segments is not None and len(segments[0]):
        p = np.pad(lab, 1, constant_values=-1)
        shared = []
        for (i, j), v in index.items():
            around = {int(p[i, j]), int(p[i, j + 1]), int(p[i + 1, j]), int(p[i + 1, j + 1])}
            if any(o >= 0 and o != k for o in around):
                shared.append((v, meta["minX"] + j * cell, meta["minZ"] + i * cell))
        if shared:
            a, b = segments
            pts = np.array([(x, z) for _, x, z in shared])
            lo, hi = pts.min(0) - 2 * cell, pts.max(0) + 2 * cell
            near = ((np.minimum(a, b) <= hi) & (np.maximum(a, b) >= lo)).all(1)
            a, b = a[near], b[near]
            if not len(a):
                a, b = segments[0][:1], segments[1][:1]  # nothing nearby: one far segment moves nothing
            ab = b - a
            t = np.clip(((pts[:, None, :] - a[None]) * ab[None]).sum(2) / np.maximum((ab * ab).sum(1), 1e-9)[None], 0, 1)
            foot = a[None] + t[..., None] * ab[None]
            dist = np.hypot(*(foot - pts[:, None, :]).transpose(2, 0, 1))
            best = dist.argmin(1)
            for n, (v, _, _) in enumerate(shared):
                if dist[n, best[n]] <= 1.5 * cell:
                    x, z = foot[n, best[n]]
                    verts[v] = to_blender(x, z, verts[v][2] * 1000.0)
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
    seeds_at = [known[rid]["seed"] for rid in ids]
    ORIGIN[:] = [int(round(sum(p[0] for p in seeds_at) / len(ids), -2)), int(round(sum(p[1] for p in seeds_at) / len(ids), -2))]
    bpy.context.scene["atlas_origin"] = list(ORIGIN)
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
        segments = border_segments(borders["borders"])
        terrain = collection("Terrain", root)
        for n, rid in enumerate(ids):
            if rid in index:
                region_mesh(f"terrain {rid}", index[rid], grid, terrain, palette[n % len(palette)], segments)
        context = collection("Neighbours (context, not exported)", root)
        for rid in around:
            if rid in index:
                region_mesh(f"terrain {rid} (neighbour)", index[rid], grid, context, (0.45, 0.45, 0.45), segments)
    add_borders(mine, borders["junctions"], collection("Borders", root), ground)
    bpy.context.scene["atlas_borders"] = json.dumps([bd["id"] for bd in mine])
    bpy.context.scene["atlas_regions"] = json.dumps(list(ids))
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
    if splines[0].type != "BEZIER":
        return [from_blender(obj.matrix_world @ p.co.xyz) for p in pts]
    # the Draw tool makes Bézier curves with few control points: follow the curve itself, every ~20 m
    from mathutils.geometry import interpolate_bezier

    out = [from_blender(obj.matrix_world @ pts[0].co)]
    for p, q in zip(pts, pts[1:]):
        steps = max(2, int((obj.matrix_world @ q.co - obj.matrix_world @ p.co).length * 1000 / 20) + 1)
        for v in interpolate_bezier(p.co, p.handle_right, q.handle_left, q.co, steps)[1:]:
            xz = from_blender(obj.matrix_world @ v)
            if xz != out[-1]:
                out.append(xz)
    return out


def position(obj):
    return from_blender(obj.matrix_world.translation)


SNAP = 80  # metres: a new border's end this close to a junction or border connects to it


def to_junctions(points, bd, at):
    """A redrawn border whose ends were drawn near, not on, its junctions: drop the drawn points
    that lie closer to a junction than the line's next stretch, so the line runs straight in
    instead of hooking back."""
    if bd.get("from") in at:
        j = at[bd["from"]]
        while len(points) > 2 and math.dist(points[1], j) < SNAP / 2:
            points.pop(0)
        points[0] = list(j)
    if bd.get("to") in at:
        j = at[bd["to"]]
        while len(points) > 2 and math.dist(points[-2], j) < SNAP / 2:
            points.pop()
        points[-1] = list(j)
    return points


def cmd_export():
    """Compute every change first, bring the border network in step, and write only when the
    result is consistent: an export either lands whole or changes nothing."""
    bpy.context.view_layer.update()  # world matrices of objects moved by a script are stale until then
    scene = bpy.context.scene
    ORIGIN[:] = list(scene.get("atlas_origin", [0, 0]))  # the frame this file was made in
    if scene.get("atlas_stale"):
        sys.exit(f"this file is out of date: {scene['atlas_stale']}")
    objs, extra_regions, drawn = {}, [], []
    for o in bpy.data.objects:
        if "atlas" not in o:
            # a curve drawn into the Borders collection is a new border
            if o.type == "CURVE" and any(c.name.startswith("Borders") for c in o.users_collection):
                drawn.append(o)
            continue
        kind, oid = o["atlas"], o.get("id", o.name)
        if o.name != o.get("imported", o.name):
            # Shift+D copies custom properties: a copied region seed is a new region, anything else is refused
            if kind == "region":
                extra_regions.append(o)
                continue
            sys.exit(f"{o.name} duplicates {o['imported']}; only region seeds can be duplicated")
        objs.setdefault(kind, {})[oid] = o
    # what this file held when it was made: whatever is gone now was deleted here
    gone_borders = [b for b in json.loads(scene.get("atlas_borders", "[]")) if b not in objs.get("border", {})]
    gone_regions = [r for r in json.loads(scene.get("atlas_regions", "[]")) if r not in objs.get("region", {})]
    names = ["terrain.json", "water.json", "polities.json", "regions.json"] + (["borders.json"] if (WORLD / "borders.json").exists() else [])
    docs = {n: json.loads((WORLD / n).read_text()) for n in names}
    before = {n: json.dumps(d, sort_keys=True) for n, d in docs.items()}
    rebase = []  # after a successful export, this file is in step with the sources again

    def moved(now, was, current, what):
        """Whether this file changed a value; refuses when the source changed since the file was made
        (or last exported), so an old file never undoes an edit exported from another one."""
        if now == was or now == current:
            return False
        if current != was:
            sys.exit(f"{what} was changed elsewhere after this file was made; regenerate the file and redo the edit")
        return True

    d = docs["terrain.json"]
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
            gate["bearing"] = int(round(math.degrees(math.atan2(x - crown["center"][0], -(z - crown["center"][1]))) % 360))

    d = docs["water.json"]
    for r in d["rivers"]:
        o = objs.get("river", {}).get(r["id"])
        if o:
            r["points"] = spline_points(o)
    for kind in ("lakes", "sinks", "springs"):
        for s in d[kind]:
            o = objs.get(kind, {}).get(s["id"])
            if o:
                s["at"] = position(o)

    d = docs["regions.json"]
    for rid in gone_regions:  # deleting a seed deletes its region (merge it by deleting the border too)
        reg = next(r for r in d["regions"] if r["id"] == rid)
        if reg.get("anchor") or reg.get("locked"):
            sys.exit(f"region {rid} holds zone-authored content and cannot be deleted")
    d["regions"] = [r for r in d["regions"] if r["id"] not in gone_regions]
    for r in d["regions"]:
        o = objs.get("region", {}).get(r["id"])
        if o and moved(position(o), list(o["at"]), r["seed"], f"region {r['id']}'s seed"):
            r["seed"] = position(o)
    # new regions continue the id sequence the generator uses, never reusing a deleted id
    used = [int(r["id"][1:]) for r in d["regions"] if r["id"][:1] == "r" and r["id"][1:].isdigit()]
    n = max(used + [d.get("lastGeneratedId", 0)]) + 1
    added = []
    for o in sorted(extra_regions, key=lambda o: o.name):
        d["regions"].append({"id": f"r{n:03d}", "seed": position(o)})
        added.append(f"r{n:03d}")
        d["lastGeneratedId"] = n
        n += 1

    d = docs["polities.json"]
    for s in d["settlements"]:
        o = objs.get("settlement", {}).get(s["id"])
        if o:
            s["at"] = position(o)
    d["claims"] = {k: v for k, v in d["claims"].items() if k not in gone_regions}

    problems = []
    if "borders.json" in docs:
        d = docs["borders.json"]
        d["borders"] = [b for b in d["borders"] if b["id"] not in gone_borders]
        for j in d["junctions"]:
            o = objs.get("junction", {}).get(j["id"])
            if o and moved(position(o), list(o["at"]), j["at"], f"junction {j['id']}"):
                j["at"] = position(o)
        at = {j["id"]: j["at"] for j in d["junctions"]}
        for bd in d["borders"]:
            o = objs.get("border", {}).get(bd["id"])
            if o and moved(spline_points(o), json.loads(o["source"]), bd["points"], f"border {bd['id']}"):
                bd["points"] = to_junctions(spline_points(o), bd, at)
            # a border ends exactly at its junctions: moving a junction moves every border ending there,
            # including borders of regions not in this file
            if bd.get("from") in at:
                bd["points"][0] = list(at[bd["from"]])
            if bd.get("to") in at:
                bd["points"][-1] = list(at[bd["to"]])
        new = [atlas_regions.attach(d, spline_points(o), SNAP)["id"] for o in sorted(drawn, key=lambda o: o.name)]
        grid = raster()
        if grid:
            meta = grid[0]
            frame = atlas_regions.Frame(meta["minX"], meta["minZ"], meta["cell"], meta["columns"], meta["rows"])
            seeds = [(r["id"], r["seed"]) for r in docs["regions.json"]["regions"]]
            problems = atlas_regions.sync(frame, grid[3] >= 0, d, seeds)  # the build labels every land cell
        else:
            print("no build artifacts: border pairs not checked; run atlas.py sync after the next build")
        topology = bool(gone_borders or gone_regions or added or new)
        for o in objs.get("border", {}).values():
            bd = next((b for b in d["borders"] if b["id"] == o["id"]), None)
            if bd:
                rebase.append((o, "source", json.dumps(bd["points"])))
        for j in d["junctions"]:
            o = objs.get("junction", {}).get(j["id"])
            if o:
                rebase.append((o, "at", j["at"]))
    else:
        topology = bool(gone_regions or added)
    if problems:
        for p in problems:
            print("problem: " + p)
        sys.exit("nothing was written: fix the problems above in this file and export again")
    for r in docs["regions.json"]["regions"]:
        o = objs.get("region", {}).get(r["id"])
        if o:
            rebase.append((o, "at", r["seed"]))

    changed = [n for n in names if json.dumps(docs[n], sort_keys=True) != before[n] and write_json(WORLD / n, docs[n])]
    for o, key, value in rebase:
        o[key] = value
    if topology:
        # regions were split or merged: the terrain pieces and border objects here no longer match
        scene["atlas_stale"] = "regions were split or merged by an export; rebuild (atlas.py build) and make the file again"
        print("regions changed: " + ", ".join(filter(None, [
            f"added {', '.join(added)}" if added else "", f"deleted {', '.join(gone_regions)}" if gone_regions else "",
            f"new borders {', '.join(new)}" if "borders.json" in docs and new else "",
            f"deleted borders {', '.join(gone_borders)}" if gone_borders else ""])))
        print("this file is now out of date: run atlas.py build, then make the region file again")
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
