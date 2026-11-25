#!/usr/bin/env python3
"""Authoring source: the village basin's rim rock band (#1195 part 4).

Owner decision #1195 Q1 = A (2026-11-24): the basin's cliffs are one continuous sculpted rock
band along the basin's smooth outline, with ledges in it and collision generated from the same
mesh; climbability is set per segment of about 20 m, and the old climbable `VALE_BASIN` square
is gone. Q3: climbable segments show subtle cues (benches, broken rock, occasional ivy; the
clients add worn handholds); sheer rock stays smooth.

The terrain under the band only steps from the floor to the rim where `basin::rock_q10` passes
`basin::STEP_Q10` (the old face's middle). On the 4 m lattice that step is a sawtooth one cell
wide. The band is fitted over it, so the lattice never shows:

- its outline is the step's level line in the smooth `rock_q10` field, sampled every metre;
- every ring is a profile across that outline. Its skin is the steepest-face envelope (a cone of
  slope `STEEP` or `SHEER_STEEP`) over the terrain's upper bound on both triangulations of each
  cell, widened along the outline and smoothed, so the face runs smoothly in front of the
  sawtooth and its top meets the rim;
- the six climbs' lanes, the brook's channel and the waterfall stay terrain: there the skin
  dips under the ground, so their ledges, landings and cuttings are cut into the band with a
  rock lip in front and a wall behind;
- climbable rings get benches and broken rock, sheer ones a smooth steeper face; the crest
  stands irregularly above the rim;
- the rings are split into closed meshes of about `SEGMENT` m (collider IDs from 45 000), each
  carrying its own `"grip": "rock" | "sheer"`. The cliff under Stillwater's terrace
  (`SHEER_LAKE_CLIFF`) is sheer. Neighbouring segments share their seam ring.

Run headless with `python3` from the repository root, after exporting its input:

    cargo run -p mmorpg-scenery --example export-basin-context > artifacts/basin/context.json
    python3 content/greyhaven/authoring/basin_rim.py

It writes `crates/mmorpg-core/content/collision/basin-rim.json` (the segments) and
`content/greyhaven/dressing/basin-rim.json` (ivy on a few climbable faces), and checks that the
skin covers every stepped terrain cell it should and none of the keep-clear ground. The output
depends only on the context, so it regenerates byte for byte.

In Blender (5.2+; the Python console, `__file__` set to this file's path) it also draws the band
in the collection `Basin rock band (1195 part 4)`: orange where climbable, blue where sheer, one
object per collider, with the ivy and the terrain lattice. Rerunning replaces only that
collection's children.
"""
import json
import math
import pathlib
import sys

try:
    import bpy
except ImportError:  # headless export
    bpy = None

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from hollow_cliffs import earclip, orient, check_closed, ray  # noqa: E402

ROOT = HERE.parents[2]
CONTEXT = ROOT / 'artifacts/basin/context.json'
COLLISION = ROOT / 'crates/mmorpg-core/content/collision/basin-rim.json'
DRESSING_OUT = ROOT / 'content/greyhaven/dressing/basin-rim.json'
NAME = 'Basin rock band (1195 part 4)'
UNITS = 100
FIRST_ID = 45_000

# Rings this far apart along the outline (m).
SPACING = 1.0
# … and this close near the climbs' lanes and the brook, within this far of them (m).
FINE_SPACING = 1.0
DENSE_NEAR = 6.0
# Near a lane, rings run square to the grid within this far across it (m), fully alongside
# it and this far past its ends, fading over the next stretch; a ring stands this far either
# side of each lane end.
SNAP_ACROSS = 16.0
SNAP_HOLD = 4.0
SNAP_FADE = 10.0
EDGE_RING = 0.06
# Each ring's profile is sampled this finely across the outline (m) before it is simplified …
DENSE = 0.1
# … from this far in front of the outline to this far behind it.
REACH = (-13.0, 28.0)
# … and simplified to within this (m).
TOLERANCE = 0.07
# … with no piece longer than this across the outline (m).
LONGEST = 2.0
# Profile samples this far apart up the face and behind it (m), on a grid shared by rings.
FACE_GRID = 0.5
SHOULDER_GRID = 1.2
# A terrain cell is part of the step (or of the old face's upper half behind it) when its corners
# differ by more than this, its lowest lies under `STEP_BELOW` and its highest over
# `STEP_ABOVE` (m): neither the rim's own hills nor the floor's mounds are.
STEP_SPAN = 1.8
STEP_BELOW = 9.5
STEP_ABOVE = 4.0
# Further behind the outline than this (m), only steps near a lane belong to the band.
FAR = 9.0
# The skin stands this far over the terrain it hides, and this far under the ground it leaves.
CLEAR = 0.05
UNDER = 0.08
# Face slopes (rise per metre) of climbable and sheer rock.
STEEP = 3.3
SHEER_STEEP = 5.0
# The step is widened this far along the outline on either side, then smoothed over a
# Gaussian this wide (m).
WIDEN = 4.0
SMOOTH = 2.5
# Climbable faces: benches (fraction of the face's height, depth in m) and buttresses
# standing out from the face by up to this (m).
BENCHES = ((0.36, 0.75), (0.68, 0.6))
BUTTRESS = 0.6
# The face rises this steeply (rise per metre) from a smooth foot.
FACE_STEEP = 4.0
# The band's foot and back reach this much past the step, then bury.
FOOT_MARGIN = 0.6
BACK_MARGIN = 1.0
BURY = 1.2
# Segments about this long (m).
SEGMENT = 20.0
# The sheer cliff under Stillwater's terrace (`greyhaven_vale::SHEER_LAKE_CLIFF`), in metres.
SHEER_LAKE_CLIFF = ((20.0, -32.0), (128.0, -20.0))
# The brook's outlet channel over the rim and the waterfall (`landscape.json`'s outlet and
# `mmorpg-presentation`'s cascade), which the band leaves open.
OUTLET = [(-63.0, 2.0), (-60.0, 15.0), (-56.0, 22.0), (-50.0, 24.0), (-48.0, 24.0)]
CHANNEL = 2.4
# The band keeps this far clear of where a creature or NPC stands (m).
UNIT_CLEAR = 3.0
# … and stays this far under the ground there (m), so a unit on sloping ground never touches it.
UNIT_UNDER = 0.8
# The band keeps this far clear of a road's centre line (m).
ROAD_CLEAR = 3.4
# … on the rim side of the step (ground above this, m) or off the step altogether: a road never
# climbs the face but by a lane.
ROAD_RIM = 5.0
# Ground level with a climb's lane end within this (m) and this close to it (m) stays open.
ACCESS_LEVEL = 0.6
ACCESS_REACH = 10.0
# … at a lane end on the floor (below this) or on the rim (above this), not between lanes (m).
LOW_END = 1.0
HIGH_END = 10.4
CASCADE = [(-48.0, 10.35), (-46.4, 10.8), (-45.8, 4.8), (-44.0, 1.4), (-42.4, 0.0)]
FALL_Z = (23.0, 25.0)
# Ivy (#621's kit) down a few climbable faces: rings (by index along the outline from its start).
IVY_EVERY = 37
VINE_PITCH = 2.0
VINE_TOP_HEIGHT = 1.0
VINE_OUT = 0.05


def wave(*keys):
    """A fixed, smooth pseudo-random value in [-1, 1] for these keys."""
    a = sum(k * m for k, m in zip(keys, (2.713, 1.371, 0.719, 3.17)))
    b = sum(k * m for k, m in zip(keys, (0.931, -2.113, 0.373, -1.57)))
    return 0.6 * math.sin(a) + 0.4 * math.sin(b + 1.3)


