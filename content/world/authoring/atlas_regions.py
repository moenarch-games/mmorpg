"""Regions as the pieces that borders cut the land into, and edits to the border network. Shared by
atlas.py and the Blender adapter, so a region is the same piece of land wherever it is computed.
Needs only numpy (Blender bundles it).

A grid here is anything with min_x, min_z, cell, nx, nz, index(x, z) and centre(r, c): atlas.py's
Grid, or Frame below for a saved raster."""

import math

import numpy as np

NB8 = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]


class Frame:
    def __init__(self, min_x, min_z, cell, nx, nz):
        self.min_x, self.min_z, self.cell, self.nx, self.nz = min_x, min_z, cell, nx, nz

    def index(self, x, z):
        r = int((z - self.min_z) // self.cell)
        c = int((x - self.min_x) // self.cell)
        return min(max(r, 0), self.nz - 1), min(max(c, 0), self.nx - 1)

    def centre(self, r, c):
        return self.min_x + (c + 0.5) * self.cell, self.min_z + (r + 0.5) * self.cell


def components(mask):
    """4-connected components: a label per cell (-1 outside the mask) and each component's size."""
    nz, nx = mask.shape
    m = mask.tolist()
    labl = np.full(mask.shape, -1, np.int64).tolist()
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


def snap(g, ok, x, z):
    """Nearest cell where `ok` holds, so a harbour town or a gate in a gorge still has a start cell."""
    r, c = g.index(x, z)
    for k in range(0, 40):
        win = ok[max(r - k, 0) : r + k + 1, max(c - k, 0) : c + k + 1]
        if win.any():
            rr, cc = np.argwhere(win)[0]
            return max(r - k, 0) + int(rr), max(c - k, 0) + int(cc)
    return r, c


def walls(g, borders):
    """Cells the borders pass through. An 8-connected wall stops a 4-connected fill."""
    wall = np.zeros((g.nz, g.nx), bool)
    for b in borders:
        for (ax, az), (bx, bz) in zip(b["points"], b["points"][1:]):
            t = np.linspace(0.0, 1.0, max(2, int(math.hypot(bx - ax, bz - az) / (g.cell / 3)) + 1))
            cols = np.floor((ax + (bx - ax) * t - g.min_x) / g.cell).astype(np.int64)
            rows = np.floor((az + (bz - az) * t - g.min_z) / g.cell).astype(np.int64)
            ok = (rows >= 0) & (rows < g.nz) & (cols >= 0) & (cols < g.nx)
            wall[rows[ok], cols[ok]] = True
    return wall


def pieces(g, land, wall, seeds, sliver=12):
    """Label each land cell with the index of the seed whose piece it lies in. Border cells and
    seedless slivers join a neighbouring piece. Returns the labels and any problems found."""
    open_ = land & ~wall
    piece, sizes = components(open_)
    owners = {}
    for k, (rid, at) in enumerate(seeds):
        r, c = snap(g, open_, *at)
        owners.setdefault(int(piece[r, c]), []).append(k)
    lut = np.full(len(sizes) + 1, -1, np.int64)
    problems = []
    for f, ks in sorted(owners.items()):
        if f < 0:
            problems.append(f"seed of {', '.join(seeds[k][0] for k in ks)} is not on land")
            continue
        lut[f] = ks[0]
        if len(ks) > 1:
            problems.append(f"{' and '.join(seeds[k][0] for k in ks)} share one piece of land: draw a border between them")
    slivers = 0
    for f, n in enumerate(sizes):
        if lut[f] < 0:
            if n > sliver:
                rr, cc = np.argwhere(piece == f)[0]
                x, z = g.centre(rr, cc)
                problems.append(f"{n * g.cell**2 / 1e6:.2f} km² around ({x:.0f}, {z:.0f}) has no seed: add one, or remove the border that cuts it off")
            else:
                slivers += 1
    label = lut[piece]
    for _ in range(50):
        missing = land & (label < 0)
        if not missing.any():
            break
        p = np.pad(label, 1, constant_values=-1)
        for dr, dc in NB8:
            nb = p[1 + dr : 1 + dr + g.nz, 1 + dc : 1 + dc + g.nx]
            label = np.where(missing & (label < 0) & (nb >= 0), nb, label)
    return label, open_, problems, slivers


def sides(g, land, open_, label, points, skip=6):
    """The region labels found a little way to either side of a border, away from its ends."""
    found = {}
    for (ax, az), (bx, bz) in zip(points, points[1:]):
        n = math.hypot(bx - ax, bz - az) or 1.0
        mx, mz = (ax + bx) / 2, (az + bz) / 2
        if min(math.hypot(mx - points[0][0], mz - points[0][1]), math.hypot(mx - points[-1][0], mz - points[-1][1])) < skip * g.cell:
            continue  # near a junction the third region is close by
        for s in (-2.5, 2.5):
            r, c = g.index(mx - (bz - az) / n * s * g.cell, mz + (bx - ax) / n * s * g.cell)
            if land[r, c] and open_[r, c] and label[r, c] >= 0:
                k = int(label[r, c])
                found[k] = found.get(k, 0) + 1
    return found


def length(points):
    return sum(math.dist(a, b) for a, b in zip(points, points[1:]))


def nearest_on(points, x, z):
    """Distance from (x, z) to a polyline, the segment index and the nearest point on it."""
    best = (math.inf, 0, (x, z))
    for k, ((ax, az), (bx, bz)) in enumerate(zip(points, points[1:])):
        vx, vz = bx - ax, bz - az
        t = max(0.0, min(1.0, ((x - ax) * vx + (z - az) * vz) / ((vx * vx + vz * vz) or 1.0)))
        px, pz = ax + t * vx, az + t * vz
        d = math.hypot(x - px, z - pz)
        if d < best[0]:
            best = (d, k, (px, pz))
    return best


def next_id(doc, kind):
    key = "lastBorderId" if kind == "b" else "lastJunctionId"
    items = doc["borders"] if kind == "b" else doc["junctions"]
    used = [int(x["id"][1:]) for x in items if x["id"][1:].isdigit()]
    n = max(used + [doc.get(key, 0)]) + 1
    doc[key] = n
    return f"{kind}{n:03d}"


def attach(doc, points, tol):
    """Add a newly drawn border. Each end near a junction ends there; an end near another border
    splits that border at a new junction; any other end is free and should lie past the coast.
    Region pairs are set by `sync`."""
    points = [list(p) for p in points]
    ends = []
    for i in (0, -1):
        x, z = points[i]
        j = min(doc["junctions"], key=lambda j: math.hypot(j["at"][0] - x, j["at"][1] - z), default=None)
        if j and math.hypot(j["at"][0] - x, j["at"][1] - z) <= tol:
            points[i] = list(j["at"])
            ends.append(j["id"])
            continue
        hits = [(nearest_on(b["points"], x, z), b) for b in doc["borders"]]
        (d, k, (px, pz)), b = min(hits, key=lambda h: h[0][0], default=((math.inf, 0, (x, z)), None))
        if b is None or d > tol:
            ends.append(None)
            continue
        at = [int(round(px)), int(round(pz))]
        jid = next_id(doc, "j")
        doc["junctions"].append({"id": jid, "at": at})
        head = b["points"][: k + 1] + [at]
        tail = [at] + b["points"][k + 1 :]
        head = [p for n, p in enumerate(head) if n == 0 or p != head[n - 1]]
        tail = [p for n, p in enumerate(tail) if n == 0 or p != tail[n - 1]]
        rest = {"id": next_id(doc, "b"), "regions": list(b["regions"]), "from": jid}
        if b.get("to"):
            rest["to"] = b["to"]
        rest["points"] = tail
        b.pop("to", None)
        b.pop("points")
        b["to"], b["points"] = jid, head  # keeps the canonical key order: id, regions, from, to, points
        doc["borders"].append(rest)
        points[i] = at
        ends.append(jid)
    # drawn points between a snapped end and where the line really starts would make a hook
    if ends[0]:
        while len(points) > 2 and math.dist(points[1], points[0]) < tol / 2:
            points.pop(1)
    if ends[1]:
        while len(points) > 2 and math.dist(points[-2], points[-1]) < tol / 2:
            points.pop(-2)
    border = {"id": next_id(doc, "b"), "regions": []}
    if ends[0]:
        border["from"] = ends[0]
    if ends[1]:
        border["to"] = ends[1]
    border["points"] = points
    doc["borders"].append(border)
    return border


def sync(g, land, doc, seeds):
    """Bring the border network in step with the seeds after edits: each border names the two
    regions on either side of it, borders meeting end to end between the same two regions are
    joined, and unused junctions are dropped. Returns the problems found (none when consistent)."""
    wall = walls(g, doc["borders"])
    label, open_, problems, _ = pieces(g, land, wall, seeds)
    ids = [rid for rid, _ in seeds]
    index = {rid: k for k, rid in enumerate(ids)}
    for b in doc["borders"]:
        found = sides(g, land, open_, label, b["points"])
        if len(found) < 2 and length(b["points"]) < 12 * g.cell:
            found = sides(g, land, open_, label, b["points"], skip=0)  # a short border: look along all of it
        named = [index[r] for r in b["regions"] if r in index]
        total = sum(found.values())
        # a border keeps the pair it names while the land around it agrees; samples near corners of
        # other regions are noise, so only a clear majority against it renames it
        if len(named) == 2 and named[0] != named[1] and (not total or sum(found.get(k, 0) for k in named) >= 0.6 * total):
            continue
        two = sorted(sorted(found, key=lambda k: -found[k])[:2])
        if len(two) == 2:
            b["regions"] = [ids[k] for k in two]
        else:
            problems.append(f"border {b['id']} has {ids[two[0]] if two else 'no region'} on both sides: delete it or add a seed")
    # join borders that meet end to end at a junction only they use, between the same two regions
    while True:
        ends = {}
        for b in doc["borders"]:
            for e in ("from", "to"):
                if b.get(e):
                    ends.setdefault(b[e], []).append(b)
        join = next(((j, bs) for j, bs in ends.items()
                     if len(bs) == 2 and bs[0] is not bs[1] and sorted(bs[0]["regions"]) == sorted(bs[1]["regions"])), None)
        if join is None:
            break
        j, (a, b) = join
        if a.get("to") != j:
            a["points"].reverse()
            a["from"], a["to"] = a.get("to"), a.get("from")
        if b.get("from") != j:
            b["points"].reverse()
            b["from"], b["to"] = b.get("to"), b.get("from")
        joined = {"id": a["id"], "regions": a["regions"]}
        if a.get("from"):
            joined["from"] = a["from"]
        if b.get("to"):
            joined["to"] = b["to"]
        joined["points"] = a["points"] + b["points"][1:]
        doc["borders"][doc["borders"].index(a)] = joined
        doc["borders"].remove(b)
    used = {b.get(e) for b in doc["borders"] for e in ("from", "to")}
    doc["junctions"] = [j for j in doc["junctions"] if j["id"] in used]
    return problems
