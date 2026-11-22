# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy>=2", "scipy>=1.11", "pillow>=10.1"]
# ///
"""Greyhaven's named areas traced from its authored geography (#1063).

    uv run content/greyhaven/authoring/areas.py            # write content/greyhaven/areas.json and a preview
    uv run content/greyhaven/authoring/areas.py --check    # fail when areas.json is stale

The same way the continent atlas draws its regions (content/world/authoring/atlas.py): every
area grows from its seeds over a raster of the zone, and the borders between the pieces are traced,
simplified and smoothed once, so neighbours share them exactly.

The raster is the zone's playable outline (region.json) at 4 m. Travel costs distance plus
climbing (terrain.json), and crossing a river, a lake shore, a road, a forest edge or a rockfield
edge (design.json, the Vale's brooks and paths in landscape.json) costs extra, so those become
borders. Places with a `reach` (the Outpost, the settlements, the Sunken Shrine) claim their
ground first; the open country then grows from its seeds and the design features it owns. The
pieces tile the outline with no gaps and no overlaps; where the outline is the border, the
outline itself is used. Seeds come from content/greyhaven/authoring/area-seeds.json; the preview
goes to artifacts/greyhaven-areas/.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage
from scipy.sparse import coo_matrix
from scipy.sparse.csgraph import dijkstra

REPO = Path(__file__).resolve().parents[3]
GREYHAVEN = REPO / "content" / "greyhaven"
OUT = REPO / "artifacts" / "greyhaven-areas"
sys.path.insert(0, str(REPO / "content" / "world" / "authoring"))
from atlas_gis import trace_arcs  # noqa: E402
from atlas_regions import chaikin, components, simplify  # noqa: E402

CELL = 4.0  # metres
UNITS = 100  # units per metre
# What crossing costs, in metres of travel.
CLIMB = 3.0  # per metre of height difference
WATER = 160.0
ROAD = 18.0
FOREST = 45.0
ROCKS = 20.0
BAND = 5.0  # the half width (m) of a river or road band: wide enough that no diagonal step skips it
SIMPLIFY = 2.5  # Douglas-Peucker tolerance, metres
SMOOTH = 1  # Chaikin rounds
CORE = 0.8  # the middle of its footprint an open area always holds, per axis


def load(name: str):
    return json.loads((GREYHAVEN / name).read_text())


class Grid:
    def __init__(self, outline):
        xs = [p[0] for p in outline]
        zs = [p[1] for p in outline]
        self.min_x = math.floor(min(xs) / CELL) * CELL - CELL
        self.min_z = math.floor(min(zs) / CELL) * CELL - CELL
        self.nx = int(math.ceil((max(xs) - self.min_x) / CELL)) + 2
        self.nz = int(math.ceil((max(zs) - self.min_z) / CELL)) + 2
        self.cell = CELL
        self.X = self.min_x + (np.arange(self.nx) + 0.5) * CELL
        self.Z = self.min_z + (np.arange(self.nz) + 0.5) * CELL

    def index(self, x, z):
        r = int((z - self.min_z) // CELL)
        c = int((x - self.min_x) // CELL)
        return min(max(r, 0), self.nz - 1), min(max(c, 0), self.nx - 1)

    def centre(self, r, c):
        return self.min_x + (c + 0.5) * CELL, self.min_z + (r + 0.5) * CELL


def inside(g: Grid, polygon) -> np.ndarray:
    """Cells whose centre lies inside `polygon` (metres), even-odd."""
    X, Z = np.meshgrid(g.X, g.Z)
    out = np.zeros(X.shape, bool)
    pts = list(polygon)
    for (ax, az), (bx, bz) in zip(pts, pts[1:] + pts[:1]):
        if az == bz:
            continue
        hit = ((az > Z) != (bz > Z)) & (X < ax + (Z - az) * (bx - ax) / (bz - az))
        out ^= hit
    return out


def ellipse(g: Grid, centre, radii) -> np.ndarray:
    X, Z = np.meshgrid(g.X, g.Z)
    return ((X - centre[0]) / radii[0]) ** 2 + ((Z - centre[1]) / radii[1]) ** 2 <= 1.0


def band(g: Grid, points, radius) -> np.ndarray:
    """Cells within `radius` metres of a polyline."""
    out = np.zeros((g.nz, g.nx), bool)
    for (ax, az), (bx, bz) in zip(points, points[1:]):
        r0, c0 = g.index(min(ax, bx) - radius, min(az, bz) - radius)
        r1, c1 = g.index(max(ax, bx) + radius, max(az, bz) + radius)
        X, Z = np.meshgrid(g.X[c0 : c1 + 1], g.Z[r0 : r1 + 1])
        vx, vz = bx - ax, bz - az
        t = np.clip(((X - ax) * vx + (Z - az) * vz) / ((vx * vx + vz * vz) or 1.0), 0.0, 1.0)
        near = np.hypot(X - ax - t * vx, Z - az - t * vz) <= radius
        out[r0 : r1 + 1, c0 : c1 + 1] |= near
    return out


def heights(g: Grid) -> np.ndarray:
    """terrain.json's macro heights (metres), bilinear on the raster."""
    t = load("terrain.json")
    h = np.array(t["heights"], float).reshape(t["rows"], t["columns"]) / UNITS
    step = t["step"] / UNITS
    ox, oz = t["origin"][0] / UNITS, t["origin"][1] / UNITS
    X, Z = np.meshgrid(g.X, g.Z)
    return ndimage.map_coordinates(h, [(Z - oz) / step, (X - ox) / step], order=1, mode="nearest")


