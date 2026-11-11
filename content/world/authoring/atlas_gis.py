"""GIS exchange for the continent atlas: GeoJSON layers and a TopoJSON of the regions, written in two
frames, and an import that brings point and line edits made in a GIS tool back into content/world/.

The world is placed at 0° N 0° E. In EPSG:3857 (Web Mercator) the scale there is 1 to within
two parts per million, so `epsg3857/` holds plain atlas metres (x east, y = −z north) that QGIS
and GDAL read with the legacy `crs` member. `wgs84/` holds the same in longitude and latitude, as
RFC 7946 requires, for web maps and strict readers. The JSON in content/world/ stays the source;
the regions here are derived and read-only.
"""

import json
import math
from pathlib import Path

import numpy as np

import atlas_regions
from atlas_regions import chaikin, simplify

R = 6378137.0  # the Web Mercator sphere
CRS_3857 = {"type": "name", "properties": {"name": "urn:ogc:def:crs:EPSG::3857"}}


def to_3857(x, z):
    return [round(x, 2), round(-z, 2)]


def from_3857(e, n):
    return e, -n


def to_wgs84(x, z):
    return [round(math.degrees(x / R), 9), round(math.degrees(2 * math.atan(math.exp(-z / R)) - math.pi / 2), 9)]


def from_wgs84(lon, lat):
    return math.radians(lon) * R, -R * math.log(math.tan(math.pi / 4 + math.radians(lat) / 2))


FRAMES = {"epsg3857": (to_3857, from_3857), "wgs84": (to_wgs84, from_wgs84)}


# ---------------------------------------------------------------- region topology


def trace_arcs(label: np.ndarray):
    """Chains of cell edges between two different labels (sea is -1), split where three labels
    meet. Returns [(pair, vertex path)]; a path that returns to its start without a node is a ring."""
    nbr = {}

    def add(v, w, p, q):
        pair = (min(p, q), max(p, q))
        nbr.setdefault(v, []).append((w, pair))
        nbr.setdefault(w, []).append((v, pair))

    a, b = label[:, :-1], label[:, 1:]
    for r, c in zip(*np.nonzero(a != b)):
        add((int(r), int(c) + 1), (int(r) + 1, int(c) + 1), int(a[r, c]), int(b[r, c]))
    a, b = label[:-1, :], label[1:, :]
    for r, c in zip(*np.nonzero(a != b)):
        add((int(r) + 1, int(c)), (int(r) + 1, int(c) + 1), int(a[r, c]), int(b[r, c]))

    def is_node(v):
        e = nbr[v]
        return len(e) != 2 or e[0][1] != e[1][1]

    seen, chains = set(), []

    def walk(v, w, pair):
        path = [v]
        while True:
            seen.add((min(v, w), max(v, w)))
            path.append(w)
            if is_node(w) or w == path[0]:
                return path
            v, w = w, next(x for x, p in nbr[w] if (min(w, x), max(w, x)) not in seen)

    for v in sorted(nbr):
        if is_node(v):
            for w, pair in sorted(nbr[v]):
                if (min(v, w), max(v, w)) not in seen:
                    chains.append((pair, walk(v, w, pair)))
    for v in sorted(nbr):
        for w, pair in sorted(nbr[v]):
            if (min(v, w), max(v, w)) not in seen:
                chains.append((pair, walk(v, w, pair)))
    return chains, {v for v in nbr if is_node(v)}


def arc_along(points, start, end):
    """The part of an authored border between the points nearest `start` and `end`, in that order,
    with `start` and `end` as its ends."""
    cum = [0.0]
    for p, q in zip(points, points[1:]):
        cum.append(cum[-1] + math.dist(p, q))

    def at(xz):
        d, k, foot = atlas_regions.nearest_on(points, *xz)
        return cum[k] + math.dist(points[k], foot)

    s0, s1 = at(start), at(end)
    inner = [list(p) for p, s in zip(points, cum) if min(s0, s1) < s < max(s0, s1)]
    if s0 > s1:
        inner.reverse()
    return [list(start)] + inner + [list(end)]


