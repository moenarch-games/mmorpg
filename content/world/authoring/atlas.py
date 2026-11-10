# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy>=2", "pillow>=10.1"]
# ///
"""Continent atlas: composes the editable layers in content/world/ into elevation, water,
climate, biomes, natural regions, kingdom claims and roads, renders previews and checks
geographic consistency.

    uv run content/world/authoring/atlas.py build [--previews docs/images/world]
    uv run content/world/authoring/atlas.py check
    uv run content/world/authoring/atlas.py seed-regions
    uv run content/world/authoring/atlas.py format

Outputs go to artifacts/world-atlas/ (ignored). Only seed-regions (which only appends region
seeds) and format (which only changes layout) write source files. No game crate reads any of
this yet (docs/WORLD_ATLAS.md).
"""

from __future__ import annotations

import argparse
import hashlib
import heapq
import json
import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, str(Path(__file__).resolve().parent))
from atlas_json import write_json  # noqa: E402

REPO = Path(__file__).resolve().parents[3]
WORLD = REPO / "content" / "world"
OUT = REPO / "artifacts" / "world-atlas"
INF = float("inf")


# ---------------------------------------------------------------- source layers


def load_layers() -> dict:
    manifest = json.loads((WORLD / "atlas.json").read_text())
    layers = {"atlas": manifest}
    for name in manifest["layers"]:
        layers[name.removesuffix(".json")] = json.loads((WORLD / name).read_text())
    for key in ("overrides/height", "overrides/biome"):
        if layers[key]["cell"] != manifest["cell"]:
            sys.exit(f"{key}.json is on a {layers[key]['cell']} m grid, the atlas on {manifest['cell']} m")
    return layers


def source_digest() -> str:
    h = hashlib.sha256()
    for path in sorted(WORLD.rglob("*.json")):
        h.update(path.relative_to(WORLD).as_posix().encode())
        h.update(path.read_bytes())
    return h.hexdigest()[:16]