def smoothstep(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


class Lattice:
    """The zone terrain's samples round the basin (metres)."""

    def __init__(self, data, soft=()):
        self.soft = [tuple((x / UNITS, z / UNITS) for x, z in box) for box in soft]
        self.ox, self.oz = (v / UNITS for v in data['origin'])
        self.step = data['step'] / UNITS
        self.columns, self.rows = data['columns'], data['rows']
        self.h = [v / UNITS for v in data['heights']]

    def corners(self, x, z):
        c = min(self.columns - 2, max(0, int(math.floor((x - self.ox) / self.step))))
        r = min(self.rows - 2, max(0, int(math.floor((z - self.oz) / self.step))))
        fx = (x - self.ox) / self.step - c
        fz = (z - self.oz) / self.step - r
        at = lambda cc, rr: self.h[rr * self.columns + cc]
        return (at(c, r), at(c + 1, r), at(c, r + 1), at(c + 1, r + 1)), fx, fz, (c, r)

    def bounds(self, x, z):
        """The lowest and highest the ground can be at `(x, z)`: bilinear or either diagonal."""
        (a, b, c, d), fx, fz, _ = self.corners(x, z)
        bilinear = (a * (1 - fx) + b * fx) * (1 - fz) + (c * (1 - fx) + d * fx) * fz
        # Diagonal a-d.
        one = a + (b - a) * fx + (d - b) * fz if fx >= fz else a + (c - a) * fz + (d - c) * fx
        # Diagonal b-c.
        two = (a + (b - a) * fx + (c - a) * fz if fx + fz <= 1
               else d + (c - d) * (1 - fx) + (b - d) * (1 - fz))
        return min(bilinear, one, two), max(bilinear, one, two)

    def stepped(self, x, z):
        if any(x0 <= x <= x1 and z0 <= z <= z1 for (x0, z0), (x1, z1) in self.soft):
            return False
        """Whether the cell at `(x, z)` is part of the floor-to-rim step: it spans more than
        `STEP_SPAN` and reaches down off the rim."""
        (a, b, c, d), _, _, _ = self.corners(x, z)
        low, high = min(a, b, c, d), max(a, b, c, d)
        return high - low > STEP_SPAN and low < STEP_BELOW and high > STEP_ABOVE


class Lanes:
    def __init__(self, lanes, units=(), roads=()):
        # Road pieces in 8 m buckets round their extent and `ROAD_CLEAR`.
        self.road_buckets = {}
        for road in roads:
            points = [(x / UNITS, z / UNITS) for x, z in road['points']]
            for a, b in zip(points, points[1:]):
                for cx in range(math.floor((min(a[0], b[0]) - ROAD_CLEAR) / 8), math.floor((max(a[0], b[0]) + ROAD_CLEAR) / 8) + 1):
                    for cz in range(math.floor((min(a[1], b[1]) - ROAD_CLEAR) / 8), math.floor((max(a[1], b[1]) + ROAD_CLEAR) / 8) + 1):
                        self.road_buckets.setdefault((cx, cz), []).append((a, b))
        self.buckets = {}
        for x, z in units:
            point = (x / UNITS, z / UNITS)
            self.buckets.setdefault((math.floor(point[0] / 4), math.floor(point[1] / 4)), []).append(point)
        self.lanes = []
        for lane in lanes:
            (x0, z0), (x1, z1) = ((v / UNITS for v in lane[k]) for k in ('min', 'max'))
            h0, h1 = (v / UNITS for v in lane['heights'])
            self.lanes.append((x0, z0, x1, z1, h0, h1, lane['along']))
        # The grid axis rings run along beside each lane: square across a sloping lane, and
        # for a level landing the same as its nearest sloping lane's, so a climb's rings agree.
        sloping = [lane for lane in self.lanes if lane[4] != lane[5]]

        def gap(a, b):
            return math.hypot(max(a[0] - b[2], b[0] - a[2], 0), max(a[1] - b[3], b[1] - a[3], 0))

        self.axes = []
        for lane in self.lanes:
            source = lane if lane[4] != lane[5] else min(sloping, key=lambda other: gap(lane, other))
            self.axes.append('z' if source[6] == 'x' else 'x')

    def access(self, x, z, lo, hi):
        """Whether `(x, z)` is open ground at a sloping lane's end, level with it within
        `ACCESS_LEVEL`: the floor a climb starts from or the rim it reaches stays open."""
        for x0, z0, x1, z1, h0, h1, along in self.lanes:
            if h0 == h1:
                continue
            ends = (((x0, (z0 + z1) / 2), h0), ((x1, (z0 + z1) / 2), h1)) if along == 'x' else \
                   ((((x0 + x1) / 2, z0), h0), (((x0 + x1) / 2, z1), h1))
            for (ex, ez), h in ends:
                if LOW_END < h < HIGH_END:
                    continue
                if math.hypot(x - ex, z - ez) < ACCESS_REACH and abs(lo - h) <= ACCESS_LEVEL \
                        and abs(hi - h) <= ACCESS_LEVEL:
                    return True
        return False

    def near_road(self, x, z):
        """Whether `(x, z)` lies on a road (within `ROAD_CLEAR` of its centre line)."""
        pieces = self.road_buckets.get((math.floor(x / 8), math.floor(z / 8)), ())
        return any(polyline_distance([a, b], x, z) < ROAD_CLEAR for a, b in pieces)

    def near_unit(self, x, z, margin=0.0):
        """Whether `(x, z)` lies where a creature or NPC stands: the band keeps clear there."""
        cx, cz = math.floor(x / 4), math.floor(z / 4)
        return any(math.hypot(x - ux, z - uz) < UNIT_CLEAR + margin
                   for dx in (-1, 0, 1) for dz in (-1, 0, 1)
                   for ux, uz in self.buckets.get((cx + dx, cz + dz), ()))

    def height(self, x, z, margin=0.0):
        """The first lane's height at `(x, z)` (lanes override in order), or None."""
        for x0, z0, x1, z1, h0, h1, along in self.lanes:
            if x0 - margin <= x <= x1 + margin and z0 - margin <= z <= z1 + margin:
                v, a, b = (x, x0, x1) if along == 'x' else (z, z0, z1)
                t = min(1.0, max(0.0, (v - a) / max(b - a, 1e-9)))
                return h0 + (h1 - h0) * t
        return None


def polyline_distance(path, x, z):
    best = math.inf
    for (ax, az), (bx, bz) in zip(path, path[1:]):
        ex, ez = bx - ax, bz - az
        t = min(1.0, max(0.0, ((x - ax) * ex + (z - az) * ez) / (ex * ex + ez * ez)))
        best = min(best, math.hypot(x - ax - ex * t, z - az - ez * t))
    return best


def cascade_height(x):
    for (x0, h0), (x1, h1) in zip(CASCADE, CASCADE[1:]):
        if x0 <= x <= x1:
            return h0 + (h1 - h0) * (x - x0) / (x1 - x0)
    return None


def in_fall(x, z):
    return CASCADE[0][0] - 0.4 <= x <= CASCADE[-1][0] and FALL_Z[0] <= z <= FALL_Z[1]


def in_channel(x, z):
    return x <= CASCADE[0][0] + 0.2 and polyline_distance(OUTLET, x, z) < CHANNEL


# The outline: the step's level line in the smooth rock field.

def contour(field):
    """The longest closed level line of `field` at its level, as points (metres)."""
    ox, oz = (v / UNITS for v in field['origin'])
    step = field['step'] / UNITS
    cols, rows = field['columns'], field['rows']
    values = field['values']
    level = field['level'] - 0.5
    v = lambda c, r: values[r * cols + c] - level
    edges = {}

    def point(c0, r0, c1, r1):
        a, b = v(c0, r0), v(c1, r1)
        t = a / (a - b)
        return (round(ox + (c0 + (c1 - c0) * t) * step, 6), round(oz + (r0 + (r1 - r0) * t) * step, 6))

    for r in range(rows - 1):
        for c in range(cols - 1):
            corners = [(c, r), (c + 1, r), (c + 1, r + 1), (c, r + 1)]
            crossing = []
            for k in range(4):
                (c0, r0), (c1, r1) = corners[k], corners[(k + 1) % 4]
                if (v(c0, r0) < 0) != (v(c1, r1) < 0):
                    crossing.append(point(c0, r0, c1, r1))
            if len(crossing) == 2:
                a, b = crossing
                edges.setdefault(a, []).append(b)
                edges.setdefault(b, []).append(a)
            elif len(crossing) == 4:
                # A saddle: pair by the cell centre's side.
                centre = sum(v(cc, rr) for cc, rr in corners) / 4
                pairs = [(0, 1), (2, 3)] if (centre < 0) == (v(c, r) < 0) else [(0, 3), (1, 2)]
                for i, j in pairs:
                    a, b = crossing[i], crossing[j]
                    edges.setdefault(a, []).append(b)
                    edges.setdefault(b, []).append(a)
    loops = []
    seen = set()
    for start in sorted(edges):
        if start in seen:
            continue
        loop = [start]
        seen.add(start)
        previous, here = None, start
        while True:
            options = [p for p in edges[here] if p != previous and p not in seen]
            if not options:
                break
            previous, here = here, options[0]
            seen.add(here)
            loop.append(here)
        loops.append(loop)
    loop = max(loops, key=len)
    # Counter-clockwise in (x, z) with z down is clockwise on a map: fix one orientation.
    area = sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(loop, loop[1:] + loop[:1]))
    if area < 0:
        loop.reverse()
    # Start at the outline's southernmost point, so the order is stable.
    first = max(range(len(loop)), key=lambda i: (loop[i][1], -loop[i][0]))
    return loop[first:] + loop[:first]