def layers(g: Grid, design, landscape):
    water = np.zeros((g.nz, g.nx), bool)
    for river in design["rivers"]:
        water |= band(g, river["points"], max(river["width"] / 2 + 2, BAND))
    for course in landscape["watercourses"].values():
        water |= band(g, [[x / UNITS, z / UNITS] for x, z in course], BAND)
    for lake in design["lakes"]:
        water |= ellipse(g, lake["centre"], lake["radii"])
    road = np.zeros((g.nz, g.nx), bool)
    for r in design["roads"]:
        road |= band(g, r["points"], BAND)
    for p in landscape["paths"]:
        road |= band(g, [[x / UNITS, z / UNITS] for x, z in p["points"]], BAND)
    forest = np.zeros((g.nz, g.nx), np.int32)
    for k, f in enumerate(design["forests"]):
        forest[inside(g, f["polygon"])] = k + 1
    rocks = np.zeros((g.nz, g.nx), np.int32)
    for k, f in enumerate(design["rockfields"]):
        rocks[inside(g, f["polygon"])] = k + 1
    return water, road, forest, rocks


def features(g: Grid, design) -> dict:
    """Each named design feature an area may own, as cells."""
    out = {}
    for f in design["forests"] + design["rockfields"]:
        out[f["name"]] = inside(g, f["polygon"])
    for lake in design["lakes"]:
        out[lake["name"]] = ellipse(g, lake["centre"], lake["radii"])
    return out


def graph(g: Grid, zone, h, water, road, forest, rocks):
    """The 8-neighbour travel graph over `zone` cells, as a symmetric sparse matrix."""
    n = g.nz * g.nx
    ids = np.arange(n).reshape(g.nz, g.nx)
    rows, cols, weights = [], [], []
    for dr, dc in ((0, 1), (1, 0), (1, 1), (1, -1)):
        a = (slice(0, g.nz - dr), slice(max(0, -dc), g.nx - max(0, dc)))
        b = (slice(dr, g.nz), slice(max(0, dc), g.nx + min(0, dc)))
        ok = zone[a] & zone[b]
        w = CELL * math.hypot(dr, dc) + CLIMB * np.abs(h[a] - h[b])
        w = w + WATER * (water[a] != water[b]) + ROAD * (road[a] != road[b])
        w = w + FOREST * (forest[a] != forest[b]) + ROCKS * (rocks[a] != rocks[b])
        rows.append(ids[a][ok])
        cols.append(ids[b][ok])
        weights.append(w[ok])
    r = np.concatenate(rows)
    c = np.concatenate(cols)
    w = np.concatenate(weights)
    return coo_matrix((np.concatenate([w, w]), (np.concatenate([r, c]), np.concatenate([c, r]))), shape=(n, n)).tocsr()