def topology(g, label, ids, borders):
    """Shared arcs for every region boundary: authored border geometry between two regions, and a
    smoothed coastline where a region meets the sea. Every arc is stored once; regions are rings of
    arc references, so neighbours share their border exactly as in borders.json."""
    lab = np.pad(label, 1, constant_values=-1)  # sea around the domain closes every ring
    chains, nodes = trace_arcs(lab)
    junctions = [(j["at"], j["id"]) for j in borders["junctions"]]

    def xz(v):
        return (g.min_x + (v[1] - 1) * g.cell, g.min_z + (v[0] - 1) * g.cell)

    # a node where only regions meet is an authored junction, slightly displaced by the raster
    placed, node_id = {}, {}
    for v in nodes:
        around = {int(lab[v[0] - 1, v[1] - 1]), int(lab[v[0] - 1, v[1]]), int(lab[v[0], v[1] - 1]), int(lab[v[0], v[1]])}
        p, nid = xz(v), v
        if -1 not in around and junctions:
            at, jid = min(junctions, key=lambda j: math.dist(j[0], p))
            if math.dist(at, p) <= 3 * g.cell:
                # where four regions meet, the raster shows two three-way points: both are this junction
                p, nid = tuple(at), jid
        placed[v], node_id[v] = p, nid
    by_pair = {}
    for b in borders["borders"]:
        if len(b["regions"]) == 2 and all(r in ids for r in b["regions"]):
            key = tuple(sorted(ids.index(r) for r in b["regions"]))
            by_pair.setdefault(key, []).append(b["points"])
    arcs = []
    for pair, path in chains:
        closed = path[0] == path[-1] and path[0] not in nodes
        pts = [placed.get(v, xz(v)) for v in path]
        if -1 not in pair and pair in by_pair and not closed:
            mid = pts[len(pts) // 2]
            line = min(by_pair[pair], key=lambda l: atlas_regions.nearest_on(l, *mid)[0])
            pts = arc_along(line, pts[0], pts[-1])
        else:
            if closed:
                ring = chaikin(simplify(pts, 0.7 * g.cell)[:-1], 3, closed=True)
                pts = simplify(ring + [ring[0]], 4.0)
            else:
                pts = simplify(chaikin(simplify(pts, 0.7 * g.cell), 3), 4.0)
        start, end = node_id.get(path[0], path[0]), node_id.get(path[-1], path[-1])
        if not closed and start == end and (pair not in by_pair or atlas_regions.length(pts) < 3 * g.cell):
            continue  # the raster's step between two three-way points of one authored junction
        arcs.append({"pair": pair, "points": [list(p) for p in pts], "start": start, "end": end, "closed": closed})
    return arcs


def signed_area(ring_xy):
    return sum(x0 * y1 - x1 * y0 for (x0, y0), (x1, y1) in zip(ring_xy, ring_xy[1:])) / 2


def region_rings(arcs, k, to):
    """The rings of region k as TopoJSON arc references (~i is arc i reversed), exterior first and
    counter-clockwise, holes clockwise (RFC 7946), in the output frame `to`."""
    mine = [i for i, a in enumerate(arcs) if k in a["pair"]]
    used, rings = set(), []
    for i0 in mine:
        if i0 in used:
            continue
        used.add(i0)
        if arcs[i0]["closed"]:
            rings.append([i0])
            continue
        ring, end = [i0], arcs[i0]["end"]
        while end != arcs[i0]["start"]:
            nxt = next((i for i in mine if i not in used and end in (arcs[i]["start"], arcs[i]["end"])), None)
            if nxt is None:
                break
            used.add(nxt)
            if arcs[nxt]["start"] == end:
                ring.append(nxt)
                end = arcs[nxt]["end"]
            else:
                ring.append(~nxt)
                end = arcs[nxt]["start"]
        rings.append(ring)

    def coords(ring):
        out = []
        for ref in ring:
            pts = arcs[ref]["points"] if ref >= 0 else arcs[~ref]["points"][::-1]
            out += [to(*p) for p in (pts if not out else pts[1:])]
        return out

    def flip(ring):
        return [~r for r in reversed(ring)]

    rings.sort(key=lambda r: -abs(signed_area(coords(r))))
    out = []
    for n, ring in enumerate(rings):
        ccw = signed_area(coords(ring)) > 0
        out.append(ring if ccw == (n == 0) else flip(ring))
    return out, coords


# ---------------------------------------------------------------- export


def feature(geometry, props, fid=None):
    f = {"type": "Feature", "properties": props, "geometry": geometry}
    if fid is not None:
        f["id"] = fid
    return f


def write_collection(path: Path, features, crs=None):
    head = {"type": "FeatureCollection"}
    if crs:
        head["crs"] = crs
    body = ",\n".join(json.dumps(f, ensure_ascii=False, separators=(",", ":")) for f in features)
    path.write_text(json.dumps(head, separators=(",", ":"))[:-1] + ',"features":[\n' + body + "\n]}\n")


def export(b, out: Path, digest: str, road_simplify=20.0):
    """Write GeoJSON layers and regions.topojson into out/epsg3857/ and out/wgs84/."""
    g, layers = b["grid"], b["layers"]
    ids = b["ids"]
    borders = layers["borders"]
    arcs = topology(g, b["label"], ids, borders)
    pol, terrain, water = layers["polities"], layers["terrain"], layers["water"]
    crown = terrain["crown"]
    report = {}
    for frame, (to, _) in FRAMES.items():
        d = out / frame
        d.mkdir(parents=True, exist_ok=True)
        crs = CRS_3857 if frame == "epsg3857" else None

        def point(p):
            return {"type": "Point", "coordinates": to(*p)}

        def line(pts):
            return {"type": "LineString", "coordinates": [to(*p) for p in pts]}

        stats = b["region_stats"]
        geometries, polygons = [], []
        for k, rid in enumerate(ids):
            if not stats[k]:
                continue
            rings, coords = region_rings(arcs, k, to)
            reg = layers["regions"]["regions"][k]
            props = {"id": rid, "name": reg.get("name"), "kingdom": b["kingdom"][k], "contested": b["contested"][k],
                     "areaKm2": stats[k]["areaKm2"], "meanElevation": stats[k]["meanElevation"],
                     "biomes": [layers["climate"]["biomes"][[x["id"] for x in layers["climate"]["biomes"]].index(t)]["key"]
                                for t in stats[k]["biomes"]]}
            geometries.append({"type": "Polygon", "id": rid, "arcs": rings, "properties": props})
            polygons.append(feature({"type": "Polygon", "coordinates": [coords(r) for r in rings]}, props, rid))
        topo = {"type": "Topology", "arcs": [[to(*p) for p in a["points"]] for a in arcs],
                "objects": {"regions": {"type": "GeometryCollection", "geometries": geometries}}}
        if crs:
            topo["crs"] = crs
        (d / "regions.topojson").write_text(json.dumps(topo, ensure_ascii=False, separators=(",", ":")) + "\n")
        write_collection(d / "regions.geojson", polygons, crs)
        coast = [a["points"] for a in arcs if -1 in a["pair"]]
        write_collection(d / "coastline.geojson", [feature({"type": "MultiLineString",
                                                            "coordinates": [[to(*p) for p in c] for c in coast]}, {"layer": "coastline"})], crs)
        kingdom_of = dict(zip(ids, b["kingdom"]))
        layers_out = {
            "borders": [feature(line(bd["points"]), {"id": bd["id"], "regions": " | ".join(bd["regions"]), "region_a": bd["regions"][0],
                                                     "region_b": bd["regions"][1], "from": bd.get("from"), "to": bd.get("to")}, bd["id"])
                        for bd in borders["borders"]],
            "junctions": [feature(point(j["at"]), {"id": j["id"]}, j["id"]) for j in borders["junctions"]],
            "seeds": [feature(point(r["seed"]), {"id": r["id"], "name": r.get("name"), "kingdom": kingdom_of.get(r["id"])}, r["id"])
                      for r in layers["regions"]["regions"]],
            "settlements": [feature(point(s["at"]), {"id": s["id"], "name": s["name"], "kind": s["kind"], "kingdom": s.get("kingdom")}, s["id"])
                            for s in pol["settlements"]],
            "rivers": [feature(line(r["points"]), {"id": r["id"], "name": r["name"], "mouth": r["mouth"]}, r["id"]) for r in water["rivers"]],
            # the course as carved, meandering and ending at the shore (derived, read-only)
            "river-courses": [feature(line(simplify([s for s in b["profiles"][r["id"]]["samples"] if b["land"][g.index(*s)]], 5.0)),
                                      {"id": r["id"], "name": r["name"]}, r["id"]) for r in water["rivers"]],
            "ridges": [feature(line(r["points"]), {"id": r["id"], "name": r["name"], "height": r["height"]}, r["id"]) for r in terrain["ridges"]],
            "uplands": [feature(point(u["center"]), {"id": u["id"], "name": u["name"], "height": u["height"],
                                                    "radiusX": u["radius"][0], "radiusZ": u["radius"][1]}, u["id"])
                        for u in terrain.get("uplands", [])],
            "water": [feature(point(s["at"]), {"id": s["id"], "name": s["name"], "kind": kind[:-1]}, s["id"])
                      for kind in ("lakes", "sinks", "springs") for s in water[kind]],
            "gates": [feature(point(gate_xy(crown, gt)), {"id": gt["id"], "name": gt["name"], "floor": gt["floor"]}, gt["id"])
                      for gt in crown["gates"]],
            "roads": [feature({"type": "MultiLineString", "coordinates": [[to(*p) for p in simplify(
                [g.centre(r, c) for r, c in leg], road_simplify)] for leg in road["legs"] if leg]},
                              {"id": road["id"], "name": road["name"]}, road["id"]) for road in b["roads"]],
        }
        for name, feats in layers_out.items():
            write_collection(d / f"{name}.geojson", feats, crs)
        (d / "manifest.json").write_text(json.dumps({
            "frame": frame, "source": digest,
            "note": "Made by atlas.py gis-export. gis-import accepts edited borders, junctions, seeds, settlements, rivers, "
                    "ridges, uplands and water points from this folder while content/world/ is unchanged since; "
                    "rivers.geojson holds the editable control lines and river-courses.geojson the carved courses; "
                    "regions, coastline, river courses, gates and roads are derived and read-only."}, indent=2) + "\n")
        report[frame] = len(layers_out) + 3
    return arcs


def gate_xy(crown, gate):
    b = math.radians(gate["bearing"])
    return crown["center"][0] + crown["radius"] * math.sin(b), crown["center"][1] - crown["radius"] * math.cos(b)


# ---------------------------------------------------------------- import


def read_layer(d: Path, name):
    path = d / f"{name}.geojson"
    return json.loads(path.read_text())["features"] if path.exists() else []


def import_edits(d: Path, layers: dict):
    """Apply edited geometry from a gis-export folder to the layer dicts. Features are matched by
    their `id` property; a border or seed without one is new. Returns a summary and any notes."""
    frm = FRAMES[json.loads((d / "manifest.json").read_text())["frame"]][1]

    def xz(c):
        x, z = frm(*c[:2])
        return [int(round(x)), int(round(z))]

    notes, summary = [], {"moved": 0, "newBorders": [], "newRegions": []}

    def apply(items, feats, key, kind, shape):
        by_id = {i["id"]: i for i in items}
        seen = set()
        for f in feats:
            fid = (f.get("properties") or {}).get("id") or f.get("id")
            if fid not in by_id:
                continue
            seen.add(fid)
            geom = f["geometry"]
            if geom["type"] != shape:
                notes.append(f"{kind} {fid}: expected a {shape}, found a {geom['type']}; left unchanged")
                continue
            value = xz(geom["coordinates"]) if shape == "Point" else [xz(c) for c in geom["coordinates"]]
            if by_id[fid][key] != value:
                by_id[fid][key] = value
                summary["moved"] += 1
        missing = [i["id"] for i in items if i["id"] not in seen]
        if feats and missing:
            notes.append(f"{len(missing)} {kind} missing from the GIS layer were left unchanged (delete them in Blender or the JSON)")

    doc = layers.get("borders")
    if doc:
        apply(doc["junctions"], read_layer(d, "junctions"), "at", "junctions", "Point")
        apply(doc["borders"], read_layer(d, "borders"), "points", "borders", "LineString")
        at = {j["id"]: j["at"] for j in doc["junctions"]}
        for bd in doc["borders"]:
            if bd.get("from") in at:
                bd["points"][0] = list(at[bd["from"]])
            if bd.get("to") in at:
                bd["points"][-1] = list(at[bd["to"]])
        for f in read_layer(d, "borders"):
            if not (f.get("properties") or {}).get("id") and f["geometry"]["type"] == "LineString":
                summary["newBorders"].append(atlas_regions.attach(doc, [xz(c) for c in f["geometry"]["coordinates"]], 80)["id"])
    regions = layers["regions"]
    apply(regions["regions"], read_layer(d, "seeds"), "seed", "seeds", "Point")
    for f in read_layer(d, "seeds"):
        if not (f.get("properties") or {}).get("id") and f["geometry"]["type"] == "Point":
            used = [int(r["id"][1:]) for r in regions["regions"] if r["id"][:1] == "r" and r["id"][1:].isdigit()]
            n = max(used + [regions.get("lastGeneratedId", 0)]) + 1
            regions["regions"].append({"id": f"r{n:03d}", "seed": xz(f["geometry"]["coordinates"])})
            regions["lastGeneratedId"] = n
            summary["newRegions"].append(f"r{n:03d}")
    apply(layers["polities"]["settlements"], read_layer(d, "settlements"), "at", "settlements", "Point")
    apply(layers["water"]["rivers"], read_layer(d, "rivers"), "points", "rivers", "LineString")
    apply(layers["terrain"]["ridges"], read_layer(d, "ridges"), "points", "ridges", "LineString")
    apply(layers["terrain"].get("uplands", []), read_layer(d, "uplands"), "center", "uplands", "Point")
    water_points = [s for kind in ("lakes", "sinks", "springs") for s in layers["water"][kind]]
    apply(water_points, read_layer(d, "water"), "at", "water points", "Point")
    return summary, notes