def resample_closed(loop, spacing):
    closed = loop + [loop[0]]
    lengths = [0.0]
    for a, b in zip(closed, closed[1:]):
        lengths.append(lengths[-1] + math.dist(a, b))
    count = max(3, round(lengths[-1] / spacing))
    out = []
    j = 0
    for i in range(count):
        want = lengths[-1] * i / count
        while lengths[j + 1] < want:
            j += 1
        f = (want - lengths[j]) / max(1e-12, lengths[j + 1] - lengths[j])
        a, b = closed[j], closed[j + 1]
        out.append((a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f))
    return out


def smooth_closed(points, passes, weight=0.5):
    n = len(points)
    for _ in range(passes):
        points = [((1 - weight) * points[i][0] + weight * (points[i - 1][0] + points[(i + 1) % n][0]) / 2,
                   (1 - weight) * points[i][1] + weight * (points[i - 1][1] + points[(i + 1) % n][1]) / 2)
                  for i in range(n)]
    return points


def near_keep_clear(lanes, x, z):
    """Whether `(x, z)` lies near a lane, the brook's channel or the fall, where rings run
    closer so the band's notches keep sharp edges."""
    return (lanes.height(x, z, margin=DENSE_NEAR) is not None
            or polyline_distance(OUTLET + [(CASCADE[-1][0], 24.0)], x, z) < CHANNEL + DENSE_NEAR)


def lane_axis(lanes, x, z, normal):
    """Near a lane, the grid axis the ring's normal snaps to (the lane's, `Lanes.axes`) and how
    fully (1 alongside the lane, fading to 0 past its ends); otherwise `(None, 0)`."""
    best = (None, 0.0)
    for (x0, z0, x1, z1, *_), axis in zip(lanes.lanes, lanes.axes):
        dx = max(x0 - x, 0.0, x - x1)
        dz = max(z0 - z, 0.0, z - z1)
        along, across = (dz, dx) if axis == 'x' else (dx, dz)
        if across <= SNAP_ACROSS:
            weight = 1 - smoothstep(SNAP_HOLD, SNAP_HOLD + SNAP_FADE, along)
            if weight > best[1]:
                best = (axis, weight)
    return best


def outline(context, lanes):
    """The outline's ring points, outward normals and arc lengths: every `SPACING` m, every
    `FINE_SPACING` m near the climbs' lanes and the brook. Alongside a lane the rings run square
    to the terrain grid, and a ring lies just either side of each of the lane's ends, so the
    band's notches for the lanes keep straight, sharp edges."""
    loop = contour(context['rock'])
    dense = resample_closed(smooth_closed(resample_closed(loop, 0.5), 6), 0.1)
    dense_s = [0.0]
    for a, b in zip(dense, dense[1:]):
        dense_s.append(dense_s[-1] + math.dist(a, b))
    total = dense_s[-1] + math.dist(dense[-1], dense[0])

    def tangent_normal(at):
        a = point_at(dense, dense_s, total, at - 1.5)
        b = point_at(dense, dense_s, total, at + 1.5)
        tx, tz = b[0] - a[0], b[1] - a[1]
        length = math.hypot(tx, tz)
        return (tz / length, -tx / length)

    # Orient outward: toward rising rock.
    field = context['rock']
    sign = 1.0
    probe = 0
    for k in range(0, len(dense), 10):
        (x, z), (nx, nz) = dense[k], tangent_normal(dense_s[k])
        probe += rock_at(field, x + nx * 3, z + nz * 3) - rock_at(field, x - nx * 3, z - nz * 3)
    if probe < 0:
        sign = -1.0

    def normal_at(at):
        nx, nz = tangent_normal(at)
        return (sign * nx, sign * nz)

    # Regular rings …
    marks = [0.0]
    since = 0.0
    for k in range(1, len(dense)):
        since += dense_s[k] - dense_s[k - 1]
        want = FINE_SPACING if near_keep_clear(lanes, *dense[k]) else SPACING
        if since >= want - 1e-9:
            marks.append(dense_s[k])
            since = 0.0
    # … and one just either side of each lane end the snapped rings run along.
    exact = []
    for k in range(len(dense)):
        a, b = dense[k], dense[(k + 1) % len(dense)]
        axis, weight = lane_axis(lanes, *a, normal_at(dense_s[k]))
        if weight < 1:
            continue
        coordinate = 1 if axis == 'x' else 0
        for x0, z0, x1, z1, *_ in lanes.lanes:
            ends = (z0, z1) if axis == 'x' else (x0, x1)
            for end in ends:
                for offset in (-EDGE_RING, EDGE_RING):
                    e = end + offset
                    if (a[coordinate] - e) * (b[coordinate] - e) > 0 or a[coordinate] == b[coordinate]:
                        continue
                    across = max(x0 - a[0], 0, a[0] - x1) if axis == 'x' else max(z0 - a[1], 0, a[1] - z1)
                    if across > SNAP_ACROSS:
                        continue
                    f = (e - a[coordinate]) / (b[coordinate] - a[coordinate])
                    exact.append(dense_s[k] + f * math.dist(a, b))
    exact = sorted(set(round(v, 6) for v in exact))
    marks = sorted([m for m in marks if all(abs(m - e) > 0.2 for e in exact)] + exact)
    points, normals, s = [], [], []
    for at in marks:
        x, z = point_at(dense, dense_s, total, at)
        nx, nz = normal_at(at)
        axis, weight = lane_axis(lanes, x, z, (nx, nz))
        if axis is not None:
            tx, tz = ((math.copysign(1.0, nx), 0.0) if axis == 'x' else (0.0, math.copysign(1.0, nz)))
            nx, nz = nx * (1 - weight) + tx * weight, nz * (1 - weight) + tz * weight
            length = math.hypot(nx, nz)
            nx, nz = nx / length, nz / length
        points.append((x, z))
        normals.append((nx, nz))
        s.append(at)
    return points, normals, s, total