class Grid:
    def __init__(self, atlas: dict):
        d = atlas["domain"]
        self.cell = atlas["cell"]
        self.min_x, self.min_z = d["minX"], d["minZ"]
        self.nx = (d["maxX"] - d["minX"]) // self.cell
        self.nz = (d["maxZ"] - d["minZ"]) // self.cell
        xs = self.min_x + (np.arange(self.nx) + 0.5) * self.cell
        zs = self.min_z + (np.arange(self.nz) + 0.5) * self.cell
        self.X, self.Z = np.meshgrid(xs, zs)

    def index(self, x: float, z: float) -> tuple[int, int]:
        c = int((x - self.min_x) // self.cell)
        r = int((z - self.min_z) // self.cell)
        return min(max(r, 0), self.nz - 1), min(max(c, 0), self.nx - 1)

    def centre(self, r: int, c: int) -> tuple[float, float]:
        return self.min_x + (c + 0.5) * self.cell, self.min_z + (r + 0.5) * self.cell


# ---------------------------------------------------------------- deterministic noise

MASK = 0xFFFFFFFF


def lattice_hash(ix: np.ndarray, iz: np.ndarray, seed: int) -> np.ndarray:
    h = (ix.astype(np.int64) * 374761393 + iz.astype(np.int64) * 668265263 + seed * 2246822519) & MASK
    h = h.astype(np.uint64)
    h = ((h ^ (h >> np.uint64(13))) * np.uint64(1274126177)) & np.uint64(MASK)
    h = h ^ (h >> np.uint64(16))
    return h.astype(np.float64) / MASK * 2.0 - 1.0


def value_noise(x: np.ndarray, z: np.ndarray, wavelength: float, seed: int) -> np.ndarray:
    fx, fz = x / wavelength, z / wavelength
    ix, iz = np.floor(fx), np.floor(fz)
    tx, tz = fx - ix, fz - iz
    sx, sz = tx * tx * (3 - 2 * tx), tz * tz * (3 - 2 * tz)
    a = lattice_hash(ix, iz, seed)
    b = lattice_hash(ix + 1, iz, seed)
    c = lattice_hash(ix, iz + 1, seed)
    d = lattice_hash(ix + 1, iz + 1, seed)
    return (a + (b - a) * sx) * (1 - sz) + (c + (d - c) * sx) * sz


def fbm(x, z, wavelength, octaves, seed, ridged=False):
    total, amp, norm = 0.0, 1.0, 0.0
    for o in range(octaves):
        n = value_noise(x, z, wavelength / 2**o, seed + 101 * o)
        if ridged:
            n = 1.0 - 2.0 * np.abs(n)
        total = total + amp * n
        norm += amp
        amp *= 0.5
    return total / norm


def smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


# ---------------------------------------------------------------- geometry


def dist_to_polyline(X, Z, pts, closed=False):
    pts = [tuple(p) for p in pts] + ([tuple(pts[0])] if closed else [])
    d = np.full(X.shape, INF)
    for (ax, az), (bx, bz) in zip(pts, pts[1:]):
        vx, vz = bx - ax, bz - az
        t = np.clip(((X - ax) * vx + (Z - az) * vz) / (vx * vx + vz * vz), 0.0, 1.0)
        d = np.minimum(d, np.hypot(X - ax - t * vx, Z - az - t * vz))
    return d


def inside_polygon(X, Z, pts):
    inside = np.zeros(X.shape, bool)
    n = len(pts)
    for i in range(n):
        (ax, az), (bx, bz) = pts[i], pts[(i + 1) % n]
        crosses = (az > Z) != (bz > Z)
        with np.errstate(divide="ignore", invalid="ignore"):
            xi = ax + (Z - az) * (bx - ax) / (bz - az)
        inside ^= crosses & (X < xi)
    return inside


def dist_to_rect(X, Z, rect):
    x0, z0, x1, z1 = rect
    dx = np.maximum(np.maximum(x0 - X, X - x1), 0)
    dz = np.maximum(np.maximum(z0 - Z, Z - z1), 0)
    return np.hypot(dx, dz)


def bearing_of(dx, dz):
    """Compass bearing in degrees: 0 north (-z), 90 east (+x)."""
    return np.degrees(np.arctan2(dx, -dz)) % 360.0


def gate_position(crown: dict, gate: dict, radius: float | None = None) -> tuple[float, float]:
    b = math.radians(gate["bearing"])
    r = crown["radius"] if radius is None else radius
    return crown["center"][0] + r * math.sin(b), crown["center"][1] - r * math.cos(b)


# ---------------------------------------------------------------- elevation


def land_field(g: Grid, terrain: dict, seed: int) -> np.ndarray:
    coast = terrain["coast"]
    X, Z = g.X, g.Z
    d = dist_to_polyline(X, Z, coast["points"], closed=True)
    f = np.where(inside_polygon(X, Z, coast["points"]), d, -d)
    f = f + coast["warp"]["amplitude"] * fbm(X, Z, coast["warp"]["wavelength"], 5, seed + 11)
    for sea in coast["seas"]:
        f = np.minimum(f, np.hypot(X - sea["center"][0], Z - sea["center"][1]) - sea["radius"])
    for isle in coast["islands"]:
        f = np.maximum(f, isle["radius"] - np.hypot(X - isle["center"][0], Z - isle["center"][1]))
    return f


def crown_elevation(g: Grid, crown: dict, base: np.ndarray, seed: int, warp: float) -> np.ndarray:
    X = g.X + warp * fbm(g.X, g.Z, 1800, 3, seed + 21)
    Z = g.Z + warp * fbm(g.X, g.Z, 1800, 3, seed + 22)
    dx, dz = X - crown["center"][0], Z - crown["center"][1]
    r = np.hypot(dx, dz)
    bearing = bearing_of(dx, dz)
    outer = base + crown["dome"]["amplitude"] * np.exp(-((r / crown["dome"]["radius"]) ** 2))
    crest = np.full(r.shape, float(crown["crest"]))
    crest *= 1.0 + crown["crestVariation"] * value_noise(bearing, np.zeros_like(bearing), 25.0, seed + 23)
    ring = np.exp(-(((r - crown["radius"]) / crown["width"]) ** 2))
    for gate in crown["gates"]:
        ad = np.abs((bearing - gate["bearing"] + 180.0) % 360.0 - 180.0)
        w = 1.0 - smoothstep(gate["halfWidth"] * 0.3, gate["halfWidth"] * 2.0, ad)
        crest = crest * (1 - w) + gate["floor"] * w
    mount = crest * ring + outer * (1 - ring)
    mount = np.where(r > crown["radius"], np.maximum(mount, outer), mount)
    ri = crown["interior"]["radius"]
    interior = crown["interior"]
    drain = next(x for x in crown["gates"] if x["id"] == interior["drainsTo"])
    toward = np.cos(np.radians(bearing - drain["bearing"]))
    bowl = interior["floor"] + 40.0 * (r / ri) ** 2 - interior["tilt"] * toward * np.minimum(r / ri, 1.0)
    t = smoothstep(ri * 0.85, crown["radius"], r)
    return np.where(r < crown["radius"], bowl * (1 - t) + mount * t, mount)


def build_elevation(g: Grid, layers: dict, seed: int) -> dict:
    terrain = layers["terrain"]
    lf = land_field(g, terrain, seed)
    land = remove_small(lf > 0, 40)
    low = terrain["lowland"]
    base = low["rise"] * (1 - np.exp(-np.maximum(lf, 0) / low["scale"]))
    noise = terrain["noise"]
    elev = crown_elevation(g, terrain["crown"], base, seed, noise["warp"])
    wx = g.X + 0.9 * noise["warp"] * fbm(g.X, g.Z, 2600, 3, seed + 33)
    wz = g.Z + 0.9 * noise["warp"] * fbm(g.X, g.Z, 2600, 3, seed + 34)
    for ridge in terrain["ridges"]:
        d = dist_to_polyline(wx, wz, ridge["points"])
        profile = np.clip(1 - d / ridge["halfWidth"], 0, 1) ** 1.6
        crestn = 0.7 + 0.6 * (fbm(g.X, g.Z, 1100, 4, seed + 31, ridged=True) + 1) / 2
        elev = elev + ridge["height"] * profile * crestn
    for basin in terrain["basins"]:
        d = np.hypot(g.X - basin["center"][0], g.Z - basin["center"][1])
        t = smoothstep(0, basin["radius"], d)
        elev = basin["floor"] + (elev - basin["floor"]) * (0.15 + 0.85 * t) ** 1.4
    rough = np.clip((elev - 150) / 700, 0, 1)
    elev = elev + noise["amplitude"] * fbm(g.X, g.Z, noise["wavelength"], noise["octaves"], seed + 41)
    elev = elev + noise["mountainAmplitude"] * rough * (
        fbm(g.X, g.Z, noise["wavelength"] * 0.6, noise["octaves"], seed + 42, ridged=True) - 0.35
    )
    elev = elev * smoothstep(0, 900, lf) + np.where(land, 2.0, 0.0) * (1 - smoothstep(0, 900, lf))
    sea_depth = -(4 + 70 * (1 - np.exp(np.minimum(lf, 0) / 1600)))
    elev = np.where(land, np.maximum(elev, 1.0), sea_depth)
    return {"elev": elev, "land": land, "landField": lf}


def polyline_projection(X, Z, pts):
    """Distance to a polyline and the arc length of the nearest point on it."""
    d = np.full(X.shape, INF)
    arc = np.zeros(X.shape)
    s0 = 0.0
    for (ax, az), (bx, bz) in zip(pts, pts[1:]):
        vx, vz = bx - ax, bz - az
        seg = math.hypot(vx, vz)
        t = np.clip(((X - ax) * vx + (Z - az) * vz) / (seg * seg), 0.0, 1.0)
        dd = np.hypot(X - ax - t * vx, Z - az - t * vz)
        closer = dd < d
        d = np.where(closer, dd, d)
        arc = np.where(closer, s0 + t * seg, arc)
        s0 += seg
    return d, arc


def polyline_arcs(pts):
    out, s0 = [0.0], 0.0
    for (ax, az), (bx, bz) in zip(pts, pts[1:]):
        s0 += math.hypot(bx - ax, bz - az)
        out.append(s0)
    return out


def meander(samples, arcs, carve, seed):
    """Displace a course sideways by seeded noise along its length; the ends stay put."""
    amp, wl = carve["meander"]["amplitude"], carve["meander"]["wavelength"]
    a = np.array(arcs)
    off = amp * fbm(a, np.full_like(a, 0.5), wl, 3, seed)
    taper = np.minimum(1.0, np.minimum(a, a[-1] - a) / wl)
    out = []
    for i, (x, z) in enumerate(samples):
        j0, j1 = max(i - 1, 0), min(i + 1, len(samples) - 1)
        tx, tz = samples[j1][0] - samples[j0][0], samples[j1][1] - samples[j0][1]
        n = math.hypot(tx, tz) or 1.0
        k = off[i] * taper[i]
        out.append((x - tz / n * k, z + tx / n * k))
    return out


def carve_rivers(g: Grid, elev: np.ndarray, land: np.ndarray, water: dict, seed: int) -> dict:
    """Each authored river gets a strictly falling bed; its valley is cut down to that bed plus a
    side slope, so the cut meets the terrain smoothly however high the ground around it is."""
    carve = water["carve"]
    profiles = {}
    for river in water["rivers"]:
        pts = river["points"]
        samples, arcs, s0 = [], [], 0.0
        for (ax, az), (bx, bz) in zip(pts, pts[1:]):
            seg = math.hypot(bx - ax, bz - az)
            n = max(1, int(seg / (g.cell * 0.5)))
            samples += [(ax + (bx - ax) * k / n, az + (bz - az) * k / n) for k in range(n)]
            arcs += [s0 + seg * k / n for k in range(n)]
            s0 += seg
        samples.append(tuple(pts[-1]))
        arcs.append(s0)
        samples = meander(samples, arcs, carve, seed + sum(map(ord, river["id"])))
        # the bed lies below the lowest ground across the valley, so no side slope can steal the flow
        half = int(carve["valleyHalfWidth"] * 0.6 // g.cell)
        bed, prev = [], None
        for i, (x, z) in enumerate(samples):
            r0, c0 = g.index(x, z)
            win = elev[max(r0 - half, 0) : r0 + half + 1, max(c0 - half, 0) : c0 + half + 1]
            dry = land[max(r0 - half, 0) : r0 + half + 1, max(c0 - half, 0) : c0 + half + 1]
            here = (win[dry].min() if dry.any() else elev[r0, c0]) - carve["bedDepth"]
            if prev is not None:
                here = min(here, prev - carve["minGrade"] * (arcs[i] - arcs[i - 1]))
            bed.append(here)
            prev = here
        # project onto the meandering course, coarsened to ~120 m segments, inside its bounding box
        step = max(1, int(120 // (g.cell * 0.5)))
        coarse = samples[::step] + [samples[-1]]
        coarse_arc = arcs[::step] + [arcs[-1]]
        xs, zs = [p[0] for p in samples], [p[1] for p in samples]
        pad = carve["valleyHalfWidth"] + g.cell
        r0, c0 = g.index(min(xs) - pad, min(zs) - pad)
        r1, c1 = g.index(max(xs) + pad, max(zs) + pad)
        rs, cs = slice(r0, r1 + 1), slice(c0, c1 + 1)
        d, along = polyline_projection(g.X[rs, cs], g.Z[rs, cs], coarse)
        arc = np.interp(along, polyline_arcs(coarse), coarse_arc)
        cut = np.interp(arc, arcs, bed) + carve["bedDepth"] * smoothstep(0, 3 * g.cell, d) + carve["sideSlope"] * d
        w = 1 - smoothstep(carve["valleyHalfWidth"] * 0.5, carve["valleyHalfWidth"], d)
        sub = elev[rs, cs]
        elev[rs, cs] = np.where(land[rs, cs], np.minimum(sub, sub * (1 - w) + cut * w), sub)
        # an unbroken chain of bed cells falling with the course
        for (x, z), level in zip(samples, bed):
            r0, c0 = g.index(x, z)
            if land[r0, c0]:
                elev[r0, c0] = min(elev[r0, c0], level)
        profiles[river["id"]] = {"samples": samples, "bed": bed, "length": s0}
    return profiles


def apply_cells(target: np.ndarray, layer: dict, add: bool):
    for col, row, value in layer["cells"]:
        if add:
            target[row, col] += value
        else:
            target[row, col] = value


def anchor_fields(g: Grid, anchors: dict):
    """Per anchor: mask of its footprint and the coarse stand-in elevation around it."""
    out = []
    for a in anchors["anchors"]:
        mask = np.zeros(g.X.shape, bool)
        inside = np.zeros(g.X.shape)
        nearest = np.full(g.X.shape, INF)
        num = np.zeros(g.X.shape)
        den = np.zeros(g.X.shape)
        for part in a["parts"]:
            d = dist_to_rect(g.X, g.Z, part["rect"])
            level = a["referenceElevation"] + part["local"]
            inside = np.where(d == 0, level, inside)
            # outside, the parts' levels blend by inverse distance so no seam runs off a part's edge
            wgt = 1.0 / (d + 60.0) ** 2
            num += wgt * level
            den += wgt
            nearest = np.minimum(nearest, d)
            mask |= d == 0
        target = np.where(mask, inside, num / den)
        out.append((a, mask, target, nearest))
    return out


def apply_anchors(elev: np.ndarray, fields) -> None:
    for a, mask, target, nearest in fields:
        w = 1 - smoothstep(0, a["blend"], nearest)
        elev[:] = elev * (1 - w) + target * w
        elev[mask] = target[mask]


# ---------------------------------------------------------------- hydrology

NB8 = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]


def padded(a: np.ndarray, fill) -> np.ndarray:
    return np.pad(a, 1, constant_values=fill)


def priority_flood(elev: np.ndarray, seeds: np.ndarray, blocked: np.ndarray) -> np.ndarray:
    """Barnes priority-flood with epsilon: every non-seed cell gets a strictly lower path to a seed."""
    nz, nx = elev.shape
    w = nx + 2
    h = padded(elev, INF).ravel().tolist()
    closed = padded(blocked | seeds, True).ravel().tolist()
    offs = [dr * w + dc for dr, dc in NB8]
    heap = [(h[(r + 1) * w + c + 1], (r + 1) * w + c + 1) for r, c in zip(*np.nonzero(seeds))]
    heapq.heapify(heap)
    eps = 1e-3
    while heap:
        e, i = heapq.heappop(heap)
        for o in offs:
            j = i + o
            if closed[j]:
                continue
            closed[j] = True
            if h[j] <= e:
                h[j] = e + eps
            heapq.heappush(heap, (h[j], j))
    return np.array(h).reshape(nz + 2, nx + 2)[1:-1, 1:-1]


def receivers(filled: np.ndarray, terminal: np.ndarray, cell: float) -> np.ndarray:
    nz, nx = filled.shape
    p = padded(filled, INF)
    best = np.zeros(filled.shape)
    rec = np.full(filled.shape, -1, np.int64)
    idx = np.arange(nz * nx).reshape(nz, nx)
    pidx = padded(idx, -1)
    for dr, dc in NB8:
        nb = p[1 + dr : 1 + dr + nz, 1 + dc : 1 + dc + nx]
        drop = (filled - nb) / (cell * math.hypot(dr, dc))
        better = drop > best
        best = np.where(better, drop, best)
        rec = np.where(better, pidx[1 + dr : 1 + dr + nz, 1 + dc : 1 + dc + nx], rec)
    rec[terminal] = -1
    return rec


def accumulate(filled, rec, weight, transfers):
    order = np.argsort(-filled, axis=None, kind="stable").tolist()
    acc = weight.ravel().astype(np.float64).tolist()
    recl = rec.ravel().tolist()
    for i in order:
        j = recl[i]
        if j >= 0:
            acc[j] += acc[i]
        elif i in transfers:
            acc[transfers[i]] += acc[i]
    return np.array(acc).reshape(filled.shape), order


def outlets(rec: np.ndarray, order: list) -> np.ndarray:
    """Terminal cell each cell drains to (itself when terminal)."""
    recl = rec.ravel().tolist()
    out = list(range(len(recl)))
    for i in reversed(order):
        j = recl[i]
        if j >= 0:
            out[i] = out[j]
    return np.array(out).reshape(rec.shape)


def hydrology(g: Grid, elev, land, water, anchor_mask, precip):
    seeds = ~land
    sinks = {}
    for sink in water["sinks"]:
        r, c = g.index(*sink["at"])
        if sink["kind"].startswith("endorheic"):
            win = elev[max(r - 12, 0) : r + 13, max(c - 12, 0) : c + 13]
            dr, dc = np.unravel_index(np.argmin(win), win.shape)
            r, c = max(r - 12, 0) + dr, max(c - 12, 0) + dc
        seeds[r, c] = True
        sinks[sink["id"]] = (r, c)
    filled = priority_flood(elev, seeds, np.zeros_like(land))
    rec = receivers(filled, seeds, g.cell)
    transfers = {}
    springs = {s["id"]: g.index(*s["at"]) for s in water["springs"]}
    for sink in water["sinks"]:
        if "resurgence" in sink:
            r, c = sinks[sink["id"]]
            sr, sc = springs[sink["resurgence"]]
            transfers[r * g.nx + c] = sr * g.nx + sc
    weight = np.where(land, 0.15 + precip, 0.0) * (g.cell * g.cell) / 1e6
    acc, order = accumulate(filled, rec, weight, transfers)
    depth = filled - elev
    lake = land & (depth > 1.0) & ~anchor_mask
    lake = remove_small(lake, 6)
    return {"filled": filled, "rec": rec, "acc": acc, "order": order, "lake": lake,
            "sinks": sinks, "springs": springs, "transfers": transfers, "seeds": seeds,
            "outlet": outlets(rec, order)}


def components(mask: np.ndarray):
    nz, nx = mask.shape
    lab = np.full(mask.shape, -1, np.int64)
    m = mask.tolist()
    labl = lab.tolist()
    sizes = []
    for r0 in range(nz):
        for c0 in range(nx):
            if m[r0][c0] and labl[r0][c0] < 0:
                k = len(sizes)
                stack, n = [(r0, c0)], 0
                labl[r0][c0] = k
                while stack:
                    r, c = stack.pop()
                    n += 1
                    for dr, dc in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                        rr, cc = r + dr, c + dc
                        if 0 <= rr < nz and 0 <= cc < nx and m[rr][cc] and labl[rr][cc] < 0:
                            labl[rr][cc] = k
                            stack.append((rr, cc))
                sizes.append(n)
    return np.array(labl), sizes


def remove_small(mask, min_cells):
    lab, sizes = components(mask)
    keep = np.array([s >= min_cells for s in sizes] + [False])
    return keep[lab]


# ---------------------------------------------------------------- climate and biomes


def blur(a: np.ndarray, n: int) -> np.ndarray:
    for _ in range(n):
        p = np.pad(a, 1, mode="edge")
        a = (p[:-2, 1:-1] + p[2:, 1:-1] + p[1:-1, :-2] + p[1:-1, 2:] + 4 * a) / 8
    return a


def precipitation(g: Grid, elev, land, climate) -> np.ndarray:
    pc = climate["precipitation"]

    def sweep(e, lnd):
        """Air crosses the grid column by column: it loses moisture to rain over land (more where it
        is lifted up a slope), regains it over sea, and rains less where it sinks down a lee slope.
        The index is rain relative to flat land under saturated air, so 1.0 is a wet coast."""
        m = np.ones(e.shape[0])
        out = np.zeros(e.shape)
        prev = e[:, 0]
        for j in range(e.shape[1]):
            col = e[:, j]
            slope = (col - prev) / g.cell
            rate = pc["base"] + pc["orographic"] * np.maximum(slope, 0)
            rain = np.where(lnd[:, j], np.minimum(m * rate, m), 0.0)
            lee = np.clip(-slope * pc["lee"], 0, 0.8)
            out[:, j] = rain / pc["base"] * (1 - lee)
            m = np.where(lnd[:, j], m - rain, m + (1 - m) * pc["recharge"])
            prev = col
        return out

    wet = np.maximum(elev, 0)
    west = sweep(wet, land)
    south = np.flipud(sweep(np.flipud(wet).T, np.flipud(land).T).T)
    p = west + climate["wind"]["secondaryWeight"] * south
    return np.clip(blur(p, pc["smoothCells"]), 0, 1.3)


def temperature(g: Grid, elev, climate) -> np.ndarray:
    t = climate["temperature"]
    u = (g.Z - g.Z.min()) / (g.Z.max() - g.Z.min())
    return t["north"] + (t["south"] - t["north"]) * u - t["lapsePerKm"] * np.maximum(elev, 0) / 1000


def slope_degrees(elev, cell):
    gz, gx = np.gradient(elev, cell)
    return np.degrees(np.arctan(np.hypot(gx, gz)))


def classify_biomes(g, elev, land, hyd, temp, precip, slope, climate, anchor_mask, sink_mask):
    biome = np.full(elev.shape, -1, np.int64)
    wetness = np.log10(np.maximum(hyd["acc"], 1e-6)) + 0.6  # >= 1.0 where a catchment exceeds ~2.5 km²
    for rule in climate["biomes"]:
        cond = rule.get("when")
        if cond is None:
            continue
        ok = land & (biome < 0)
        if "tMax" in cond:
            ok &= temp <= cond["tMax"]
        if "slopeMin" in cond:
            ok &= slope >= cond["slopeMin"]
        if "slopeMax" in cond:
            ok &= slope <= cond["slopeMax"]
        if "pMin" in cond:
            ok &= precip >= cond["pMin"]
        if "pMax" in cond:
            ok &= precip <= cond["pMax"]
        if "wetMin" in cond:
            ok &= (wetness >= cond["wetMin"]) | hyd["lake_edge"]
        if "sink" in cond:
            ok &= sink_mask
        biome[ok] = rule["id"]
    biome[~land] = 0
    coast = land & (padded(~land, False)[2:, 1:-1] | padded(~land, False)[:-2, 1:-1]
                    | padded(~land, False)[1:-1, 2:] | padded(~land, False)[1:-1, :-2]) & (elev < 8)
    biome[coast] = 2
    biome[hyd["lake"]] = 1
    biome[anchor_mask] = 15
    return biome


# ---------------------------------------------------------------- graph search on the grid


def grid_search(cost_h, passable, extra, sources, k_abs, k_sq=0.0, target=None, cell=40.0):
    """Multi-source Dijkstra on 8-neighbours. Returns (dist, label) flat lists on the padded grid."""
    nz, nx = cost_h.shape
    w = nx + 2
    h = padded(cost_h, 0.0).ravel().tolist()
    ok = padded(passable, False).ravel().tolist()
    ex = padded(extra, 0.0).ravel().tolist()
    offs = [(dr * w + dc, cell * math.hypot(dr, dc)) for dr, dc in NB8]
    dist = [INF] * len(h)
    lab = [-1] * len(h)
    prev = [-1] * len(h)
    heap = []
    for (r, c), label in sources:
        i = (r + 1) * w + c + 1
        dist[i], lab[i] = 0.0, label
        heap.append((0.0, i))
    heapq.heapify(heap)
    goal = None if target is None else (target[0] + 1) * w + target[1] + 1
    while heap:
        d, i = heapq.heappop(heap)
        if d > dist[i]:
            continue
        if i == goal:
            break
        hi = h[i]
        for o, ds in offs:
            j = i + o
            if not ok[j]:
                continue
            dh = abs(h[j] - hi)
            nd = d + ds + k_abs * dh + k_sq * dh * dh / ds + ex[j]
            if nd < dist[j]:
                dist[j], lab[j], prev[j] = nd, lab[i], i
                heapq.heappush(heap, (nd, j))
    return dist, lab, prev, w


def unpad(flat, nz, nx, dtype=float):
    return np.array(flat, dtype=dtype).reshape(nz + 2, nx + 2)[1:-1, 1:-1]


# ---------------------------------------------------------------- regions


def region_seed_candidates(g: Grid, land, anchor_mask, regions, seed):
    """Jittered hexagonal seeds on land, away from existing seeds. Deterministic from the atlas seed."""
    s = regions["spacing"]
    existing = [tuple(r["seed"]) for r in regions["regions"]]
    out = []
    row = 0
    z = g.min_z + s / 2
    while z < g.min_z + g.nz * g.cell:
        x = g.min_x + s / 2 + (s / 2 if row % 2 else 0)
        while x < g.min_x + g.nx * g.cell:
            j = lattice_hash(np.array([int(x)]), np.array([int(z)]), seed + 77)[0]
            k = lattice_hash(np.array([int(z)]), np.array([int(x)]), seed + 78)[0]
            px, pz = x + 0.35 * s * j, z + 0.35 * s * k
            r, c = g.index(px, pz)
            if land[r, c] and not anchor_mask[r, c] and all(math.hypot(px - ex, pz - ez) > 0.6 * s for ex, ez in existing + out):
                out.append((round(px / 10) * 10, round(pz / 10) * 10))
            x += s
        z += s * math.sqrt(3) / 2
        row += 1
    return out


def grow_regions(g, elev, land, hyd, anchors_f, regions, big_river):
    sources = []
    ids = [r["id"] for r in regions["regions"]]
    for k, reg in enumerate(regions["regions"]):
        r, c = g.index(*reg["seed"])
        sources.append(((r, c), k))
        if reg.get("anchor"):
            for a, mask, _, _ in anchors_f:
                if a["id"] == reg["anchor"]:
                    sources += [((rr, cc), k) for rr, cc in zip(*np.nonzero(mask))]
    extra = np.where(big_river, 900.0, 0.0)
    passable = land.copy()
    anchor_any = np.zeros_like(land)
    for a, mask, _, _ in anchors_f:
        anchor_any |= mask
    dist, lab, _, _ = grid_search(elev, passable, extra, sources, k_abs=14.0, cell=g.cell)
    label = unpad(lab, g.nz, g.nx, np.int64)
    label[~land] = -1
    # islets without a seed join the nearest region
    for _ in range(400):
        missing = land & (label < 0)
        if not missing.any():
            break
        p = padded(label, -1)
        for dr, dc in NB8:
            nb = p[1 + dr : 1 + dr + g.nz, 1 + dc : 1 + dc + g.nx]
            label = np.where(missing & (label < 0) & (nb >= 0), nb, label)
    # an anchor belongs wholly to its region
    for k, reg in enumerate(regions["regions"]):
        if reg.get("anchor"):
            for a, mask, _, _ in anchors_f:
                if a["id"] == reg["anchor"]:
                    label[mask] = k
    return label, ids


def border_blend(label: np.ndarray, cells: int):
    """Secondary region and its weight within `cells` of a border: regions overlap there instead of meeting on a line."""
    nz, nx = label.shape
    second = np.full(label.shape, -1, np.int64)
    dist = np.zeros(label.shape, np.int64)
    p = np.pad(label, cells, constant_values=-1)
    for k in range(1, cells + 1):
        for dr in range(-k, k + 1):
            for dc in range(-k, k + 1):
                if max(abs(dr), abs(dc)) != k:
                    continue
                nb = p[cells + dr : cells + dr + nz, cells + dc : cells + dc + nx]
                hit = (nb >= 0) & (nb != label) & (label >= 0) & (second < 0)
                second = np.where(hit, nb, second)
                dist = np.where(hit, k, dist)
    weight = np.where(second >= 0, 0.5 * (1 - (dist - 1) / cells), 0.0)
    return second, weight


def region_adjacency(label, n, elev, big_river, land):
    nz, nx = label.shape
    shared = {}
    for dr, dc in ((0, 1), (1, 0)):
        a = label[: nz - dr, : nx - dc]
        b = label[dr:, dc:]
        m = (a >= 0) & (b >= 0) & (a != b)
        ea = elev[: nz - dr, : nx - dc][m]
        eb = elev[dr:, dc:][m]
        rv = (big_river[: nz - dr, : nx - dc] | big_river[dr:, dc:])[m]
        for x, y, e1, e2, river in zip(a[m].tolist(), b[m].tolist(), ea.tolist(), eb.tolist(), rv.tolist()):
            key = (min(x, y), max(x, y))
            s = shared.setdefault(key, [0, 0, 0.0, INF])
            s[0] += 1
            s[1] += river
            s[2] = max(s[2], max(e1, e2))
            s[3] = min(s[3], max(e1, e2))
    return shared


# ---------------------------------------------------------------- build


def snap(g: Grid, ok: np.ndarray, x: float, z: float) -> tuple[int, int]:
    """Nearest cell where `ok` holds, so a harbour town or a gate in a gorge still has a start cell."""
    r, c = g.index(x, z)
    for k in range(0, 40):
        win = ok[max(r - k, 0) : r + k + 1, max(c - k, 0) : c + k + 1]
        if win.any():
            rr, cc = np.argwhere(win)[0]
            return max(r - k, 0) + int(rr), max(c - k, 0) + int(cc)
    return r, c


def resolve_point(ref: str, layers, g: Grid):
    crown = layers["terrain"]["crown"]
    if ref.startswith("gate:"):
        gate = next(x for x in crown["gates"] if x["id"] == ref[5:])
        return gate_position(crown, gate)
    s = next(x for x in layers["polities"]["settlements"] if x["id"] == ref)
    return tuple(s["at"])


def build(layers: dict) -> dict:
    atlas = layers["atlas"]
    g = Grid(atlas)
    seed = atlas["seed"]
    e = build_elevation(g, layers, seed)
    elev, land = e["elev"], e["land"]
    profiles = carve_rivers(g, elev, land, layers["water"], seed)
    apply_cells(elev, layers["overrides/height"], add=True)
    anchors_f = anchor_fields(g, layers["anchors"])
    apply_anchors(elev, anchors_f)
    anchor_mask = np.zeros_like(land)
    for _, mask, _, _ in anchors_f:
        anchor_mask |= mask
    climate = layers["climate"]
    precip = precipitation(g, elev, land, climate)
    temp = temperature(g, elev, climate)
    hyd = hydrology(g, elev, land, layers["water"], anchor_mask, precip)
    lake_edge = hyd["lake"].copy()
    for _ in range(2):
        lake_edge = blur(lake_edge.astype(float), 1) > 0.01
    hyd["lake_edge"] = lake_edge & land & ~hyd["lake"]
    slope = slope_degrees(elev, g.cell)
    sink_mask = np.zeros_like(land)
    for sink in layers["water"]["sinks"]:
        if sink["kind"].startswith("endorheic"):
            r, c = hyd["sinks"][sink["id"]]
            sink_mask |= hyd["outlet"] == r * g.nx + c
            sink_mask &= elev < elev[r, c] + 45
    biome = classify_biomes(g, elev, land, hyd, temp, precip, slope, climate, anchor_mask, sink_mask)
    apply_cells(biome, layers["overrides/biome"], add=False)
    river = (hyd["acc"] > 2.5) & land
    big_river = (hyd["acc"] > 12.0) & land & ~anchor_mask
    regions = layers["regions"]
    label, ids = grow_regions(g, elev, land, hyd, anchors_f, regions, big_river)
    second, weight = border_blend(label, regions["blendCells"])
    adj = region_adjacency(label, len(ids), elev, big_river, land)
    out = {"grid": g, "elev": elev, "land": land, "landField": e["landField"], "profiles": profiles,
           "anchors": anchors_f, "anchor_mask": anchor_mask, "precip": precip, "temp": temp, "hyd": hyd,
           "slope": slope, "biome": biome, "river": river, "big_river": big_river, "label": label,
           "ids": ids, "second": second, "weight": weight, "adj": adj, "layers": layers}
    out["kingdom"], out["contested"], out["region_stats"] = claim_regions(out)
    out["roads"] = route_roads(out)
    return out


def claim_regions(b):
    layers, g = b["layers"], b["grid"]
    pol = layers["polities"]
    crown = layers["terrain"]["crown"]
    kingdoms = pol["kingdoms"]
    land, elev, label = b["land"], b["elev"], b["label"]
    extra = np.where(b["big_river"], 300.0, 0.0) + np.where(elev > 750, 200.0, 0.0)
    costs = []
    for k in kingdoms:
        # a kingdom holds land outward from its capital and its other settlements
        seats = [s["at"] for s in pol["settlements"] if s.get("kingdom") == k["id"]]
        seats.append(resolve_point(k["capital"], layers, g))
        sources = [(snap(g, land, *p), 0) for p in seats]
        dist, _, _, _ = grid_search(elev, land, extra, sources, k_abs=12.0, cell=g.cell)
        costs.append(unpad(dist, g.nz, g.nx))
    costs = np.stack(costs)
    n = len(b["ids"])
    reg = layers["regions"]["regions"]
    cx, cz = crown["center"]
    capitals = {int(label[snap(g, land, *resolve_point(k["capital"], layers, g))]): k["id"] for k in kingdoms}
    stats, kingdom, contested = [], [], []
    for k in range(n):
        m = label == k
        cells = int(m.sum())
        if cells == 0:
            stats.append(None)
            kingdom.append(None)
            contested.append(False)
            continue
        mean_e = float(elev[m].mean())
        mx, mz = float(b["grid"].X[m].mean()), float(b["grid"].Z[m].mean())
        rc = np.array([np.median(costs[i][m]) for i in range(len(kingdoms))])
        order = np.argsort(rc)
        pinned = layers["polities"]["claims"].get(b["ids"][k]) or capitals.get(k)
        inside = math.hypot(mx - cx, mz - cz) < crown["radius"]
        if pinned:
            owner = pinned
        elif mean_e > 760 or inside:
            owner = "crown"
        else:
            owner = kingdoms[order[0]]["id"]
        kingdom.append(owner)
        contested.append(bool(owner != "crown" and not pinned and rc[order[1]] < rc[order[0]] * 1.18))
        biomes, counts = np.unique(b["biome"][m], return_counts=True)
        top = biomes[np.argsort(-counts)][:2].tolist()
        stats.append({"cells": cells, "areaKm2": round(cells * g.cell * g.cell / 1e6, 2),
                      "centroid": [round(mx), round(mz)], "meanElevation": round(mean_e),
                      "maxElevation": round(float(elev[m].max())), "biomes": top,
                      "seed": reg[k]["seed"]})
    return kingdom, contested, stats


def route_roads(b):
    layers, g = b["layers"], b["grid"]
    elev, land = b["elev"], b["land"]
    road = np.zeros(land.shape)
    river_cost = np.where(b["big_river"], 250.0, 0.0) + np.where(b["river"], 40.0, 0.0)
    passable = land & ~b["hyd"]["lake"]
    out = []
    for route in layers["polities"]["routes"]:
        cells = []
        stops = [resolve_point(s, layers, g) for s in route["stops"]]
        for a, z in zip(stops, stops[1:]):
            ra, rb = snap(g, passable, *a), snap(g, passable, *z)
            extra = river_cost - np.where(road > 0, 25.0, 0.0)
            dist, _, prev, w = grid_search(elev, passable, extra, [(ra, 0)], k_abs=6.0, k_sq=120.0,
                                           target=rb, cell=g.cell)
            i = (rb[0] + 1) * w + rb[1] + 1
            path = []
            while i >= 0:
                path.append(divmod(i, w))
                i = prev[i]
            path = [(r - 1, c - 1) for r, c in reversed(path)]
            if path[0] != ra:
                path = []
            cells.append(path)
            for r, c in path:
                road[r, c] += 1
        out.append({"id": route["id"], "name": route["name"], "legs": cells})
    return out


# ---------------------------------------------------------------- checks


def run_checks(b) -> list[dict]:
    g, layers = b["grid"], b["layers"]
    hyd, land, elev = b["hyd"], b["land"], b["elev"]
    water = layers["water"]
    results = []

    def check(name, ok, detail):
        results.append({"check": name, "ok": bool(ok), "detail": detail})

    # coast: one continent plus declared islands
    lab, sizes = components(land)
    big = sorted(sizes, reverse=True)
    expected = 1 + len(layers["terrain"]["coast"]["islands"])
    major = [s for s in big if s * g.cell * g.cell / 1e6 > 0.25]
    check("coast.landmasses", len(major) == expected,
          f"{len(major)} landmasses over 0.25 km² (expected {expected}); largest {big[0] * g.cell**2 / 1e6:.0f} km²")
    land_km2 = land.sum() * g.cell**2 / 1e6
    check("scale.continent", 250 <= land_km2 <= 420 and land.any(),
          f"land {land_km2:.0f} km² in a {g.nx * g.cell / 1000:.0f} × {g.nz * g.cell / 1000:.0f} km domain")

    # rivers: strictly falling beds, mouths where declared, real catchments along them
    nx = g.nx
    rec = hyd["rec"].ravel()
    for river in water["rivers"]:
        prof = b["profiles"][river["id"]]
        bed = prof["bed"]
        falling = all(b2 < b1 for b1, b2 in zip(bed, bed[1:]))
        r, c = g.index(*river["points"][0])
        i, steps, path = r * nx + c, 0, []
        while rec[i] >= 0 and steps < 200000:
            path.append(i)
            i = rec[i]
            steps += 1
        end = divmod(i, nx)
        mouth = river["mouth"]
        if mouth == "sea":
            reached = not land[end]
        elif mouth.startswith("sink:"):
            reached = end == hyd["sinks"][mouth[5:]]
        else:
            other = b["profiles"][mouth[6:]]["samples"]
            pts = [divmod(p, nx) for p in path]
            reached = any(min(abs(r2 - rr) + abs(c2 - cc) for rr, cc in (g.index(x, z) for x, z in other[::2])) <= 3
                          for r2, c2 in pts[-400:])
        # the derived drainage runs along the authored course: a channel cell (≥0.15 km² effective
        # catchment) lies within two cells of each sample past the first tenth of its length
        acc = hyd["acc"]
        on = []
        for x, z in prof["samples"][len(prof["samples"]) // 10 :]:
            r0, c0 = g.index(x, z)
            on.append(acc[max(r0 - 2, 0) : r0 + 3, max(c0 - 2, 0) : c0 + 3].max() >= 0.15)
        frac = sum(on) / max(len(on), 1)
        mr, mc = [g.index(x, z) for x, z in prof["samples"] if land[g.index(x, z)]][-3]
        check(f"river.{river['id']}", falling and reached and frac >= 0.85,
              f"bed falls {bed[0]:.0f}→{bed[-1]:.0f} m over {prof['length'] / 1000:.1f} km; reaches {mouth}: {reached}; "
              f"drainage follows {frac:.0%} of its course; {acc[max(mr - 2, 0) : mr + 3, max(mc - 2, 0) : mc + 3].max():.0f} km² effective catchment near the mouth")

    # every land cell drains to the sea or a declared sink, never uphill
    out = hyd["outlet"]
    terminal_ok = (~land.ravel()[out.ravel()]) | np.isin(out.ravel(), [r * nx + c for r, c in hyd["sinks"].values()])
    check("water.drainage", bool(terminal_ok[land.ravel()].all()), "every land cell drains to the sea or a declared sink")
    f = hyd["filled"].ravel()
    valid = rec >= 0
    check("water.downhill", bool((f[rec[valid]] < f[valid]).all()), "each flow step descends on the depression-filled surface")
    for sink in water["sinks"]:
        if "resurgence" in sink:
            sr, sc = hyd["springs"][sink["resurgence"]]
            pr, pc = hyd["sinks"][sink["id"]]
            check(f"water.karst.{sink['id']}", elev[sr, sc] < elev[pr, pc],
                  f"resurgence {elev[sr, sc]:.0f} m lies below the ponor {elev[pr, pc]:.0f} m")
    for lake in water["lakes"]:
        r, c = g.index(*lake["at"])
        has = hyd["lake"][max(r - 10, 0) : r + 11, max(c - 10, 0) : c + 11].any()
        check(f"water.lake.{lake['id']}", has, "a lake forms there from the terrain")

    # anchors: the atlas leaves zone-authored ground at its stand-in elevation
    for a, mask, target, _ in b["anchors"]:
        check(f"anchor.{a['id']}.elevation", bool(np.allclose(elev[mask], target[mask])),
              f"{int(mask.sum())} cells keep the stand-in elevations {sorted({p['local'] for p in a['parts']})} m over {a['referenceElevation']} m")
        check(f"anchor.{a['id']}.dry", not hyd["lake"][mask].any() and not b["big_river"][mask].any(),
              "no generated lake or major river inside the zone footprint")
        rim = (b["anchors"][0][3] > 0) & (b["anchors"][0][3] < 400)
        step = np.abs(elev[rim] - target[rim]).max()
        check(f"anchor.{a['id']}.continuity", step < 140, f"surroundings within 400 m stay within {step:.0f} m of the anchor")

    # regions: ~150 of 2–4 km², every land cell in exactly one, contiguous
    label = b["label"]
    stats = [s for s in b["region_stats"] if s]
    areas = [s["areaKm2"] for s in stats]
    lo, hi = layers["regions"]["targetAreaKm2"]
    in_band = sum(1 for a in areas if lo * 0.6 <= a <= hi * 1.4) / len(areas)
    uncovered = np.argwhere(land & (label < 0))
    check("regions.cover", len(uncovered) == 0,
          f"{len(stats)} regions; {len(uncovered)} land cells uncovered"
          + (f", e.g. at {g.centre(*uncovered[0])}" if len(uncovered) else ""))
    check("regions.size", 120 <= len(stats) <= 180 and in_band >= 0.85,
          f"{len(stats)} regions; median {np.median(areas):.1f} km² (min {min(areas):.1f}, max {max(areas):.1f}); "
          f"{in_band:.0%} within 0.6×–1.4× of {lo}–{hi} km²")
    split = 0
    for k in range(len(b["ids"])):
        m = label == k
        if m.any():
            _, s = components(m)
            if len([x for x in s if x >= 3]) > 1:
                split += 1
    check("regions.contiguous", split == 0, f"{split} regions are split into separate pieces")

    # chunks stay internal: composed fields evaluated per streaming chunk match the whole-world evaluation
    chunk = 1024 // g.cell
    whole = compose(b, 0, 0, g.nz, g.nx)
    worst = 0.0
    for r0 in range(0, g.nz, chunk):
        for c0 in range(0, g.nx, chunk):
            part = compose(b, r0, c0, min(r0 + chunk, g.nz), min(c0 + chunk, g.nx))
            worst = max(worst, float(np.abs(part - whole[r0 : r0 + chunk, c0 : c0 + chunk]).max()))
    check("compose.chunks", worst == 0.0, f"per-chunk composition differs from the whole by at most {worst}")

    # polities
    for k in layers["polities"]["kingdoms"]:
        owned = [i for i, o in enumerate(b["kingdom"]) if o == k["id"]]
        area = sum(b["region_stats"][i]["areaKm2"] for i in owned)
        cap = resolve_point(k["capital"], layers, g)
        cap_region = label[snap(g, land, *cap)]
        crown_touch = any(
            (min(i, j), max(i, j)) in b["adj"]
            for i in owned
            for j, o in enumerate(b["kingdom"])
            if o == "crown"
        )
        check(f"kingdom.{k['id']}", cap_region in owned and crown_touch and len(owned) >= 8,
              f"{len(owned)} regions, {area:.0f} km²; capital inside: {cap_region in owned}; borders the Crown: {crown_touch}")
    for road in b["roads"]:
        check(f"road.{road['id']}", all(leg for leg in road["legs"]),
              f"{sum(len(leg) for leg in road['legs']) * g.cell / 1000:.1f} km routed over land")
    return results


def compose(b, r0, c0, r1, c1):
    """Composition of overlapping regions for one window: each cell's ground tint is its region's
    tint blended with the neighbour region's by the border weight. Reads only cells in the window."""
    label = b["label"][r0:r1, c0:c1]
    second = b["second"][r0:r1, c0:c1]
    w = b["weight"][r0:r1, c0:c1]
    tint = region_tint(np.maximum(label, 0))
    other = region_tint(np.maximum(second, 0))
    return np.where(second >= 0, tint * (1 - w) + other * w, tint)


def region_tint(k):
    return (lattice_hash(k, k * 0 + 3, 991) + 1) / 2


def fingerprint(b) -> str:
    h = hashlib.sha256()
    for key in ("elev", "precip", "temp"):
        h.update(np.round(b[key] * 10).astype(np.int64).tobytes())
    for key in ("biome", "label", "second"):
        h.update(b[key].astype(np.int64).tobytes())
    h.update(np.round(b["hyd"]["acc"] * 100).astype(np.int64).tobytes())
    h.update(json.dumps(b["kingdom"]).encode())
    h.update(json.dumps([[leg for leg in r["legs"]] for r in b["roads"]]).encode())
    return h.hexdigest()[:16]


# ---------------------------------------------------------------- outputs


def hex_rgb(s):
    return tuple(int(s[i : i + 2], 16) for i in (1, 3, 5))


def hillshade(elev, cell, az=315, alt=45):
    gz, gx = np.gradient(elev * 2.2, cell)
    slope = np.arctan(np.hypot(gx, gz))
    aspect = np.arctan2(-gx, gz)
    a, z = math.radians(az), math.radians(90 - alt)
    shade = np.cos(z) * np.cos(slope) + np.sin(z) * np.sin(slope) * np.cos(a - aspect)
    return np.clip(shade, 0, 1)


HYPSO = [(-80, (31, 66, 94)), (-1, (58, 104, 138)), (0, (122, 156, 104)), (120, (150, 172, 110)),
         (300, (196, 190, 132)), (500, (178, 150, 104)), (750, (150, 124, 100)), (1000, (190, 184, 178)),
         (1300, (246, 246, 246))]


def hypsometric(elev):
    xs = [h for h, _ in HYPSO]
    rgb = np.zeros(elev.shape + (3,))
    for ch in range(3):
        rgb[..., ch] = np.interp(elev, xs, [c[ch] for _, c in HYPSO])
    return rgb


def to_image(rgb, scale=2):
    img = Image.fromarray(np.clip(rgb, 0, 255).astype(np.uint8), "RGB")
    return img.resize((img.width * scale, img.height * scale), Image.Resampling.BILINEAR)


def font(size):
    for path in ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf"):
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default(size)


def font_bold(size):
    for path in ("/usr/share/fonts/truetype/dejavu/DejaVuSerif-Bold.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"):
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    return font(size)


class Canvas:
    def __init__(self, b, rgb, scale=2):
        self.b, self.g, self.s = b, b["grid"], scale
        self.img = to_image(rgb, scale)
        self.d = ImageDraw.Draw(self.img, "RGBA")

    def px(self, x, z):
        return ((x - self.g.min_x) / self.g.cell * self.s, (z - self.g.min_z) / self.g.cell * self.s)

    def rc(self, r, c):
        return ((c + 0.5) * self.s, (r + 0.5) * self.s)

    def label(self, x, z, text, size=13, fill=(30, 26, 22), halo=(250, 246, 236), bold=False, anchor="mm"):
        f = font_bold(size) if bold else font(size)
        self.d.text(self.px(x, z), text, font=f, fill=fill, anchor=anchor, stroke_width=3, stroke_fill=halo)

    def rivers(self, colour=(52, 98, 150), threshold=2.5):
        hyd, g = self.b["hyd"], self.g
        acc = hyd["acc"]
        rec = hyd["rec"].ravel()
        land = self.b["land"].ravel()
        for i in np.nonzero((acc.ravel() > threshold) & land)[0].tolist():
            j = rec[i]
            if j < 0:
                continue
            width = max(1, min(6, int(0.7 * math.log(acc.ravel()[i] / threshold + 1) * self.s)))
            a, bb = self.rc(*divmod(i, g.nx)), self.rc(*divmod(j, g.nx))
            self.d.line([a, bb], fill=colour + (255,), width=width)
        lake = hyd["lake"]
        self.overlay(lake, (79, 134, 168, 255))

    def overlay(self, mask, rgba):
        layer = np.zeros(mask.shape + (4,), np.uint8)
        layer[mask] = rgba
        im = Image.fromarray(layer, "RGBA").resize(self.img.size, Image.Resampling.NEAREST)
        self.img.paste(im, (0, 0), im)
        self.d = ImageDraw.Draw(self.img, "RGBA")

    def borders(self, label, rgba, width=1, between=None):
        nz, nx = label.shape
        edge = np.zeros(label.shape, bool)
        for dr, dc in ((0, 1), (1, 0)):
            a, b2 = label[: nz - dr, : nx - dc], label[dr:, dc:]
            m = (a != b2) & (a >= 0) & (b2 >= 0)
            if between is not None:
                m &= between(a, b2)
            edge[: nz - dr, : nx - dc] |= m
        ys, xs = np.nonzero(edge)
        for r, c in zip(ys.tolist(), xs.tolist()):
            x, y = self.rc(r, c)
            self.d.ellipse([x - width, y - width, x + width, y + width], fill=rgba)

    def anchor_outline(self, colour=(210, 60, 45, 255)):
        for a, *_ in self.b["anchors"]:
            for p in a["parts"]:
                x0, z0, x1, z1 = p["rect"]
                self.d.rectangle([*self.px(x0, z0), *self.px(x1, z1)], outline=colour, width=2)

    def scalebar(self, km=5):
        w = km * 1000 / self.g.cell * self.s
        x0, y0 = 30, self.img.height - 40
        self.d.rectangle([x0 - 6, y0 - 22, x0 + w + 40, y0 + 14], fill=(250, 246, 236, 220))
        for i in range(km):
            self.d.rectangle([x0 + i * w / km, y0, x0 + (i + 1) * w / km, y0 + 6],
                             fill=(30, 30, 30) if i % 2 == 0 else (250, 250, 250), outline=(30, 30, 30))
        self.d.text((x0, y0 - 4), "0", font=font(12), fill=(30, 30, 30), anchor="mb")
        self.d.text((x0 + w, y0 - 4), f"{km} km", font=font(12), fill=(30, 30, 30), anchor="mb")
        nx0, ny0 = self.img.width - 40, 40
        self.d.polygon([(nx0, ny0 - 22), (nx0 - 9, ny0 + 6), (nx0, ny0), (nx0 + 9, ny0 + 6)], fill=(30, 30, 30))
        self.d.text((nx0, ny0 + 10), "N", font=font_bold(14), fill=(30, 30, 30), anchor="mt")

    def title(self, text, sub=None):
        self.d.rectangle([0, 0, self.img.width, 34 + (18 if sub else 0)], fill=(250, 246, 236, 230))
        self.d.text((14, 8), text, font=font_bold(18), fill=(30, 26, 22))
        if sub:
            self.d.text((14, 33), sub, font=font(12), fill=(80, 70, 60))

    def legend(self, items, x=None, y=60):
        x = self.img.width - 250 if x is None else x
        h = 20 * len(items) + 12
        self.d.rectangle([x - 8, y - 8, x + 236, y + h], fill=(250, 246, 236, 225))
        for i, (rgb, text) in enumerate(items):
            self.d.rectangle([x, y + i * 20, x + 16, y + i * 20 + 14], fill=rgb, outline=(60, 60, 60))
            self.d.text((x + 24, y + i * 20 + 7), text, font=font(12), fill=(30, 26, 22), anchor="lm")

    def save(self, path):
        self.img.save(path, optimize=True)


def relief_base(b, tint):
    shade = hillshade(np.maximum(b["elev"], 0), b["grid"].cell)[..., None]
    base = tint * (0.55 + 0.6 * shade)
    sea = ~b["land"]
    base[sea] = hypsometric(b["elev"])[sea] * (0.95 + 0.05 * shade[sea])
    return base


def kingdom_colour_map(b):
    pol = b["layers"]["polities"]
    colours = {k["id"]: hex_rgb(k["color"]) for k in pol["kingdoms"]}
    colours["crown"] = (150, 140, 150)
    lut = np.array([colours.get(o, (0, 0, 0)) if o else (0, 0, 0) for o in b["kingdom"]] + [(0, 0, 0)], float)
    return lut


def render(b, out: Path, previews: Path | None):
    out.mkdir(parents=True, exist_ok=True)
    g, layers = b["grid"], b["layers"]
    crown = layers["terrain"]["crown"]
    pol = layers["polities"]
    files = {}

    # elevation
    c = Canvas(b, relief_base(b, hypsometric(b["elev"])))
    c.rivers(threshold=6)
    c.anchor_outline()
    for el, name in ((0, "sea"), (300, "300 m"), (600, "600 m"), (900, "900 m"), (1200, "1200 m")):
        pass
    c.title("Elevation", "hypsometric tint over hillshade; Greyhaven zone outlined in red")
    c.legend([(tuple(int(v) for v in rgb), f"{h} m") for h, rgb in HYPSO[1:]], y=70)
    c.scalebar()
    files["elevation"] = c

    # hydrology: drainage basins tinted, rivers, lakes, sinks
    outlet = b["hyd"]["outlet"]
    uniq, inv, counts = np.unique(outlet, return_inverse=True, return_counts=True)
    big_basins = counts * g.cell**2 / 1e6 > 8
    tint_id = np.where(big_basins[inv.reshape(outlet.shape)], inv.reshape(outlet.shape), -1)
    pal = (lattice_hash(np.arange(len(uniq)), np.zeros(len(uniq), int), 5)[:, None] + 1) / 2
    basin_rgb = np.where(tint_id[..., None] >= 0, 150 + 90 * np.concatenate(
        [pal, (lattice_hash(np.arange(len(uniq)), np.ones(len(uniq), int), 6)[:, None] + 1) / 2,
         np.full((len(uniq), 1), 0.7)], axis=1)[np.maximum(tint_id, 0)], 205)
    c = Canvas(b, relief_base(b, basin_rgb))
    c.rivers(colour=(30, 80, 160), threshold=1.2)
    for river in layers["water"]["rivers"]:
        x, z = river["points"][len(river["points"]) // 2]
        c.label(x, z, river["name"], size=13, fill=(20, 60, 130))
    for sink in layers["water"]["sinks"]:
        x, z = sink["at"]
        px, py = c.px(x, z)
        c.d.ellipse([px - 6, py - 6, px + 6, py + 6], outline=(160, 40, 30), width=3)
        c.label(x, z + 260, sink["name"], size=12, fill=(160, 40, 30))
    for s in layers["water"]["springs"]:
        px, py = c.px(*s["at"])
        c.d.rectangle([px - 4, py - 4, px + 4, py + 4], fill=(30, 80, 160))
    for lake in layers["water"]["lakes"]:
        c.label(lake["at"][0], lake["at"][1], lake["name"], size=12, fill=(20, 60, 130))
    c.anchor_outline()
    c.title("Hydrology", "drainage basins over 8 km² tinted; rivers from flow accumulation; red rings: declared sinks")
    c.scalebar()
    files["hydrology"] = c

    # biomes
    lut = np.array([hex_rgb(x["color"]) for x in sorted(layers["climate"]["biomes"], key=lambda x: x["id"])], float)
    c = Canvas(b, relief_base(b, lut[np.maximum(b["biome"], 0)]))
    c.rivers(threshold=6)
    c.title("Biomes", "temperature × precipitation × slope, westerly rain shadow; red: zone-authored Greyhaven")
    c.legend([(hex_rgb(x["color"]), x["name"]) for x in layers["climate"]["biomes"]], y=64)
    c.scalebar()
    files["biomes"] = c

    # climate
    p = b["precip"]
    t = b["temp"]
    rgb = np.stack([np.interp(t, [-4, 6, 18], [90, 200, 240]), np.interp(p, [0, 0.5, 1], [190, 200, 120]),
                    np.interp(p, [0, 1], [110, 230])], axis=-1)
    c = Canvas(b, relief_base(b, rgb))
    c.title("Climate", "blue: wet (westerlies rain out on the Crown's west flank), red: warm; dry east is the rain shadow")
    c.scalebar()
    files["climate"] = c

    # natural regions with soft borders
    tint = region_tint(np.maximum(b["label"], 0))
    tint2 = region_tint(np.maximum(b["second"], 0))
    mix = np.where(b["second"] >= 0, tint * (1 - b["weight"]) + tint2 * b["weight"], tint)
    klut = kingdom_colour_map(b)
    kc = klut[np.where(b["label"] >= 0, b["label"], len(b["kingdom"]))]
    kc2 = klut[np.where(b["second"] >= 0, b["second"], len(b["kingdom"]))]
    kmix = np.where((b["second"] >= 0)[..., None], kc * (1 - b["weight"][..., None]) + kc2 * b["weight"][..., None], kc)
    regions_rgb = kmix * (0.75 + 0.35 * mix[..., None])
    c = Canvas(b, relief_base(b, regions_rgb))
    c.rivers(threshold=6)
    c.borders(b["label"], (40, 30, 30, 120), width=0.6)
    for k, s in enumerate(b["region_stats"]):
        if s:
            name = layers["regions"]["regions"][k].get("name")
            c.label(*s["centroid"], name or b["ids"][k], size=11 if name else 9, fill=(30, 26, 22), bold=bool(name))
    c.anchor_outline()
    c.title("Natural regions", f"{sum(1 for s in b['region_stats'] if s)} regions grown from seeds over the terrain; borders blend over "
            f"{layers['regions']['blendCells'] * g.cell} m; tinted by claim")
    c.scalebar()
    files["regions"] = c

    # composition demonstrator: chunk grid is internal and unrelated to region borders
    c = Canvas(b, relief_base(b, (compose(b, 0, 0, g.nz, g.nx)[..., None] * 120 + 110) * np.array([1.0, 0.95, 0.85])))
    chunk_px = 1024 / g.cell * c.s
    xk = 0.0
    while xk < c.img.width:
        c.d.line([(xk, 0), (xk, c.img.height)], fill=(20, 20, 20, 70), width=1)
        xk += chunk_px
    yk = 0.0
    while yk < c.img.height:
        c.d.line([(0, yk), (c.img.width, yk)], fill=(20, 20, 20, 70), width=1)
        yk += chunk_px
    c.borders(b["label"], (120, 20, 20, 160), width=0.6)
    c.title("Region composition vs streaming chunks",
            "regions overlap and blend across natural borders (red); the 1024 m chunk grid (grey) is internal and evaluates identically")
    files["composition"] = c

    # the readable world map
    files["world-map"] = world_map(b)
    files["greyhaven-context"] = greyhaven_context(b)

    for name, canvas in files.items():
        canvas.save(out / f"{name}.png")
        if previews is not None:
            previews.mkdir(parents=True, exist_ok=True)
            img = canvas.img.convert("RGB")
            if name == "world-map":
                # the map's legend and kingdom colours must survive, so it keeps full colour as a JPEG
                img.save(previews / f"{name}.jpg", quality=86, optimize=True)
                continue
            if name != "greyhaven-context":
                img = img.resize((img.width * 3 // 4, img.height * 3 // 4), Image.Resampling.LANCZOS)
            img.quantize(colors=192, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(
                previews / f"{name}.png", optimize=True)
    return sorted(files)


def world_map(b):
    g, layers = b["grid"], b["layers"]
    pol = layers["polities"]
    crown = layers["terrain"]["crown"]
    # muted biome-tinted relief with kingdom washes
    lut = np.array([hex_rgb(x["color"]) for x in sorted(layers["climate"]["biomes"], key=lambda x: x["id"])], float)
    bio = lut[np.maximum(b["biome"], 0)]
    paper = np.array([238, 230, 210], float)
    base = bio * 0.55 + paper * 0.45
    klut = kingdom_colour_map(b)
    kc = klut[np.where(b["label"] >= 0, b["label"], len(b["kingdom"]))]
    base = np.where(b["land"][..., None], base * 0.78 + kc * 0.22, base)
    c = Canvas(b, relief_base(b, base))
    c.rivers(colour=(52, 98, 150), threshold=3)
    # region borders faint, kingdom borders strong
    owner_idx = {o: i for i, o in enumerate(sorted({o for o in b["kingdom"] if o}))}
    kid = np.array([owner_idx.get(o, -1) for o in b["kingdom"]] + [-1])
    kingdom_label = kid[np.where(b["label"] >= 0, b["label"], len(b["kingdom"]))]
    c.borders(b["label"], (90, 70, 60, 55), width=0.5)
    c.borders(kingdom_label, (70, 30, 30, 230), width=1.6)
    # contested regions hatched
    hatch = np.zeros(b["label"].shape, bool)
    rr, cc = np.indices(hatch.shape)
    stripes = ((rr + cc) % 8) < 2
    for k, flag in enumerate(b["contested"]):
        if flag:
            hatch |= (b["label"] == k) & stripes
    c.overlay(hatch, (120, 30, 30, 70))
    # roads
    for road in b["roads"]:
        for leg in road["legs"]:
            pts = [c.rc(r, cc2) for r, cc2 in leg[::2]] + ([c.rc(*leg[-1])] if leg else [])
            if len(pts) > 1:
                c.d.line(pts, fill=(250, 240, 220, 220), width=5)
                c.d.line(pts, fill=(120, 70, 40, 255), width=2)
    # crown gates
    for gate in crown["gates"]:
        x, z = gate_position(crown, gate)
        px, py = c.px(x, z)
        c.d.polygon([(px, py - 8), (px + 7, py + 6), (px - 7, py + 6)], fill=(60, 50, 50), outline=(250, 246, 236))
        lx, lz = gate_position(crown, gate, crown["radius"] + 900)
        c.label(lx, lz, gate["name"], size=12, fill=(60, 40, 40), bold=True)
    # settlements
    for s in pol["settlements"]:
        x, z = s["at"]
        px, py = c.px(x, z)
        if s["kind"] == "capital":
            c.d.rectangle([px - 7, py - 7, px + 7, py + 7], fill=(140, 30, 30), outline=(250, 246, 236), width=2)
            c.label(x, z - 330, s["name"], size=15, bold=True)
        elif s["kind"] == "ruin":
            c.d.ellipse([px - 6, py - 6, px + 6, py + 6], outline=(60, 50, 50), width=2)
            c.label(x, z - 300, s["name"] + " (ruins)", size=12, fill=(70, 60, 60))
        elif s.get("anchor"):
            continue
        else:
            c.d.ellipse([px - 5, py - 5, px + 5, py + 5], fill=(40, 30, 30), outline=(250, 246, 236), width=2)
            if s["kind"] == "fort":
                c.label(x + 160, z + 120, s["name"], size=12, anchor="lm")
            else:
                c.label(x, z - 280, s["name"], size=13)
    c.anchor_outline(colour=(200, 40, 30, 255))
    c.label(900, 380, "Greyhaven", size=15, fill=(170, 30, 20), bold=True, anchor="lm")
    # kingdom names at the area-weighted centre of their regions
    for k in pol["kingdoms"]:
        owned = [i for i, o in enumerate(b["kingdom"]) if o == k["id"]]
        w = np.array([b["region_stats"][i]["areaKm2"] for i in owned])
        xy = np.array([b["region_stats"][i]["centroid"] for i in owned], float)
        x, z = (xy * w[:, None]).sum(0) / w.sum()
        # step off any settlement or landform label the name would cover
        others = [s["at"] for s in pol["settlements"]] + [r["points"][len(r["points"]) // 2] for r in layers["terrain"]["ridges"]]
        for _ in range(6):
            if all(abs(x - ox) > 2000 or abs(z - oz) > 450 for ox, oz in others):
                break
            z += 500
        c.label(x, z, k["name"].upper(), size=24, fill=tuple(int(v * 0.55) for v in hex_rgb(k["color"])), bold=True)
    c.label(crown["center"][0], crown["center"][1] + 1500, "THE SHATTERED CROWN", size=20, fill=(60, 50, 60), bold=True)
    for river in layers["water"]["rivers"]:
        pts = river["points"]
        x, z = [(a + bb) / 2 for a, bb in zip(pts[len(pts) // 2 - 1], pts[len(pts) // 2])]
        c.label(x + 250, z, river["name"], size=12, fill=(30, 70, 140), anchor="lm")
    for lake in layers["water"]["lakes"]:
        c.label(lake["at"][0], lake["at"][1], lake["name"], size=12, fill=(30, 70, 140))
    for sink in layers["water"]["sinks"]:
        if sink["kind"].startswith("endorheic"):
            c.label(sink["at"][0], sink["at"][1] + 500, sink["name"], size=13, fill=(110, 90, 60), bold=True)
    for ridge in layers["terrain"]["ridges"]:
        x, z = ridge["points"][len(ridge["points"]) // 2]
        c.label(x, z, ridge["name"], size=12, fill=(90, 70, 50))
    c.title("The continent (provisional atlas)",
            "kingdom borders dark red; contested border regions hatched; roads brown; ▲ Crown gates; ■ capitals")
    c.legend([(hex_rgb(k["color"]), k["name"]) for k in pol["kingdoms"]] + [((150, 140, 150), "Shattered Crown (unclaimed)")],
             x=30, y=c.img.height - 210)
    c.scalebar()
    return c


def greyhaven_context(b):
    """A 6 × 6 km window around Greyhaven at 6 px per cell."""
    g = b["grid"]
    half = 3000
    r0, c0 = g.index(-half, -half - 1200)
    r1, c1 = g.index(half, half - 1200)
    sub = {k: b[k][r0:r1, c0:c1] for k in ("elev", "land", "biome", "label")}
    lut = np.array([hex_rgb(x["color"]) for x in sorted(b["layers"]["climate"]["biomes"], key=lambda x: x["id"])], float)
    shade = hillshade(np.maximum(sub["elev"], 0), g.cell)[..., None]
    rgb = (lut[np.maximum(sub["biome"], 0)] * 0.6 + hypsometric(sub["elev"]) * 0.4) * (0.55 + 0.6 * shade)
    s = 6
    img = to_image(rgb, s)
    d = ImageDraw.Draw(img, "RGBA")
    ox, oz = g.min_x + c0 * g.cell, g.min_z + r0 * g.cell

    def px(x, z):
        return ((x - ox) / g.cell * s, (z - oz) / g.cell * s)

    hyd = b["hyd"]
    rec = hyd["rec"]
    for r in range(r0, r1):
        for cc in range(c0, c1):
            a = hyd["acc"][r, cc]
            if a > 0.6 and b["land"][r, cc] and rec[r, cc] >= 0:
                rr, c2 = divmod(int(rec[r, cc]), g.nx)
                d.line([((cc - c0 + 0.5) * s, (r - r0 + 0.5) * s), ((c2 - c0 + 0.5) * s, (rr - r0 + 0.5) * s)],
                       fill=(40, 90, 160, 255), width=max(1, min(7, int(1.5 * math.log(a / 0.6 + 1)))))
    for road in b["roads"]:
        for leg in road["legs"]:
            pts = [((cc - c0 + 0.5) * s, (r - r0 + 0.5) * s) for r, cc in leg if r0 <= r < r1 and c0 <= cc < c1]
            if len(pts) > 1:
                d.line(pts, fill=(120, 70, 40, 255), width=3)
    lab = sub["label"]
    nz, nx = lab.shape
    for dr, dc in ((0, 1), (1, 0)):
        m = (lab[: nz - dr, : nx - dc] != lab[dr:, dc:])
        for r, cc in zip(*np.nonzero(m)):
            d.rectangle([cc * s + 2, r * s + 2, cc * s + 4, r * s + 4], fill=(60, 30, 30, 140))
    for a, *_ in b["anchors"]:
        for p in a["parts"]:
            x0, z0, x1, z1 = p["rect"]
            d.rectangle([*px(x0, z0), *px(x1, z1)], outline=(210, 40, 30, 255), width=3)
        for e in a["entrances"]:
            ex, ey = px(*e["at"])
            col = (210, 40, 30) if e["established"] else (250, 240, 220)
            d.ellipse([ex - 6, ey - 6, ex + 6, ey + 6], fill=col, outline=(30, 30, 30), width=2)
            d.text((ex + 10, ey), e["name"], font=font(13), fill=(30, 26, 22), anchor="lm", stroke_width=3, stroke_fill=(250, 246, 236))
    sink = next(x for x in b["layers"]["water"]["sinks"] if x["id"] == "greyhaven-ponor")
    sx, sy = px(*sink["at"])
    d.ellipse([sx - 8, sy - 8, sx + 8, sy + 8], outline=(30, 80, 160), width=3)
    for sp in b["layers"]["water"]["springs"]:
        qx, qy = px(*sp["at"])
        d.rectangle([qx - 6, qy - 6, qx + 6, qy + 6], fill=(30, 80, 160))
        d.text((qx + 10, qy), sp["name"] + " (resurgence)", font=font(13), fill=(20, 60, 130), anchor="lm", stroke_width=3, stroke_fill=(250, 246, 236))
    crown = b["layers"]["terrain"]["crown"]
    gate = next(x for x in crown["gates"] if x["id"] == "greyfang-gate")
    gx_, gy_ = px(*gate_position(crown, gate))
    d.polygon([(gx_, gy_ - 10), (gx_ + 9, gy_ + 7), (gx_ - 9, gy_ + 7)], fill=(60, 50, 50))
    d.text((gx_ + 12, gy_), "Greyfang Gate", font=font_bold(14), fill=(60, 40, 40), anchor="lm", stroke_width=3, stroke_fill=(250, 246, 236))
    for st in b["layers"]["polities"]["settlements"]:
        x, z = st["at"]
        qx, qy = px(x, z)
        if 0 <= qx < img.width and 0 <= qy < img.height and not st.get("anchor"):
            d.ellipse([qx - 6, qy - 6, qx + 6, qy + 6], fill=(40, 30, 30))
            d.text((qx + 10, qy), st["name"], font=font_bold(14), fill=(30, 26, 22), anchor="lm", stroke_width=3, stroke_fill=(250, 246, 236))
    c = Canvas.__new__(Canvas)
    c.img, c.d, c.b, c.g, c.s = img, d, b, g, s
    c.title("Greyhaven in its surroundings (6 × 6 km)",
            "red: zone 1 footprint (authored, untouched); red dots: established passes; pale dots: future exits; blue ring: Greyhaven Sink")
    return c


def write_report(b, out: Path, checks, fp):
    g, layers = b["grid"], b["layers"]
    regions = []
    for k, s in enumerate(b["region_stats"]):
        if not s:
            continue
        reg = layers["regions"]["regions"][k]
        neighbours = []
        for (i, j), (n, river, hi, lo) in sorted(b["adj"].items()):
            if k in (i, j) and n >= 3:
                other = j if i == k else i
                kind = "river" if river > n * 0.5 else ("pass" if lo > 700 else ("ridge" if hi - s["meanElevation"] > 150 else "open"))
                neighbours.append({"region": b["ids"][other], "borderKm": round(n * g.cell / 1000, 2), "crossing": kind})
        regions.append({"id": b["ids"][k], "name": reg.get("name"), "kingdom": b["kingdom"][k],
                        "contested": b["contested"][k], **s,
                        "biomes": [next(x["key"] for x in layers["climate"]["biomes"] if x["id"] == bb) for bb in s["biomes"]],
                        "neighbours": neighbours})
    report = {"source": source_digest(), "fingerprint": fp, "checks": checks, "regions": regions,
              "roads": [{"id": r["id"], "name": r["name"], "km": round(sum(len(l) for l in r["legs"]) * g.cell / 1000, 1)} for r in b["roads"]]}
    (out / "atlas-report.json").write_text(json.dumps(report, indent=1) + "\n")
    lines = ["| Region | Kingdom | km² | Elev. m | Biomes | Neighbours (crossing) |", "| --- | --- | --- | --- | --- | --- |"]
    for r in regions:
        nb = ", ".join(f"{n['region']} ({n['crossing']})" for n in r["neighbours"])
        lines.append(f"| {r['name'] or r['id']} | {r['kingdom']}{' (contested)' if r['contested'] else ''} | {r['areaKm2']} | "
                     f"{r['meanElevation']} | {', '.join(r['biomes'])} | {nb} |")
    (out / "adjacency.md").write_text("\n".join(lines) + "\n")
    return report


# ---------------------------------------------------------------- commands


def cmd_build(args, check=False) -> int:
    before = source_digest()
    greyhaven_before = digest_tree(REPO / "content" / "greyhaven")
    layers = load_layers()
    b = build(layers)
    fp = fingerprint(b)
    checks = run_checks(b) if (check or args.checks) else []
    if check:
        again = fingerprint(build(load_layers()))
        checks.append({"check": "reproducible", "ok": again == fp, "detail": f"two builds give {fp} and {again}"})
    checks.append({"check": "sources.untouched", "ok": source_digest() == before, "detail": "content/world/ unchanged by the build"})
    checks.append({"check": "greyhaven.untouched", "ok": digest_tree(REPO / "content" / "greyhaven") == greyhaven_before,
                   "detail": "content/greyhaven/ unchanged by the build"})
    out = Path(args.out)
    rendered = render(b, out, Path(args.previews) if args.previews else None)
    # reference terrain for the Blender adapter (atlas_blender.py); never read back
    np.save(out / "elevation.npy", b["elev"].astype(np.float32))
    (out / "elevation.json").write_text(json.dumps({"minX": b["grid"].min_x, "minZ": b["grid"].min_z,
                                                    "cell": b["grid"].cell, "rows": b["grid"].nz, "columns": b["grid"].nx}) + "\n")
    report = write_report(b, out, checks, fp)
    for c in checks:
        print(("ok   " if c["ok"] else "FAIL ") + f"{c['check']}: {c['detail']}")
    print(f"fingerprint {fp}; {len(report['regions'])} regions; wrote {', '.join(rendered)} to {out}")
    return 0 if all(c["ok"] for c in checks) else 1


def digest_tree(path: Path) -> str:
    h = hashlib.sha256()
    for p in sorted(path.rglob("*")):
        if p.is_file() and "__pycache__" not in p.parts:
            h.update(p.relative_to(path).as_posix().encode())
            h.update(p.read_bytes())
    return h.hexdigest()


def cmd_seed_regions(args) -> int:
    layers = load_layers()
    g = Grid(layers["atlas"])
    e = build_elevation(g, layers, layers["atlas"]["seed"])
    anchors_f = anchor_fields(g, layers["anchors"])
    mask = np.zeros_like(e["land"])
    for _, m, _, _ in anchors_f:
        mask |= m
    regions = layers["regions"]
    new = region_seed_candidates(g, e["land"], mask, regions, layers["atlas"]["seed"])
    taken = {r["id"] for r in regions["regions"]}
    n = 1
    for x, z in new:
        while f"r{n:03d}" in taken:
            n += 1
        regions["regions"].append({"id": f"r{n:03d}", "seed": [x, z]})
        taken.add(f"r{n:03d}")
    write_json(WORLD / "regions.json", regions)
    print(f"appended {len(new)} region seeds; {len(regions['regions'])} in total")
    return 0


def cmd_format(args) -> int:
    """Rewrite every source layer in the canonical layout; values never change."""
    changed = [p.relative_to(WORLD).as_posix() for p in sorted(WORLD.rglob("*.json"))
               if write_json(p, json.loads(p.read_text()))]
    print("reformatted: " + (", ".join(changed) if changed else "nothing"))
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    for name in ("build", "check"):
        s = sub.add_parser(name)
        s.add_argument("--out", default=str(OUT))
        s.add_argument("--previews", default=None, help="also write compact previews here, e.g. docs/images/world")
        s.add_argument("--checks", action="store_true", help="run the consistency checks during build")
    sub.add_parser("seed-regions")
    sub.add_parser("format")
    args = p.parse_args()
    if args.cmd == "seed-regions":
        return cmd_seed_regions(args)
    if args.cmd == "format":
        return cmd_format(args)
    return cmd_build(args, check=args.cmd == "check")


if __name__ == "__main__":
    sys.exit(main())