def snap(g: Grid, ok: np.ndarray, x, z):
    """The nearest cell where `ok` holds."""
    r, c = g.index(x, z)
    if ok[r, c]:
        return r, c
    rr, cc = np.nonzero(ok)
    k = int(np.argmin((rr - r) ** 2 + (cc - c) ** 2))
    return int(rr[k]), int(cc[k])


def centre(rect):
    (x0, z0), (x1, z1) = rect
    return [(x0 + x1) / 2, (z0 + z1) / 2]


def shrink(rect, fraction):
    """The middle `fraction` of a rectangle along each axis."""
    (x0, z0), (x1, z1) = rect
    cx, cz = centre(rect)
    hx, hz = (x1 - x0) / 2 * fraction, (z1 - z0) / 2 * fraction
    return [[cx - hx, cz - hz], [cx + hx, cz + hz]]


def rect_cells(g: Grid, rect) -> np.ndarray:
    X, Z = np.meshgrid(g.X, g.Z)
    (x0, z0), (x1, z1) = rect
    return (X >= x0) & (X < x1) & (Z >= z0) & (Z < z1)


def fill_from_neighbours(label, todo, avoid=None):
    """Gives each `todo` cell a neighbouring label (4-neighbours), repeatedly, never `avoid[cell]`."""
    nz, nx = label.shape
    for _ in range(4 * (nz + nx)):
        if not todo.any():
            break
        p = np.pad(label, 1, constant_values=-1)
        for dr, dc in ((0, 1), (1, 0), (0, -1), (-1, 0)):
            nb = p[1 + dr : 1 + dr + nz, 1 + dc : 1 + dc + nx]
            take = todo & (nb >= 0)
            if avoid is not None:
                take &= nb != avoid
            label = np.where(take, nb, label)
            todo = todo & ~take
    return label, todo