def point_at(points, s, total, at):
    """The outline's point `at` metres along it (closed)."""
    at %= total
    lo, hi = 0, len(s) - 1
    while lo < hi:
        mid = (lo + hi + 1) // 2
        if s[mid] <= at:
            lo = mid
        else:
            hi = mid - 1
    a = points[lo]
    b = points[(lo + 1) % len(points)]
    span = (s[lo + 1] if lo + 1 < len(s) else total) - s[lo]
    f = (at - s[lo]) / span if span > 0 else 0.0
    return (a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f)


def window(rings, total, reach):
    """For each ring, the indices of the rings within `reach` metres along the outline, with
    their signed offsets."""
    n = len(rings)
    out = []
    for i in range(n):
        here = []
        for step in (1, -1):
            j, travelled = i, 0.0
            while True:
                k = (j + step) % n
                travelled += math.dist((rings[j].x, rings[j].z), (rings[k].x, rings[k].z))
                if travelled > reach or k == i:
                    break
                here.append((k, step * travelled))
                j = k
        out.append([(i, 0.0)] + here)
    return out


def rock_at(field, x, z):
    ox, oz = (v / UNITS for v in field['origin'])
    step = field['step'] / UNITS
    c = min(field['columns'] - 1, max(0, round((x - ox) / step)))
    r = min(field['rows'] - 1, max(0, round((z - oz) / step)))
    return field['values'][r * field['columns'] + c]


# Profiles across the outline.

class Ring:
    pass


def sheer_at(x, z):
    (x0, z0), (x1, z1) = SHEER_LAKE_CLIFF
    return x0 <= x <= x1 and z0 <= z <= z1


def reach_limits(points, normals, near):
    """How far in front and behind each ring's profile may run before it meets another part of
    the outline (the medial axis), so rings never fold through each other."""
    bucket = {}
    for i, (x, z) in enumerate(points):
        bucket.setdefault((math.floor(x / 4), math.floor(z / 4)), []).append(i)

    def nearest(x, z, skip, around):
        best = math.inf
        bx, bz = math.floor(x / 4), math.floor(z / 4)
        for dx in range(-around, around + 1):
            for dz in range(-around, around + 1):
                for j in bucket.get((bx + dx, bz + dz), ()):
                    if j in skip:
                        continue
                    best = min(best, math.dist(points[j], (x, z)))
        return best

    n = len(points)
    limits = []
    for i in range(n):
        (x, z), (nx, nz) = points[i], normals[i]
        skip = {j for j, _ in near[i]}
        out = []
        for sign, reach in ((-1, -REACH[0]), (1, REACH[1])):
            d = 0.5
            limit = reach
            while d <= reach:
                px, pz = x + sign * nx * d, z + sign * nz * d
                if nearest(px, pz, skip, 1 + int(d / 4)) < d * 0.95:
                    limit = d
                    break
                d += 0.5
            out.append(limit)
        limits.append((-out[0], out[1]))
    return limits


def build_rings(context):
    lattice = Lattice(context['lattice'], context['softShoulder'])
    lanes = Lanes(context['lanes'], context['units'], context['roads'])
    points, normals, arc, total = outline(context, lanes)
    n = len(points)
    rings = []
    for i in range(n):
        ring = Ring()
        ring.index = i
        ring.s = arc[i]
        (ring.x, ring.z), (ring.nx, ring.nz) = points[i], normals[i]
        rings.append(ring)
    near = window(rings, total, 12.0)
    limits = reach_limits(points, normals, near)
    samples = [round(REACH[0] + DENSE * k, 6) for k in range(round((REACH[1] - REACH[0]) / DENSE) + 1)]
    for i, ring in enumerate(rings):
        ring.limits = limits[i]
        ring.d = samples
        # Half way to each neighbour along the outline: the strip this ring's samples stand for.
        before = math.dist(points[i], points[i - 1]) / 2
        after = math.dist(points[i], points[(i + 1) % n]) / 2
        tx, tz = -ring.nz, ring.nx
        ring.lo, ring.hi, ring.keep, ring.step, ring.fall = [], [], [], [], []
        ring.deep = []
        for d in samples:
            x, z = ring.x + ring.nx * d, ring.z + ring.nz * d
            lo, hi = lattice.bounds(x, z)
            for f in (-before, before * -0.5, after * 0.5, after):
                lo2, hi2 = lattice.bounds(x + tx * f, z + tz * f)
                lo, hi = min(lo, lo2), max(hi, hi2)
            ring.lo.append(lo)
            ring.hi.append(hi)
            lane = lanes.height(x, z, margin=-0.03)
            if lane is None and (in_channel(x, z) or lanes.near_unit(x, z)
                                 or lanes.access(x, z, lo, hi)
                                 or lanes.near_road(x, z) and (lo >= ROAD_RIM or not lattice.stepped(x, z))):
                lane = lo
            ring.keep.append(lane)
            ring.deep.append(lanes.near_unit(x, z))

            inside = ring.limits[0] <= d <= ring.limits[1]
            # Far behind the outline only a climb's lanes and their banks belong to the band.
            near_lane = lanes.height(x, z, 5.0) is not None
            ring.step.append(inside and (lattice.stepped(x, z) and (d <= FAR or near_lane)
                                         or lanes.height(x, z, 0.5) is not None))
            ring.fall.append(cascade_height(x) if in_fall(x, z) else None)
        ring.sheer = sheer_at(ring.x, ring.z) or sheer_at(ring.x + ring.nx * 3, ring.z + ring.nz * 3)
    # A lone ring of the other kind joins its neighbours, so no sliver segment forms.
    flags = [r.sheer for r in rings]
    for i, ring in enumerate(rings):
        if flags[i - 1] == flags[(i + 1) % n] != flags[i]:
            ring.sheer = flags[i - 1]
    wide = window(rings, total, WIDEN)
    smooth = window(rings, total, 3 * SMOOTH)
    return rings, lattice, lanes, wide, smooth


def band_extent(rings, wide, smooth):
    """Each ring's stepped interval `[front, back]` across the outline, widened along it. A
    ring with no step near the outline (the soft southern shoulder) carries no band."""
    for ring in rings:
        steps = [d for d, s in zip(ring.d, ring.step) if s]
        # The cluster round the outline: gaps narrower than a lane are bridged.
        near = [d for d in steps if -5.0 <= d <= 7.0]
        ring.empty = not near
        if ring.empty:
            ring.front, ring.back = -1.0, 1.0
            continue
        front, back = min(near), max(near)
        for d in sorted(steps):
            if front - 4.6 <= d < front:
                front = d
        for d in sorted(steps, reverse=True):
            if back < d <= back + 4.6:
                back = d
        for d in sorted(steps):
            if back < d <= back + 4.6:
                back = d
        ring.front, ring.back = front, back
    # A short run of rings without a step between banded ones is bridged.
    n = len(rings)
    for i, ring in enumerate(rings):
        if ring.empty:
            if any(o < 0 for j, o in wide[i] if not rings[j].empty) and \
                    any(o > 0 for j, o in wide[i] if not rings[j].empty):
                ring.bridge = True
    for ring in rings:
        if getattr(ring, 'bridge', False):
            ring.empty = False
    banded = [not ring.empty for ring in rings]
    fronts = [min(rings[j].front for j, _ in wide[i] if banded[j]) if banded[i] else -1.0
              for i in range(n)]
    backs = [max(rings[j].back for j, _ in wide[i] if banded[j]) if banded[i] else 1.0
             for i in range(n)]
    fronts = gaussian(fronts, smooth, banded)
    backs = gaussian(backs, smooth, banded)
    for ring, front, back in zip(rings, fronts, backs):
        ring.front = max(min(front, ring.front) - FOOT_MARGIN, ring.limits[0] + 0.5)
        ring.back = min(max(back, ring.back) + BACK_MARGIN, ring.limits[1] - 0.5)


def gaussian(values, smooth, banded=None):
    """`values` (one per ring) smoothed along the outline over the rings' `smooth` windows,
    from banded rings only when `banded` is given."""
    out = []
    for i in range(len(values)):
        weights = [(math.exp(-0.5 * (offset / SMOOTH) ** 2), j) for j, offset in smooth[i]
                   if banded is None or banded[j]]
        total = sum(w for w, _ in weights)
        out.append(sum(w * values[j] for w, j in weights) / total if total else values[i])
    return out


def skins(rings, wide, smooth):
    """Each ring's dense skin height across its samples."""
    n = len(rings)
    m = len(rings[0].d)
    # What each sample must cover: the terrain's upper bound, or a lane's own level.
    need = []
    for ring in rings:
        row = []
        for k, d in enumerate(ring.d):
            if not ring.front - 0.01 <= d <= ring.back + 0.01:
                row.append(-math.inf)
            elif ring.keep[k] is not None:
                row.append(ring.keep[k])
            elif ring.fall[k] is not None:
                row.append(ring.hi[k] + CLEAR)
            else:
                row.append(ring.hi[k] + CLEAR)
        need.append(row)
    # Widened along the outline …
    wide = [[max(need[j][k] for j, _ in wide[i]) for k in range(m)] for i in range(n)]
    # … then the steepest face over it, in front of every point it must cover.
    faces = []
    for i, ring in enumerate(rings):
        slope = (SHEER_STEEP if ring.sheer else STEEP) * DENSE
        row = [-math.inf] * m
        best = -math.inf
        for k in range(m - 1, -1, -1):
            best = max(wide[i][k], best - slope)
            row[k] = best
        faces.append(row)
    # Smoothed along the outline, so the face's plan follows the outline, not the lattice.
    across = []
    for k in range(m):
        column = [faces[i][k] if faces[i][k] > -1e6 else -50.0 for i in range(n)]
        across.append(gaussian(column, smooth))
    smooth = across
    for i, ring in enumerate(rings):
        ring.skin = []
        for k, d in enumerate(ring.d):
            s = max(smooth[k][i], need[i][k])
            ring.skin.append(s)


def plain_faces(rings, wide, smooth):
    """A clean, steep face on every ring of the band: from a foot set smoothly along the
    outline in front of the skin's own rise, up at `FACE_STEEP` to the top of the step, so the
    face stays steep and climbable where smoothing has laid the skin back. It only raises the
    skin, so it still hides the terrain. In front of an opening (a lane, a road, a creature's
    post) it stops at the opening's own level, so a ledge keeps its lip and the opening stays
    open."""
    n = len(rings)
    feet = []
    for ring in rings:
        k0 = first_at(ring, ring.front)
        k1 = max(k for k, d in enumerate(ring.d) if d <= ring.back)
        floor = ring.lo[k0]
        rise = next((k for k in range(k0, k1 + 1) if ring.skin[k] > floor + 0.15), k0)
        feet.append(ring.d[rise])
    lowest = [min(feet[j] for j, _ in wide[i]) for i in range(n)]
    eased = gaussian(lowest, smooth)
    for i, ring in enumerate(rings):
        if ring.empty:
            continue
        foot = min(eased[i], feet[i])
        k0 = first_at(ring, ring.front)
        k1 = max(k for k, d in enumerate(ring.d) if d <= ring.back)
        floor = ring.lo[k0]
        # The face climbs to the top of the step it hides, not to a hill further back …
        stepped = [ring.hi[k] for k in range(k0, k1 + 1) if ring.step[k]]
        rim = min(max(ring.skin[k] for k in range(k0, k1 + 1)), max(stepped, default=floor) + 0.6)
        # … and no higher than the first opening behind it, which it stops in front of.
        opening = next((k for k in range(k0, k1 + 1)
                        if ring.keep[k] is not None or ring.fall[k] is not None), k1 + 1)
        if opening <= k1:
            level = ring.keep[opening] if ring.keep[opening] is not None else ring.fall[opening]
            rim = min(rim, level + 0.05)
        slope = SHEER_STEEP if ring.sheer else FACE_STEEP
        for k in range(k0, opening):
            ring.skin[k] = max(ring.skin[k], min(rim, floor + slope * (ring.d[k] - foot)))
        ring.front = min(ring.front, foot - 0.1)


def first_at(ring, d):
    return next(k for k, v in enumerate(ring.d) if v >= d)


def shift_face(ring, below, steps, level=None):
    """Moves the face under height `below` `steps` samples toward the floor: a bench at
    `level` when given, else a buttress. Only ever raises the skin."""
    old = ring.skin[:]
    k_front = first_at(ring, ring.front)
    cross = next((k for k in range(k_front, len(old)) if old[k] >= below), None)
    if cross is None or k_front - steps < 1:
        return False
    for k in range(k_front - steps, cross):
        moved = old[k + steps]
        if level is not None and k >= cross - steps:
            moved = min(level, moved)
        ring.skin[k] = max(old[k], moved)
    ring.front = ring.d[k_front - steps]
    return True