def grow(g: Grid, zone, plain, cost_full, bands, seeds, owned):
    """The label raster: an index into `seeds` per zone cell, -1 outside."""
    n = g.nz * g.nx
    label = np.full((g.nz, g.nx), -1, np.int64)
    # Places first: each claims what it reaches within its travel budget, nearest place first.
    best = np.full(n, np.inf)
    for k, a in enumerate(seeds):
        if not a.get("place"):
            continue
        src = {(int(r), int(c)) for r, c in zip(*np.nonzero(rect_cells(g, a["footprint"]) & zone))}
        src.add(snap(g, zone, *centre(a["footprint"])))
        idx = sorted(r * g.nx + c for r, c in src)
        d = dijkstra(plain, directed=False, indices=idx, min_only=True, limit=a["reach"])
        closer = d < best
        best[closer] = d[closer]
        label.ravel()[closer] = k
    for k, a in enumerate(seeds):
        if a.get("place"):
            label[rect_cells(g, a["footprint"]) & zone] = k
    placed = label >= 0
    # The open country grows round them from the middle of its footprint, its seeds and the
    # features it owns; the middles come first, so no feature takes another area's heart.
    free = zone & ~placed
    src, lab = [], []
    cores = {}
    for k, a in enumerate(seeds):
        if not a.get("place"):
            cores[k] = rect_cells(g, shrink(a["footprint"], CORE)) & free
    taken = placed.copy()
    for mask in cores.values():
        taken |= mask
    for k, a in enumerate(seeds):
        if a.get("place"):
            continue
        cells = {(int(r), int(c)) for r, c in zip(*np.nonzero(cores[k]))}
        for p in [centre(a["footprint"])] + a.get("seeds", []):
            cells.add(snap(g, free, *p))
        for name in a.get("features", []):
            mask = owned[name] & free & ~taken
            taken |= mask
            cells |= {(int(r), int(c)) for r, c in zip(*np.nonzero(mask))}
        for r, c in sorted(cells):
            src.append(r * g.nx + c)
            lab.append(k)
    keep = free.ravel()
    sub = cost_full[keep][:, keep]
    where = np.flatnonzero(keep)
    pos = np.full(n, -1)
    pos[where] = np.arange(len(where))
    order = np.argsort(src, kind="stable")
    src_sub = pos[np.array(src)[order]]
    lab_sub = np.array(lab)[order]
    uniq, first = np.unique(src_sub, return_index=True)
    _, _, origin = dijkstra(sub, directed=False, indices=uniq, min_only=True, return_predecessors=True)
    lut = dict(zip(uniq.tolist(), lab_sub[first].tolist()))
    grown = np.array([lut.get(o, -1) for o in origin.tolist()])
    flat = label.ravel()
    flat[where] = grown
    label = flat.reshape(g.nz, g.nx)
    label[~zone] = -1
    # A river or road is a border, not a corridor: whoever reached a band first would own it
    # all the way along, so each band cell takes the label of the nearest ground beside it.
    seeded = np.zeros(n, bool)
    seeded[src] = True
    loose = bands & free & ~seeded.reshape(g.nz, g.nx)
    solid = free & ~loose & (label >= 0)
    if solid.any():
        _, (rows, cols) = ndimage.distance_transform_edt(~solid, return_indices=True)
        label = np.where(loose, label[rows, cols], label)
    return tidy(label, zone, len(seeds))


def tidy(label, zone, n):
    """One 4-connected piece per area, no unclaimed cells and no checkerboard corners."""
    for _ in range(20):
        changed = False
        # stray pieces join their neighbours
        for k in range(n):
            mask = label == k
            if not mask.any():
                continue
            piece, sizes = components(mask)
            if len(sizes) > 1:
                stray = (piece >= 0) & (piece != int(np.argmax(sizes)))
                label = np.where(stray, -2, label)
                label, left = fill_from_neighbours(label, stray, avoid=k)
                label = np.where(left, k, label)
                changed = True
        todo = zone & (label < 0)
        if todo.any():
            label, _ = fill_from_neighbours(np.where(todo, -2, label), todo)
            changed = True
        # A B / B A corners: the top right cell takes the top left's label
        a, b, c, d = label[:-1, :-1], label[:-1, 1:], label[1:, :-1], label[1:, 1:]
        bad = (a == d) & (b == c) & (a != b) & (a >= 0)
        if bad.any():
            rr, cc = np.nonzero(bad)
            label[rr, cc + 1] = label[rr, cc]
            changed = True
        if not changed:
            break
    return label


def ring_path(points, a, b, length):
    """The part of the closed ring `points` from the point nearest `a` to the one nearest `b`,
    in the direction whose length is closest to `length`."""
    ring = points + points[:1]
    cum = [0.0]
    for p, q in zip(ring, ring[1:]):
        cum.append(cum[-1] + math.dist(p, q))
    total = cum[-1]

    def at(x, z):
        best = (math.inf, 0, 0.0)
        for k, ((ax, az), (bx, bz)) in enumerate(zip(ring, ring[1:])):
            vx, vz = bx - ax, bz - az
            t = max(0.0, min(1.0, ((x - ax) * vx + (z - az) * vz) / ((vx * vx + vz * vz) or 1.0)))
            d = math.hypot(x - ax - t * vx, z - az - t * vz)
            if d < best[0]:
                best = (d, k, t)
        _, k, t = best
        return cum[k] + t * (cum[k + 1] - cum[k])

    s0, s1 = at(*a), at(*b)
    forward = (s1 - s0) % total
    backward = (s0 - s1) % total
    step = 1 if abs(forward - length) <= abs(backward - length) else -1
    inner = []
    n = len(points)
    if step == 1:
        k = next((i for i in range(n) if cum[i] > s0), n) % n
        for _ in range(n):
            if (cum[k] - s0) % total >= forward or (k == 0 and forward >= total):
                break
            inner.append(list(points[k]))
            k = (k + 1) % n
    else:
        k = next((i for i in reversed(range(n)) if cum[i] < s0), 0)
        for _ in range(n):
            if (s0 - cum[k]) % total >= backward:
                break
            inner.append(list(points[k]))
            k = (k - 1) % n
    return inner