def shape(rings):
    """Benches, broken rock and the crest on each ring's face; the keep-clear ground under."""
    for ring in rings:
        u = ring.s
        k_back = max(k for k, d in enumerate(ring.d) if d <= ring.back)
        span = range(first_at(ring, ring.front), k_back + 1)
        rim = max(ring.skin[k] for k in span)
        floor = ring.lo[span[0]]
        lanes_here = any(ring.keep[k] is not None or ring.fall[k] is not None for k in span)
        plain = not ring.sheer and not lanes_here and rim - floor > 6
        # Benches on climbable rock, where the face is plain: the face under each moves out.
        if plain:
            for b, (base, depth) in enumerate(BENCHES):
                level = floor + (rim - floor) * (base + 0.06 * wave(u * 0.071, b, 3))
                deep = depth * max(0.0, 0.55 + 0.6 * wave(u * 0.113, b, 7))
                deep = min(deep, ring.front - ring.limits[0] - 0.6)
                if deep >= 2 * DENSE:
                    shift_face(ring, level, round(deep / DENSE), level)
        # Buttresses standing proud of the face between cracks, slowly along the outline.
        if plain:
            push = min(BUTTRESS * max(0.0, wave(u * 0.23, 3)) ** 0.7,
                       ring.front - ring.limits[0] - 0.6)
            if push >= DENSE:
                shift_face(ring, rim - 0.3, round(push / DENSE))
        # Sheer rock rises as one steep, plain face from its foot: no shelf a climber could
        # pull up onto.
        if ring.sheer:
            k_front = first_at(ring, ring.front)
            stepped = [ring.hi[k] for k in range(k_front, k_back + 1) if ring.step[k]]
            top = min(rim, max(stepped, default=floor) + 0.6)
            # The wall starts far enough in front that no rock at the foot stands above it.
            lift = max(ring.skin[k] - floor - SHEER_STEEP * (ring.d[k] - ring.d[k_front])
                       for k in range(k_front, k_back + 1) if ring.skin[k] < top)
            steps = max(0, math.ceil(lift / SHEER_STEEP / DENSE))
            k_wall = max(1, k_front - steps)
            for k in range(k_wall, k_back + 1):
                wall = floor + SHEER_STEEP * (ring.d[k] - ring.d[k_wall])
                ring.skin[k] = max(ring.skin[k], min(top, wall))
            ring.front = ring.d[k_wall]
        # The crest stands irregularly over the rim; sheer rock keeps a cleaner lip.
        span = range(first_at(ring, ring.front), k_back + 1)
        extra = (0.35 + 0.55 * (0.5 + 0.5 * wave(u * 0.083, 11))) * (0.6 if ring.sheer else 1.0)
        top = next((k for k in span if ring.skin[k] >= rim - 0.4), None)
        if top is not None:
            for k in span:
                if ring.keep[k] is not None or ring.fall[k] is not None:
                    continue
                t = ring.d[k] - ring.d[top]
                bump = smoothstep(-1.2, 0.0, t) * (1 - smoothstep(0.6, 3.5, t))
                ring.skin[k] += extra * bump
        # Keep-clear ground: lanes and the brook's channel stay terrain; the fall's rock stays
        # behind its water.
        for k in span:
            if ring.keep[k] is not None:
                ring.skin[k] = min(ring.skin[k], ring.lo[k] - (UNIT_UNDER if ring.deep[k] else UNDER))
            elif ring.fall[k] is not None:
                ring.skin[k] = max(ring.hi[k] + CLEAR, min(ring.skin[k], ring.fall[k] - 0.25))


def simplify(points, tolerance):
    """Douglas-Peucker over a polyline of (d, h)."""
    keep = [False] * len(points)
    keep[0] = keep[-1] = True
    stack = [(0, len(points) - 1)]
    while stack:
        a, b = stack.pop()
        (ax, ay), (bx, by) = points[a], points[b]
        best, at = 0.0, None
        length = math.hypot(bx - ax, by - ay)
        for k in range(a + 1, b):
            px, py = points[k]
            if length < 1e-12:
                dist = math.hypot(px - ax, py - ay)
            else:
                dist = abs((bx - ax) * (ay - py) - (ax - px) * (by - ay)) / length
            if dist > best:
                best, at = dist, k
        if at is not None and best > tolerance:
            keep[at] = True
            stack += [(a, at), (at, b)]
    return [p for p, k in zip(points, keep) if k]


def profiles(rings):
    """Each ring's profile `[(d, h)]`, from the buried front over the skin to the buried back,
    closed underneath. The skin is sampled where its offset along the ring's direction from the
    world origin is a multiple of `FACE_GRID` up the face and `SHOULDER_GRID` behind it, so
    neighbouring rings share their samples, plus either side of every lane or fall edge."""
    for ring in rings:
        k_front = next(k for k, d in enumerate(ring.d) if d >= ring.front)
        k_back = max(k for k, d in enumerate(ring.d) if d <= ring.back)
        span = range(k_front, k_back + 1)
        rim = max(ring.skin[k] for k in span)
        crest = next((ring.d[k] for k in span if ring.skin[k] >= rim - 0.4), ring.d[k_back])
        shift = ring.x * ring.nx + ring.z * ring.nz
        wanted = {ring.d[k_front], ring.d[k_back]}
        for grid, a, b in ((FACE_GRID, ring.d[k_front], crest + 1.0),
                           (SHOULDER_GRID, crest + 1.0, ring.d[k_back])):
            k = math.ceil((a + shift) / grid)
            while k * grid - shift < b:
                wanted.add(round(k * grid - shift, 6))
                k += 1
        for k in range(k_front, k_back):
            edge = (ring.keep[k] is None) != (ring.keep[k + 1] is None) or \
                   (ring.fall[k] is None) != (ring.fall[k + 1] is None)
            if edge:
                wanted.add(ring.d[k])
                wanted.add(ring.d[k + 1])

        def skin_at(d):
            k = min(max(int(math.floor((d - ring.d[0]) / DENSE + 1e-9)), k_front), k_back - 1)
            f = min(1.0, max(0.0, (d - ring.d[k]) / DENSE))
            return ring.skin[k] + (ring.skin[k + 1] - ring.skin[k]) * f

        line = [(d, skin_at(d)) for d in sorted(wanted) if ring.d[k_front] <= d <= ring.d[k_back]]
        foot = (ring.d[k_front] - 0.3, ring.lo[k_front] - UNDER)
        back = (ring.d[k_back] + 0.4, ring.lo[k_back] - UNDER)
        line = [foot] + line + [back]
        bottom = min(h for _, h in line) - BURY
        ring.profile = [(foot[0], bottom)] + line + [(back[0], bottom)]


def vertex(ring, d, h):
    return (ring.x + ring.nx * d, h, ring.z + ring.nz * d)


def zipper(a, b, along):
    """Triangles joining two open skin polylines `[(vertex index, position)]`, advancing by
    each point's position along `along` (the two rings' shared direction across the outline)
    and then along each line, so that features standing at the same place, such as a lane's
    edges, meet."""
    def keys(points):
        return [(round(p[1][0] * along[0] + p[1][2] * along[1], 3), k / (len(points) - 1))
                for k, p in enumerate(points)]

    ka, kb = keys(a), keys(b)
    i = j = 0
    out = []
    while i < len(a) - 1 or j < len(b) - 1:
        if j == len(b) - 1 or (i < len(a) - 1 and ka[i + 1] <= kb[j + 1]):
            out.append((a[i][0], a[i + 1][0], b[j][0]))
            i += 1
        else:
            out.append((a[i][0], b[j + 1][0], b[j][0]))
            j += 1
    return out


def segment_mesh(rings, members):
    """One segment's closed mesh over its rings: the skin between neighbouring profiles, their
    buried front, back and bottom, and a cap at either end."""
    vertices = []
    lines = []
    for i in members:
        ring = rings[i]
        line = []
        for d, h in ring.profile:
            line.append((len(vertices), (d, h)))
            vertices.append(vertex(ring, d, h))
        lines.append(line)
    triangles = []
    for (a, b), (i, j) in zip(zip(lines, lines[1:]), zip(members, members[1:])):
        nx, nz = rings[i].nx + rings[j].nx, rings[i].nz + rings[j].nz
        length = math.hypot(nx, nz)
        along = (nx / length, nz / length)
        # Each profile is [bottom front, foot, … skin …, back, bottom back].
        skin_a = [(index, vertices[index]) for index, _ in a[1:-1]]
        skin_b = [(index, vertices[index]) for index, _ in b[1:-1]]
        triangles += zipper(skin_a, skin_b, along)
        quads = [(a[0][0], a[1][0], b[1][0], b[0][0]),
                 (a[-2][0], a[-1][0], b[-1][0], b[-2][0]),
                 (a[-1][0], a[0][0], b[0][0], b[-1][0])]
        for p, q, r, t in quads:
            triangles += [(p, q, r), (p, r, t)]
    for line in (lines[0], lines[-1]):
        polygon = [p for _, p in line]
        for tri in earclip(polygon):
            triangles.append(tuple(line[k][0] for k in tri))
    triangles = [t for t in triangles if len(set(t)) == 3]
    triangles = orient(vertices, triangles)
    return vertices, triangles