def nearest_on_ring(points, x, z):
    ring = points + points[:1]
    best = (math.inf, (x, z))
    for (ax, az), (bx, bz) in zip(ring, ring[1:]):
        vx, vz = bx - ax, bz - az
        t = max(0.0, min(1.0, ((x - ax) * vx + (z - az) * vz) / ((vx * vx + vz * vz) or 1.0)))
        px, pz = ax + t * vx, az + t * vz
        d = math.hypot(x - px, z - pz)
        if d < best[0]:
            best = (d, (px, pz))
    return best[1]


def left_label(lab, v, w):
    """The label on the left of the cell edge v→w (vertex coordinates of the padded raster),
    with north up: x east, z south."""
    (r, c), (r2, c2) = v, w
    if c2 == c + 1:
        return int(lab[r - 1, c])
    if c2 == c - 1:
        return int(lab[r, c - 1])
    if r2 == r + 1:
        return int(lab[r, c])
    return int(lab[r - 1, c - 1])


def trace(g: Grid, label, outline):
    """Each area's rings in units: shared arcs simplified and smoothed once."""
    lab = np.pad(label, 1, constant_values=-1)
    chains, nodes = trace_arcs(lab)

    def xz(v):
        return (g.min_x + (v[1] - 1) * CELL, g.min_z + (v[0] - 1) * CELL)

    placed = {}
    for v in nodes:
        around = {int(lab[v[0] - 1, v[1] - 1]), int(lab[v[0] - 1, v[1]]), int(lab[v[0], v[1] - 1]), int(lab[v[0], v[1]])}
        placed[v] = nearest_on_ring(outline, *xz(v)) if -1 in around else xz(v)
    arcs = []  # (left, right, points in units, start node, end node)
    for pair, path in chains:
        left = left_label(lab, path[0], path[1])
        right = pair[0] if pair[1] == left else pair[1]
        closed = path[0] == path[-1] and path[0] not in nodes
        if closed:
            pts = [xz(v) for v in path[:-1]]
            if -1 in pair:
                pts = [tuple(p) for p in outline]
                # the outline runs the other way round when the area is on its right
                if signed_area(pts) * signed_area([xz(v) for v in path[:-1]]) < 0:
                    pts.reverse()
            else:
                half = len(pts) // 2
                pts = simplify(pts[: half + 1], SIMPLIFY)[:-1] + simplify(pts[half:] + pts[:1], SIMPLIFY)[:-1]
                pts = chaikin(pts, SMOOTH, closed=True)
            arcs.append((left, right, [to_units(p) for p in pts], None, None))
            continue
        a, b = placed[path[0]], placed[path[-1]]
        if -1 in pair:
            raw = [xz(v) for v in path]
            length = sum(math.dist(p, q) for p, q in zip(raw, raw[1:]))
            pts = [a] + ring_path(outline, a, b, length) + [b]
        else:
            pts = [a] + [xz(v) for v in path[1:-1]] + [b]
            pts = chaikin(simplify(pts, SIMPLIFY), SMOOTH)
        arcs.append((left, right, [to_units(p) for p in pts], path[0], path[-1]))
    return arcs


def to_units(p):
    return (int(round(p[0] * UNITS)), int(round(p[1] * UNITS)))


def signed_area(pts):
    return 0.5 * sum(ax * bz - bx * az for (ax, az), (bx, bz) in zip(pts, pts[1:] + pts[:1]))


def rings_of(k, arcs):
    """Area `k`'s rings, chained from the arcs it lies left of (reversed where it lies right)."""
    rings, open_ = [], []
    for left, right, pts, s, e in arcs:
        if k not in (left, right):
            continue
        pts, s, e = (pts, s, e) if left == k else (pts[::-1], e, s)
        if s is None:
            rings.append(list(pts))
        else:
            open_.append((s, e, pts))
    by_start = {}
    for s, e, pts in open_:
        if s in by_start:
            raise SystemExit(f"area {k}: two borders leave one corner; widen the area or move a seed")
        by_start[s] = (e, pts)
    while by_start:
        s0 = min(by_start)
        ring, s = [], s0
        while True:
            e, pts = by_start.pop(s)
            ring += pts[:-1]
            s = e
            if s == s0:
                break
            if s not in by_start:
                raise SystemExit(f"area {k}: a border ends nowhere")
        rings.append(ring)
    rings.sort(key=lambda r: -abs(signed_area(r)))
    # drop repeated points the rounding made
    out = []
    for r in rings:
        clean = [p for i, p in enumerate(r) if p != r[i - 1]]
        out.append(clean)
    return out


def centre_of(g: Grid, label, k):
    """The point deepest inside the area and its distance to the area's edge, in metres."""
    mask = np.pad(label == k, 1, constant_values=False)
    d = ndimage.distance_transform_edt(mask)[1:-1, 1:-1]
    r, c = np.unravel_index(int(np.argmax(d)), d.shape)
    return g.centre(r, c), float(d[r, c]) * CELL - CELL / 2