def segments(rings):
    """Ring index groups of about `SEGMENT` m along the outline, split where the grip changes
    and where the band leaves off; each shares its first and last ring with its neighbours."""
    n = len(rings)
    banded = [not ring.empty for ring in rings]
    if all(banded):
        # Start at a grip change, if any, so no segment straddles one.
        start = next((i for i in range(n) if rings[i].sheer != rings[i - 1].sheer), 0)
        runs = [[(start + k) % n for k in range(n)] + [start]]
    else:
        start = next(i for i in range(n) if banded[i] and not banded[i - 1])
        runs, current = [], []
        for k in range(n):
            i = (start + k) % n
            if banded[i]:
                current.append(i)
            elif current:
                runs.append(current)
                current = []
        if current:
            runs.append(current)
    length = lambda a, b: math.dist((rings[a].x, rings[a].z), (rings[b].x, rings[b].z))
    groups = []
    for run in runs:
        pieces = []
        current = [run[0]]
        kind = rings[run[0]].sheer
        travelled = 0.0
        for i in run[1:]:
            travelled += length(current[-1], i)
            current.append(i)
            if rings[i].sheer != kind and i != run[-1]:
                current.pop()
                short = sum(length(a, b) for a, b in zip(current, current[1:])) < SEGMENT / 2
                if short and pieces and rings[pieces[-1][-1]].sheer == kind:
                    # A short piece before a grip change joins the one before it.
                    pieces[-1] = pieces[-1] + current[1:]
                else:
                    pieces.append(current)
                current, travelled = [current[-1], i], length(current[-1], i)
                kind = rings[i].sheer
            elif travelled >= SEGMENT:
                pieces.append(current)
                current, travelled = [i], 0.0
        if len(current) > 1:
            pieces.append(current)
        # Fold a short last piece into its predecessor.
        last = sum(length(a, b) for a, b in zip(pieces[-1], pieces[-1][1:]))
        if len(pieces) > 1 and last < SEGMENT / 2 and \
                rings[pieces[-1][-1]].sheer == rings[pieces[-2][0]].sheer:
            pieces[-2] = pieces[-2] + pieces[-1][1:]
            pieces.pop()
        groups += pieces
    return groups


def rasterise(meshes, cell=0.25):
    """The highest upward face over each `cell` square, from every segment."""
    top = {}
    for vertices, triangles in meshes:
        for a, b, c in triangles:
            p = [vertices[a], vertices[b], vertices[c]]
            area = (p[1][0] - p[0][0]) * (p[2][2] - p[0][2]) - (p[1][2] - p[0][2]) * (p[2][0] - p[0][0])
            if area >= -1e-9:
                continue  # facing down or edge-on (outward winding faces up when negative)
            xs = [q[0] for q in p]
            zs = [q[2] for q in p]
            for cx in range(math.floor(min(xs) / cell), math.floor(max(xs) / cell) + 1):
                for cz in range(math.floor(min(zs) / cell), math.floor(max(zs) / cell) + 1):
                    x, z = (cx + 0.5) * cell, (cz + 0.5) * cell
                    wa = (p[1][0] - x) * (p[2][2] - z) - (p[1][2] - z) * (p[2][0] - x)
                    wb = (p[2][0] - x) * (p[0][2] - z) - (p[2][2] - z) * (p[0][0] - x)
                    wc = area - wa - wb
                    if wa > 0 or wb > 0 or wc > 0:
                        continue
                    y = (wa * p[0][1] + wb * p[1][1] + wc * p[2][1]) / area
                    key = (cx, cz)
                    if y > top.get(key, -math.inf):
                        top[key] = y
    return top


def check(meshes, rings, lattice, lanes, cell=0.25):
    """Every stepped terrain cell in the band's reach is under its skin, and no lane or channel
    is: returns the worst misses for the report."""
    top = rasterise(meshes, cell)
    poking, covered = [], []
    for (cx, cz), y in top.items():
        x, z = (cx + 0.5) * cell, (cz + 0.5) * cell
        lo, hi = lattice.bounds(x, z)
        lane = lanes.height(x, z, margin=-0.1)
        if lane is not None or in_channel(x, z) or lanes.near_unit(x, z, -0.1) \
                or lanes.access(x, z, lo, hi) \
                or lanes.near_road(x, z) and (lo >= ROAD_RIM or not lattice.stepped(x, z)):
            if y > lo - 0.02:
                covered.append((y - lo, x, z))
        elif y < hi and not in_fall(x, z):
            poking.append((hi - y, x, z))
    # Stepped cells near the outline that the band does not reach at all.
    bare = []
    seen = set()
    for ring in rings:
        for k, d in enumerate(ring.d):
            if not (-3 <= d <= 5) or not ring.step[k]:
                continue
            x, z = ring.x + ring.nx * d, ring.z + ring.nz * d
            key = (math.floor(x / cell), math.floor(z / cell))
            if key in seen:
                continue
            seen.add(key)
            if key not in top and lanes.height(x, z, -0.1) is None and not in_channel(x, z) \
                    and not in_fall(x, z):
                span = 1.0
                if lattice.stepped(x, z):
                    bare.append((span, x, z))
    return sorted(poking, reverse=True), sorted(covered, reverse=True), sorted(bare, reverse=True)


def mesh_entry(id, grip, vertices, triangles):
    xs = [v[0] for v in vertices]
    zs = [v[2] for v in vertices]
    centre = [round((min(xs) + max(xs)) / 2 * UNITS), 0, round((min(zs) + max(zs)) / 2 * UNITS)]
    units = [[round(x * UNITS) - centre[0], round(y * UNITS), round(z * UNITS) - centre[2]]
             for x, y, z in vertices]
    kept = []
    for a, b, c in triangles:
        u = [units[b][i] - units[a][i] for i in range(3)]
        v = [units[c][i] - units[a][i] for i in range(3)]
        cross = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
        if any(cross):
            kept.append([a, b, c])
    return {'id': id, 'position': centre, 'vertices': units, 'triangles': kept, 'grip': grip}


def ivy(rings, groups, meshes, ids):
    """Climbing ivy down a few plain climbable faces: a vine top under the crest and 2 m strips
    down to the foot, each on the face's collision (as `hollow_cliffs.py`'s)."""
    out = []
    owner = {}
    for (id, grip), members in zip(ids, groups):
        for i in members[1:-1]:
            owner[i] = (id, grip)
    column = 0
    for i in range(IVY_EVERY // 2, len(rings), IVY_EVERY):
        ring = rings[i]
        if i not in owner or owner[i][1] != 'rock':
            continue
        if any(ring.keep[k] is not None or ring.fall[k] is not None for k in range(len(ring.d))
               if ring.front <= ring.d[k] <= ring.back):
            continue
        id = owner[i][0]
        vertices, triangles = meshes[id]
        floor = ring.lo[next(k for k, d in enumerate(ring.d) if d >= ring.front)]
        rim = max(h for _, h in ring.profile)
        cap = rim - 0.6 - VINE_TOP_HEIGHT
        if cap - floor < 4:
            continue
        nx, nz = ring.nx, ring.nz
        yaw = round(math.atan2(-nx, -nz) / math.tau * 65536) % 65536
        sx, sz = ring.x + nx * (ring.front - 6), ring.z + nz * (ring.front - 6)
        pieces = [('vine-top', cap, VINE_TOP_HEIGHT)]
        k = 1
        while cap - VINE_PITCH * k + VINE_PITCH > floor + 1.2:
            pieces.append(('vine-strip', cap - VINE_PITCH * k, VINE_PITCH))
            k += 1
        placed = []
        for index, (part, base, height) in enumerate(pieces):
            reach = None
            for f in (0.1, 0.5, 0.9):
                y = max(base + height * f, floor + 0.2)
                hit = ray((vertices, triangles), (sx, y, sz), (nx, 0, nz))
                if hit is not None:
                    reach = hit if reach is None else max(reach, hit)
            if reach is None:
                placed = None
                break
            at = (sx + nx * (reach - VINE_OUT), base, sz + nz * (reach - VINE_OUT))
            placed.append({
                'id': f'basin-ivy-{column}-{index}',
                'asset': f'climbing.kit.{part}',
                'position': [round(at[0] * UNITS), round(at[1] * UNITS), round(at[2] * UNITS)],
                'yaw': yaw,
                'scale': [1000, 1000, 1000],
            })
        if placed:
            out += placed
            column += 1
    return out


def write(entries, placed):
    COLLISION.write_text(json.dumps(entries, separators=(',', ':')) + '\n')
    lines = ['{', '  "version": 1,', '  "area": "village-basin-rim",', '  "placements": [']
    lines += ['    ' + json.dumps(p, separators=(',', ':')) + (',' if i + 1 < len(placed) else '')
              for i, p in enumerate(placed)]
    lines += ['  ]', '}']
    DRESSING_OUT.write_text('\n'.join(lines) + '\n')


def review(meshes, ids, placed, lattice):
    """The Blender review collection. Game (x, y, z) metres are Blender (x, -z, y)."""
    root = bpy.data.collections.get(NAME) or bpy.data.collections.new(NAME)
    if root.name not in bpy.context.scene.collection.children:
        bpy.context.scene.collection.children.link(root)
    for child in list(root.children):
        for obj in list(child.objects):
            bpy.data.objects.remove(obj, do_unlink=True)
        bpy.data.collections.remove(child)
    for obj in list(root.objects):
        bpy.data.objects.remove(obj, do_unlink=True)

    def material(name, colour):
        mat = bpy.data.materials.get(name) or bpy.data.materials.new(name)
        mat.use_nodes = True
        shader = next(n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED')
        shader.inputs['Base Color'].default_value = (*colour, 1)
        shader.inputs['Roughness'].default_value = 0.92
        mat.diffuse_color = (*colour, 1)
        return mat

    tints = {'rock': material('Basin band climbable (1195 4)', (0.85, 0.45, 0.15)),
             'sheer': material('Basin band sheer (1195 4)', (0.2, 0.4, 0.85))}
    grass = material('Basin band ground (1195 4)', (0.30, 0.34, 0.22))
    leaf = material('Basin band ivy (1195 4)', (0.15, 0.45, 0.12))

    def child(label):
        collection = bpy.data.collections.new(f'{NAME} {label}')
        root.children.link(collection)
        return collection

    def add(collection, name, vertices, faces, mat):
        data = bpy.data.meshes.new(name)
        data.from_pydata([(x, -z, y) for x, y, z in vertices], [], faces)
        data.update()
        for polygon in data.polygons:
            polygon.use_smooth = False
        data.materials.append(mat)
        obj = bpy.data.objects.new(name, data)
        obj.color = mat.diffuse_color
        collection.objects.link(obj)
        return obj

    cols, rows = lattice.columns, lattice.rows
    ground = [(lattice.ox + c * lattice.step, lattice.h[r * cols + c], lattice.oz + r * lattice.step)
              for r in range(rows) for c in range(cols)]
    faces = [(r * cols + c, r * cols + c + 1, (r + 1) * cols + c + 1, (r + 1) * cols + c)
             for r in range(rows - 1) for c in range(cols - 1)]
    add(child('terrain lattice'), 'Basin terrain (4 m lattice)', ground, faces, grass)
    band = child('segments')
    for id, grip in ids:
        vertices, triangles = meshes[id]
        obj = add(band, f'Basin rim {id} ({grip})', vertices, triangles, tints[grip])
        obj['collider_id'] = id
        obj['grip'] = grip
    dressing = child('ivy')
    for p in placed:
        x, y, z = (v / UNITS for v in p['position'])
        turn = p['yaw'] / 65536 * math.tau
        height = VINE_TOP_HEIGHT if p['asset'].endswith('vine-top') else VINE_PITCH
        sx, sz = math.cos(turn) * 0.5, -math.sin(turn) * 0.5
        quad = [(x - sx, y, z - sz), (x + sx, y, z + sz), (x + sx, y + height, z + sz),
                (x - sx, y + height, z - sz)]
        add(dressing, p['id'], quad, [(0, 1, 2, 3)], leaf)


def main():
    context = json.loads(CONTEXT.read_text())
    rings, lattice, lanes, wide, smooth = build_rings(context)
    band_extent(rings, wide, smooth)
    skins(rings, wide, smooth)
    plain_faces(rings, wide, smooth)
    shape(rings)
    profiles(rings)
    groups = segments(rings)
    ids = [(FIRST_ID + k, 'sheer' if rings[g[len(g) // 2]].sheer else 'rock') for k, g in enumerate(groups)]
    meshes = {}
    for (id, grip), members in zip(ids, groups):
        vertices, triangles = segment_mesh(rings, members)
        check_closed(id, triangles)
        meshes[id] = (vertices, triangles)
    poking, covered, bare = check(list(meshes.values()), rings, lattice, lanes)
    placed = ivy(rings, groups, meshes, ids)
    entries = [mesh_entry(id, grip, *meshes[id]) for id, grip in ids]
    write(entries, placed)
    count = sum(len(e['triangles']) for e in entries)
    length = rings[-1].s + math.dist((rings[-1].x, rings[-1].z), (rings[0].x, rings[0].z))
    print(f'Wrote {len(entries)} rim segments ({count} triangles, {length:.0f} m of outline) into '
          f'{COLLISION.relative_to(ROOT)} and {len(placed)} ivy placements into '
          f'{DRESSING_OUT.relative_to(ROOT)}')
    for (id, grip), members in zip(ids, groups):
        a, b = rings[members[0]], rings[members[-1]]
        span = sum(math.dist((rings[p].x, rings[p].z), (rings[q].x, rings[q].z))
                   for p, q in zip(members, members[1:]))
        print(f'  {id} {grip:5} {span:5.1f} m  ({a.x:6.1f}, {a.z:6.1f}) -> ({b.x:6.1f}, {b.z:6.1f})')
    print(f'terrain over the skin: {len(poking)} cells, worst {poking[:3]}')
    print(f'skin over keep-clear ground: {len(covered)} cells, worst {covered[:3]}')
    print(f'stepped cells the band misses: {len(bare)}, worst {bare[:3]}')
    if bpy is not None:
        review(meshes, ids, placed, lattice)
    return meshes, rings


if __name__ == "__main__":
    main()