def segments_cross(arcs) -> list[str]:
    """Pairs of border segments that cross: the traced rings must be simple and disjoint."""
    segs = []
    for n, (_, _, pts, s, e) in enumerate(arcs):
        ring = pts + pts[:1] if s is None else pts
        for i, (p, q) in enumerate(zip(ring, ring[1:])):
            segs.append((n, i, len(ring) - 1, p, q, s is None))
    bucket = {}
    size = 2000
    for k, (_, _, _, p, q, _) in enumerate(segs):
        for bx in range(min(p[0], q[0]) // size, max(p[0], q[0]) // size + 1):
            for bz in range(min(p[1], q[1]) // size, max(p[1], q[1]) // size + 1):
                bucket.setdefault((bx, bz), []).append(k)

    def orient(a, b, c):
        v = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
        return (v > 0) - (v < 0)

    problems = set()
    for ks in bucket.values():
        for x in range(len(ks)):
            for y in range(x + 1, len(ks)):
                s1, s2 = segs[ks[x]], segs[ks[y]]
                p1, q1, p2, q2 = s1[3], s1[4], s2[3], s2[4]
                if len({p1, q1, p2, q2}) < 4:
                    continue  # neighbours along a border, or borders meeting at a corner
                if orient(p1, q1, p2) * orient(p1, q1, q2) < 0 and orient(p2, q2, p1) * orient(p2, q2, q1) < 0:
                    problems.add(f"borders cross near ({p1[0] / UNITS:.0f}, {p1[1] / UNITS:.0f}) m")
    return sorted(problems)


def build():
    region = load("region.json")
    design = load("design.json")
    landscape = load("landscape.json")
    seeds = json.loads((GREYHAVEN / "authoring" / "area-seeds.json").read_text())["areas"]
    outline = [(x / UNITS, z / UNITS) for x, z in region["outline"]]
    g = Grid(outline)
    zone = inside(g, outline)
    h = heights(g)
    water, road, forest, rocks = layers(g, design, landscape)
    cost = graph(g, zone, h, water, road, forest, rocks)
    # a place is its ground whatever brook or lane runs through it: only distance and climbing
    none = np.zeros((g.nz, g.nx), bool)
    plain = graph(g, zone, h, none, none, none.astype(np.int32), none.astype(np.int32))
    label = grow(g, zone, plain, cost, water | road, seeds, features(g, design))
    arcs = trace(g, label, outline)
    problems = segments_cross(arcs)
    areas = []
    for k, a in enumerate(seeds):
        (cx, cz), radius = centre_of(g, label, k)
        areas.append(
            {
                "id": a["id"],
                "name": a["name"],
                "centre": [int(round(cx * UNITS)), int(round(cz * UNITS))],
                "radius": int(round(radius * UNITS)),
                "rings": [[list(p) for p in r] for r in rings_of(k, arcs)],
            }
        )
    areas.sort(key=lambda a: a["id"])
    return g, label, areas, problems


NOTE = (
    "Generated by content/greyhaven/authoring/areas.py from authoring/area-seeds.json, region.json, "
    "terrain.json, design.json and landscape.json; do not edit. Greyhaven's named areas (#1063): each a "
    "polygon in units (cm) in the zone frame, its outer ring first, then any holes. Together they tile "
    "the zone's outline. `centre` is the point deepest inside, `radius` its distance to the edge."
)


def render(areas) -> str:
    lines = ["{", '  "version": 1,', f'  "note": {json.dumps(NOTE)},', '  "areas": [']
    for n, a in enumerate(areas):
        lines.append("    {")
        lines.append(f'      "id": {a["id"]},')
        lines.append(f'      "name": {json.dumps(a["name"])},')
        lines.append(f'      "centre": {json.dumps(a["centre"])},')
        lines.append(f'      "radius": {a["radius"]},')
        lines.append('      "rings": [')
        for m, ring in enumerate(a["rings"]):
            comma = "," if m + 1 < len(a["rings"]) else ""
            lines.append(f"        {json.dumps(ring, separators=(',', ':'))}{comma}")
        lines.append("      ]")
        lines.append("    }" + ("," if n + 1 < len(areas) else ""))
    lines += ["  ]", "}", ""]
    return "\n".join(lines)


def preview(g: Grid, label, areas, path: Path):
    scale = 2
    rng = np.random.default_rng(7)
    colours = rng.integers(70, 230, size=(len(areas) + 1, 3))
    rgb = np.where((label >= 0)[..., None], colours[np.maximum(label, 0)], 40).astype(np.uint8)
    img = Image.fromarray(rgb).resize((g.nx * scale, g.nz * scale), Image.NEAREST)
    draw = ImageDraw.Draw(img)

    def px(x, z):
        return ((x / UNITS - g.min_x) / CELL * scale, (z / UNITS - g.min_z) / CELL * scale)

    for a in areas:
        for ring in a["rings"]:
            draw.line([px(*p) for p in ring + ring[:1]], fill=(20, 20, 20), width=1)
    for a in areas:
        x, z = px(*a["centre"])
        draw.text((x, z), a["name"], fill=(0, 0, 0), anchor="mm")
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="fail when areas.json is stale")
    args = parser.parse_args()
    g, label, areas, problems = build()
    for p in problems:
        print(f"problem: {p}", file=sys.stderr)
    text = render(areas)
    target = GREYHAVEN / "areas.json"
    if args.check:
        if target.read_text() != text:
            print("content/greyhaven/areas.json is stale: run content/greyhaven/authoring/areas.py", file=sys.stderr)
            return 1
        return 1 if problems else 0
    target.write_text(text)
    preview(g, label, areas, OUT / "areas.png")
    vertices = sum(len(r) for a in areas for r in a["rings"])
    print(f"{len(areas)} areas, {vertices} vertices -> {target.relative_to(REPO)}; preview in {(OUT / 'areas.png').relative_to(REPO)}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
