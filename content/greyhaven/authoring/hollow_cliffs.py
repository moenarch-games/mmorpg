#!/usr/bin/env python3
"""Authoring source: the Redbrand quarry's one continuous horseshoe wall (#1195 part 3b).

Owner decision #1195 part 3b (2026-11-24, approved from the mock-up "Redbrand quarry mock-up
(1195 3b)"): the seven ridges round the Redbrand camp give way to one worked quarry. A single
horseshoe wall, its rim 11-15 m above a gravel floor of about 38 x 42 m centred at (0, -113),
encloses the camp. One 12 m gateway cuts 8 m through the rim on the west side, between two
natural rock spurs, so the Outpost to the south sees the closed south wall. The north and south
walls are worked faces: 4.4 m vertical cuts with 2.5 m benches, broken by scree cones, and
climbable. The east wall is one smooth, sheer cut face over the old mine's adit. The rim is a
grassy shoulder and the outside falls away as natural rock buttresses.

The wall is generated here as rings: a profile (offset out from the inner foot, height above the
11 m datum) at every sample of the inner outline, blended between a benched cut face, the sheer
face, natural rock and a scree cone. It is split into nine closed collision meshes of about
10-20 m (collider IDs 400-408), each carrying its own `"grip": "rock" | "sheer"`. Neighbouring
segments on the arc share their seam ring, so a climber crosses a seam on one continuous face.
The gateway's spurs are separate meshes that overlap the arc at the gateway's inner corners;
there each surface is lowered to the other's rising face, so the corners meet in a mitre.

The rope (`greyhaven_vale::ROPES[0]`) hangs down the south wall at x = 3.5 m: there the inner
foot runs straight at z = -92.6 m, the face is upright for `ROPE_FACE` m either side and the rim
is a level landing at `ROPE_TOP`, the highest point of its segment (406), so the climber steps
off the rope onto it.

Run it headless with `python3` (it needs nothing else): it replaces meshes 400-408 in
`crates/mmorpg-core/content/collision/redbrand.json` (the cave's meshes, authored by
`redbrand.py`, stay byte for byte) and writes the ivy on a few of its cuts into
`content/greyhaven/dressing/hollow-cliffs.json`. Every height is measured from the 11 m datum
and buried 1.2 m, so the output never depends on the drawn ground and regenerates byte for byte.

In Blender (5.2+; the Python console, `__file__` set to this file's path) it also draws the
wall in the collection `Redbrand quarry (1195 3b) built`: orange where climbable, blue where
sheer, one object per collider, with the ivy, and the ground round it when
`artifacts/hollow/site.json` exists:

    cargo run -p mmorpg-scenery --example export-site-context -- \\
        --centre 0,-11400 --half 5200,4400 > artifacts/hollow/site.json

Rerunning replaces only that collection's children.
"""
import json
import math
import pathlib

try:
    import bpy
except ImportError:  # headless export
    bpy = None

ROOT = pathlib.Path(__file__).resolve().parents[3]
CONTEXT = ROOT / 'artifacts/hollow/site.json'
COLLISION = ROOT / 'crates/mmorpg-core/content/collision/redbrand.json'
DRESSING_OUT = ROOT / 'content/greyhaven/dressing/hollow-cliffs.json'
NAME = 'Redbrand quarry (1195 3b) built'
UNITS = 100
# Every mesh in redbrand.json stands on the Hollow's 11 m datum.
DATUM = 1100

# The quarry floor's inner outline: a rounded box (superellipse) centred at (CX, CZ) metres.
CX, CZ = 0.0, -113.0
AX, BZ = 19.0, 21.0
ROUND = 0.42
# The gateway: 12 m wide on the west side, its spurs' outer ends at x = GATE_OUT. Beyond them
# the gateway's face line runs on to `GATE_TAIL` as a low, vanishing face, which keeps the
# quarry's outer slopes from standing up as a wall beside the mouth.
MOUTH_HALF = 6.0
GATE_OUT = -28.0
GATE_TAIL = -38.0
# Rings about this far apart along the inner foot (m).
SPACING = 0.9
DENSE = 8192

# The rope (ROPES[0]): down the south wall's inner face at x = ROPE_X. The inner foot is a
# straight cut at z = ROPE_FACE_Z wherever the round outline would lie south of it.
ROPE_X = 3.5
ROPE_FACE_Z = -92.6
ROPE_FACE = 2.0
ROPE_PLATEAU = 4.0
ROPE_TOP = 13.0

# Profiles: (offset out from the inner foot, height above the floor), from the buried inner
# foot over the face, the rim and the outer slope down to the buried outer foot. All the same
# length, blended per ring.
BENCH = [(-0.8, -1.2), (0.0, 0.0), (0.35, 4.4), (2.9, 4.5), (3.2, 8.8), (5.8, 8.9), (6.1, 13.0),
         (8.5, 13.3), (11.5, 11.8), (15.0, 6.0), (18.5, -1.2)]
SHEER = [(-0.6, -1.2), (0.0, 0.0), (0.25, 4.0), (0.5, 7.0), (0.75, 9.8), (1.0, 12.4), (1.25, 15.0),
         (4.0, 15.3), (8.0, 13.2), (14.0, 6.0), (18.0, -1.2)]
NATURAL = [(-1.0, -1.2), (0.0, 0.3), (1.4, 3.4), (2.6, 5.8), (3.8, 8.0), (5.0, 9.8), (6.2, 11.4),
           (8.0, 11.9), (11.0, 10.2), (14.5, 4.6), (17.5, -1.2)]
# A scree cone where a bench has collapsed: a talus up to the first bench, then cut.
SCREE = [(-1.0, -1.2), (-3.5, 0.0), (-1.2, 2.2), (1.4, 4.4), (3.2, 8.6), (5.8, 8.9), (6.1, 13.0),
         (8.5, 13.3), (11.5, 11.8), (15.0, 6.0), (18.5, -1.2)]
K = len(BENCH)
# The rim's back edge: the end of the face's rising part.
RIM = 7
# Where benches have collapsed into scree cones: (x, z) on the inner foot, half width (m).
SCREES = [((-6.0, -134.0), 3.2), ((10.0, -133.0), 2.0), ((-15.8, -95.5), 2.4)]

# Climbing ivy (#621's kit) down a few cut faces: (x, z) on the inner foot, the bench it hangs
# from (0: the floor's cut, 1: the cut above the first bench).
IVY = [((-12.5, -133.6), 0), ((-11.5, -133.7), 0), ((-2.0, -134.0), 1), ((13.0, -132.0), 0),
       ((-13.0, -92.8), 0), ((-13.6, -92.9), 1)]
VINE_PITCH = 2.0
VINE_TOP_HEIGHT = 1.0
VINE_OUT = 0.05

# Segments: (collider ID, grip) in the outline's order; their extents are found in `segments`.
SEGMENTS = [(400, 'rock'), (401, 'rock'), (402, 'rock'), (403, 'rock'), (404, 'sheer'),
            (405, 'rock'), (406, 'rock'), (407, 'rock'), (408, 'rock')]
ROPE_SEGMENT = 406


def wave(*keys):
    """A fixed, smooth pseudo-random value in [-1, 1] for these keys."""
    a = sum(k * m for k, m in zip(keys, (2.713, 1.371, 0.719, 3.17)))
    b = sum(k * m for k, m in zip(keys, (0.931, -2.113, 0.373, -1.57)))
    return 0.6 * math.sin(a) + 0.4 * math.sin(b + 1.3)


def smooth(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


def superellipse(t):
    a = math.pi + t * math.tau
    s, c = math.sin(a), math.cos(a)
    x = CX + AX * math.copysign(abs(c) ** ROUND, c)
    z = CZ + BZ * math.copysign(abs(s) ** ROUND, s)
    # The rope's straight cut along the south wall.
    return x, min(z, ROPE_FACE_Z) if z > CZ else z


def resample(path, spacing):
    lengths = [0.0]
    for a, b in zip(path, path[1:]):
        lengths.append(lengths[-1] + math.dist(a, b))
    count = max(1, round(lengths[-1] / spacing))
    out = []
    j = 0
    for i in range(count + 1):
        want = lengths[-1] * i / count
        while j < len(lengths) - 2 and lengths[j + 1] < want:
            j += 1
        f = (want - lengths[j]) / max(1e-12, lengths[j + 1] - lengths[j])
        a, b = path[j], path[j + 1]
        out.append((a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f))
    return out


def outline():
    """The inner foot as `(part, points, normals)`: the gateway's north side (west to east), the
    arc round north, east and south, and the gateway's south side (east to west)."""
    dense = [superellipse(j / DENSE) for j in range(DENSE)]
    arc = [p for p in dense if not (p[0] < CX and abs(p[1] - CZ) < MOUTH_HALF)]
    # The kept arc starts on the gateway's north side and ends on its south side; pin its ends
    # to the gateway's faces.
    arc[0] = (arc[0][0], CZ - MOUTH_HALF)
    arc[-1] = (arc[-1][0], CZ + MOUTH_HALF)
    arc = resample(arc, SPACING)
    normals = []
    for i in range(len(arc)):
        a, b = arc[max(i - 2, 0)], arc[min(i + 2, len(arc) - 1)]
        tx, tz = b[0] - a[0], b[1] - a[1]
        length = math.hypot(tx, tz)
        nx, nz = tz / length, -tx / length
        x, z = arc[i]
        if nx * (x - CX) + nz * (z - CZ) < 0:
            nx, nz = -nx, -nz
        if z >= ROPE_FACE_Z - 1e-6:
            # The rope's straight cut faces due north.
            nx, nz = 0.0, 1.0
        normals.append((nx, nz))
    north = resample([(GATE_OUT, CZ - MOUTH_HALF), (arc[0][0], CZ - MOUTH_HALF)], SPACING)
    south = resample([(arc[-1][0], CZ + MOUTH_HALF), (GATE_OUT, CZ + MOUTH_HALF)], SPACING)
    return [('north', north, [(0.0, -1.0)] * len(north)), ('arc', arc, normals),
            ('south', south, [(0.0, 1.0)] * len(south))]


def weights(part, x, z):
    """How much of the benched cut, the sheer face, natural rock and scree a ring is."""
    if part != 'arc':
        return 0.0, 0.0, 1.0, 0.0
    ang = math.degrees(math.atan2(-(z - CZ) / BZ, (x - CX) / AX))   # 0 east, 90 north
    sheer = min(1.0, max(0.0, 1 - abs(ang) / 34) * 1.7)
    natural = min(1.0, max(0.0, 1 - (180 - abs(ang)) / 60) * 1.7)
    scree = 0.0
    for (sx, sz), half in SCREES:
        d = math.dist((x, z), (sx, sz)) / half
        scree += max(0.0, 1 - d * d)
    scree = min(1.0, scree * 1.5) * (1 - sheer) * (1 - natural)
    bench = max(0.0, 1 - sheer - natural - scree)
    total = bench + sheer + natural + scree
    return bench / total, sheer / total, natural / total, scree / total


def outer_scale(x, z):
    dz, dx = (z - CZ) / BZ, (x - CX) / AX
    s = 1.0
    if dz > 0:
        s -= 0.6 * min(1, 1.25 * dz)  # south: keep off the basin rim and the Hollow Road
    if dx > 0:
        s -= 0.12 * min(1, dx)     # east: keep off the cave mound
    return s


def rope_side(x, z):
    """Whether `(x, z)` on the inner foot is on the rope's straight cut."""
    return z >= ROPE_FACE_Z - 1e-6


def ring(part, index, count, x, z, n, u):
    """One ring: its base point on the inner foot and its profile `[(offset, height)]`."""
    nx, nz = n
    wb, ws, wn, wsc = weights(part, x, z)
    rope = part == 'arc' and rope_side(x, z)
    # Plan irregularity: natural rock wanders, cut faces stay straighter; the gateway stays a
    # clean cutting and the rope's cut is straight.
    plan = (2.2 * wn + 0.9 * wb + 0.9 * wsc + 0.25 * ws) * (0.7 * wave(u * 9.1, 1) + 0.3 * wave(u * 23.0, 2))
    if part != 'arc':
        plan *= 0.35
    if z > CZ:
        plan *= smooth(0.0, 3.0, ROPE_FACE_Z - z) if rope else smooth(0.0, 3.0, abs(x) - 6.0)
    bx, bz = x + nx * plan, z + nz * plan
    profile = []
    blended = []
    lobe = 2.6 * max(0.0, wave(u * 11.0, 13))                  # buttress lobes outside
    for k in range(K):
        o = wb * BENCH[k][0] + ws * SHEER[k][0] + wn * NATURAL[k][0] + wsc * SCREE[k][0]
        h = wb * BENCH[k][1] + ws * SHEER[k][1] + wn * NATURAL[k][1] + wsc * SCREE[k][1]
        blended.append(o)
        rough = wn + 0.3 * wb + 0.6 * wsc
        if 1 <= k <= 6:
            h += rough * 0.6 * wave(index * 0.23 + u * 31, k)
            o += rough * 0.55 * wave(index * 0.19 + u * 37, k, 3)
            h += (wb + wsc) * 0.6 * wave(u * 5.0, 2)          # benches jog slowly
        if 6 <= k <= 9:
            h += 1.4 * wave(u * 7.0, 7) * (1 - 0.7 * ws)      # crest saddles and knolls
        if k == RIM:
            # The rim keeps its width behind the face's top.
            o = profile[RIM - 1][0] + (o - blended[RIM - 1]) + lobe / 4
        if k > RIM:
            # The outer slope falls from the rim's back edge, its reach shortened toward the
            # basin rim and the cave mound.
            reach = o - blended[RIM] + (lobe * (k - RIM) / 4 if k < K - 1 else lobe * 3 / 4)
            o = profile[RIM][0] + reach * outer_scale(x, z)
        if RIM <= k <= 9:
            o += 0.8 * wave(u * 53.0, k, 5) * (0.3 if k == RIM else 1.0)
            h += 0.9 * wave(u * 61.0, k, 5)
        profile.append([o, h])
    # The gateway's spurs drop toward their outer ends.
    if part != 'arc':
        end = abs(x - GATE_OUT)
        f = 0.45 + 0.55 * smooth(0.0, 5.4, end)
        profile = [[o, h] if k == 0 else [o * (0.7 + 0.3 * f), max(h * f, -1.2)]
                   for k, (o, h) in enumerate(profile)]
    if rope:
        # The rope's cut: nothing reaches north of the foot, the top never rises above the
        # landing, the face is upright beside the rope and the landing level round it.
        for p in profile:
            p[0] = max(p[0], 0.0)
            p[1] = min(p[1], ROPE_TOP)
        if abs(x - ROPE_X) <= ROPE_FACE:
            for k in range(1, RIM):
                profile[k][0] = 0.0
        if abs(x - ROPE_X) <= ROPE_PLATEAU:
            profile[RIM - 1][1] = profile[RIM][1] = ROPE_TOP
    # Nothing overhangs: up the face each point stands at least as far back and as high as the
    # one under it; down the outer slope each lies further out and lower.
    for k in range(2, RIM + 1):
        profile[k][0] = max(profile[k][0], profile[k - 1][0])
        profile[k][1] = max(profile[k][1], profile[k - 1][1] + 0.05)
    for k in range(RIM + 1, K):
        profile[k][0] = max(profile[k][0], profile[k - 1][0] + 0.3)
        profile[k][1] = min(profile[k][1], profile[k - 1][1] - 0.2)
    profile[0][1] = profile[K - 1][1] = -1.2
    if rope:
        profile[RIM][1] = min(profile[RIM][1], ROPE_TOP)
    return (bx, bz), n, [tuple(p) for p in profile], (x, z)


def build_rings():
    parts = outline()
    total = sum(len(points) for _, points, _ in parts)
    rings = []
    index = 0
    for part, points, normals in parts:
        for i, ((x, z), n) in enumerate(zip(points, normals)):
            rings.append((part,) + ring(part, index, len(points), x, z, n, index / total))
            index += 1
    return rings


def height_on(profile, d):
    """The height of a ring's rising face `d` metres out from its foot, or None past its rim."""
    if d > profile[RIM][0]:
        return None
    if d <= profile[1][0]:
        return profile[1][1]
    for k in range(1, RIM):
        (o0, h0), (o1, h1) = profile[k], profile[k + 1]
        if o0 <= d <= o1:
            return h0 if o1 - o0 < 1e-9 else h0 + (h1 - h0) * (d - o0) / (o1 - o0)
    return profile[RIM][1]


def height_across(profile, d):
    """The height of a ring's whole profile `d` metres out from its foot: up its face, over
    its rim and down its outer slope."""
    if d <= profile[1][0]:
        return profile[1][1]
    for k in range(1, K - 1):
        (o0, h0), (o1, h1) = profile[k], profile[k + 1]
        if o0 <= d <= o1:
            return h0 if o1 - o0 < 1e-9 else h0 + (h1 - h0) * (d - o0) / (o1 - o0)
    return profile[K - 1][1]


def faces_near(rings, members):
    """The inner faces of the rings in `members` as a polyline: (base, normal, profile)."""
    return [(rings[i][1], rings[i][2], rings[i][3]) for i in members]


def over_face(face, point, height, pick):
    """`height(profile, distance)` of `face` over `point`, where `point` lies on the face's
    rock side, combined over the face's pieces by `pick` (None where it lies over none). The
    distance is to each piece's nearest point, so the field has no gaps where pieces turn."""
    best = None
    (px, pz) = point
    for (a, na, pa), (b, nb, pb) in zip(face, face[1:]):
        ex, ez = b[0] - a[0], b[1] - a[1]
        length2 = ex * ex + ez * ez
        t = min(1.0, max(0.0, ((px - a[0]) * ex + (pz - a[1]) * ez) / length2))
        fx, fz = a[0] + ex * t, a[1] + ez * t
        nx, nz = na[0] + (nb[0] - na[0]) * t, na[1] + (nb[1] - na[1]) * t
        side = (px - fx) * nx + (pz - fz) * nz
        d = math.copysign(math.hypot(px - fx, pz - fz), side)
        if d < 0:
            # A point a little in front of a face (its foot wanders) is held at its foot.
            if height is not height_cap or d < -1.5:
                continue
            d = 0.0
        ha, hb = height(pa, d), height(pb, d)
        if ha is None or hb is None:
            continue
        h = ha + (hb - ha) * t
        best = h if best is None else pick(best, h)
    return best


def height_cap(profile, d):
    """The most rock a face allows `d` metres out: its rising part, then steeply more behind
    its rim, where other rock may rise beyond it."""
    rim = height_on(profile, d)
    return rim if rim is not None else profile[RIM][1] + 2.0 * (d - profile[RIM][0])


def profile_across(profile, d):
    """A ring's rock `d` metres out, or None past its buried outer foot."""
    return None if d > profile[K - 1][0] else height_across(profile, d)


def segments(rings):
    """Each segment's ring indices, in `SEGMENTS` order."""
    north = [i for i, r in enumerate(rings) if r[0] == 'north']
    arc = [i for i, r in enumerate(rings) if r[0] == 'arc']
    south = [i for i, r in enumerate(rings) if r[0] == 'south']
    sheer = [i for i in arc if weights('arc', *rings[i][4])[1] >= 0.5]
    rope = [i for i in arc if rope_side(*rings[i][4]) and rings[i][4][1] > CZ]
    s0, s1 = sheer[0], sheer[-1]
    r0, r1 = rope[0], rope[-1]
    a0, a1 = arc[0], arc[-1]

    def split(first, last, pieces):
        cuts = [round(first + (last - first) * p / pieces) for p in range(pieces + 1)]
        return [list(range(cuts[p], cuts[p + 1] + 1)) for p in range(pieces)]

    upper = split(a0, s0, 3)
    return ([north] + upper + [list(range(s0, s1 + 1)), list(range(s1, r0 + 1)),
                               list(range(r0, r1 + 1)), list(range(r1, a1 + 1)), south])


def vertex(ring_, k):
    (bx, bz), (nx, nz), profile = ring_[1], ring_[2], ring_[3]
    o, h = profile[k]
    return (bx + nx * o, h, bz + nz * o)


def mitre(rings, groups):
    """Where a gateway spur and the arc's end overlap at the gateway's inner corners, each
    point takes the height of the face it lies nearest, as the rock between two faces does, so
    the corners meet in a mitre rather than one standing up through the other. Beyond the
    spurs' outer ends, the arc's outer slope is kept down to a low face along the gateway's
    line. Returns every ring's vertices."""
    north, south = groups[0], groups[-1]
    arc_start = groups[1][:30]
    arc_end = groups[-2][-30:]

    def tail(members, sign):
        # The spur's face line continued past its outer end as a low face.
        base = rings[members[0] if sign < 0 else members[-1]]
        profile = [(o, max(-1.2, h * 0.5)) for o, h in base[3]]
        z = CZ + sign * MOUTH_HALF
        steps = round((GATE_OUT - GATE_TAIL) / SPACING)
        return [((GATE_TAIL + (GATE_OUT - GATE_TAIL) * s / steps, z), (0.0, float(sign)), profile)
                for s in range(steps + 1)]

    faces = {}
    for i in north:
        faces[i] = faces_near(rings, arc_start)
    for i in south:
        faces[i] = faces_near(rings, arc_end)
    for i in arc_start:
        faces[i] = faces_near(rings, north)
    for i in arc_end:
        faces[i] = faces_near(rings, south[::-1])
    tails = {i: tail(north, -1) for i in arc_start}
    tails.update({i: tail(south, 1) for i in arc_end})
    out = []
    for i, r in enumerate(rings):
        points = []
        for k in range(K):
            x, h, z = vertex(r, k)
            if 1 <= k <= K - 2 and (i in faces or i in tails):
                # Rock is where any face's profile puts it (the faces' masses join behind
                # their rims; in front of its own rim a point keeps its own face) ...
                if i in faces and r[3][k][0] > r[3][RIM][0]:
                    other = over_face(faces[i], (x, z), profile_across, max)
                    if other is not None:
                        h = max(h, other)
                # ... but never above a face's rising part in front of its rim.
                for face in (faces.get(i), tails.get(i)):
                    if face:
                        cap = over_face(face, (x, z), height_cap, min)
                        if cap is not None:
                            h = min(h, cap)
                h = max(h, -1.0)
            points.append((x, h, z))
        out.append(points)
    return out


def earclip(polygon):
    """Triangles over a simple polygon's indices (counter-clockwise or clockwise)."""
    area = sum(polygon[i][0] * polygon[(i + 1) % len(polygon)][1]
               - polygon[(i + 1) % len(polygon)][0] * polygon[i][1] for i in range(len(polygon)))
    sign = 1 if area > 0 else -1
    left = list(range(len(polygon)))
    out = []

    def cross(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

    guard = 0
    while len(left) > 3:
        guard += 1
        if guard > 10000:
            raise SystemExit('a cap polygon cannot be triangulated')
        for j in range(len(left)):
            i0, i1, i2 = left[j - 1], left[j], left[(j + 1) % len(left)]
            a, b, c = polygon[i0], polygon[i1], polygon[i2]
            if cross(a, b, c) * sign <= 1e-12:
                continue
            if any(cross(a, b, polygon[m]) * sign >= 0 and cross(b, c, polygon[m]) * sign >= 0
                   and cross(c, a, polygon[m]) * sign >= 0 for m in left if m not in (i0, i1, i2)):
                continue
            out.append((i0, i1, i2))
            left.pop(j)
            break
        else:
            raise SystemExit('a cap polygon is not simple')
    out.append(tuple(left))
    return out


def segment_mesh(rings, points, members):
    """One segment's closed mesh: `(vertices in metres, triangles)`."""
    n = len(members)
    vertices = [points[i][k] for i in members for k in range(K)]

    def index(j, k):
        return j * K + k

    triangles = []
    for j in range(n - 1):
        for k in range(K):
            k1 = (k + 1) % K
            a, b, c, e = index(j, k), index(j + 1, k), index(j + 1, k1), index(j, k1)
            if math.dist(vertices[a], vertices[c]) <= math.dist(vertices[b], vertices[e]):
                triangles += [(a, b, c), (a, c, e)]
            else:
                triangles += [(a, b, e), (b, c, e)]
    for j, flip in ((0, True), (n - 1, False)):
        r = rings[members[j]]
        (bx, bz), (nx, nz) = r[1], r[2]
        polygon = [((x - bx) * nx + (z - bz) * nz, y) for x, y, z in
                   (vertices[index(j, k)] for k in range(K))]
        try:
            cap = earclip(polygon)
        except SystemExit:
            raise SystemExit(f'ring {members[j]} cap is not simple: {[(round(o, 2), round(y, 2)) for o, y in polygon]}')
        for tri in cap:
            t = tuple(index(j, k) for k in tri)
            triangles.append(t)
    # Wind every side triangle and cap consistently: make each cap agree with the sides.
    triangles = orient(vertices, triangles)
    return vertices, triangles


def orient(vertices, triangles):
    """Flip triangles so that every edge is used once in each direction, then outward."""
    adjacency = {}
    for t, tri in enumerate(triangles):
        for e in ((tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])):
            adjacency.setdefault(frozenset(e), []).append(t)
    flipped = [None] * len(triangles)
    flipped[0] = False
    stack = [0]
    while stack:
        t = stack.pop()
        tri = triangles[t][::-1] if flipped[t] else triangles[t]
        for e in ((tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])):
            for other in adjacency[frozenset(e)]:
                if other == t or flipped[other] is not None:
                    continue
                o = triangles[other]
                directed = {(o[0], o[1]), (o[1], o[2]), (o[2], o[0])}
                # A neighbour agrees when it uses the shared edge the other way round.
                flipped[other] = e in directed
                stack.append(other)
    if any(f is None for f in flipped):
        raise SystemExit('a segment mesh is not connected')
    out = [t[::-1] if f else t for t, f in zip(triangles, flipped)]
    if volume(vertices, out) < 0:
        out = [t[::-1] for t in out]
    return out


def volume(vertices, triangles):
    total = 0.0
    for a, b, c in triangles:
        (ax, ay, az), (bx, by, bz), (cx, cy, cz) = vertices[a], vertices[b], vertices[c]
        total += ax * (by * cz - bz * cy) - ay * (bx * cz - bz * cx) + az * (bx * cy - by * cx)
    return total / 6


def check_closed(id, triangles):
    edges = {}
    for t in triangles:
        for e in ((t[0], t[1]), (t[1], t[2]), (t[2], t[0])):
            edges[e] = edges.get(e, 0) + 1
    for (a, b), count in edges.items():
        if count != 1 or edges.get((b, a)) != 1:
            raise SystemExit(f'segment {id} is not closed and consistently wound at edge {a}-{b}')


def mesh_entry(id, grip, vertices, triangles):
    units = [[round(x * UNITS), round(y * UNITS), round(z * UNITS)] for x, y, z in vertices]
    # Drop any triangle that rounding to units leaves without area.
    kept = []
    for a, b, c in triangles:
        u = [units[b][i] - units[a][i] for i in range(3)]
        v = [units[c][i] - units[a][i] for i in range(3)]
        cross = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
        if any(cross):
            kept.append([a, b, c])
    return {'id': id, 'position': [0, DATUM, 0], 'vertices': units, 'triangles': kept, 'grip': grip}


def ray(mesh, origin, direction):
    """The nearest hit along `direction` from `origin` (Möller-Trumbore), or None."""
    vertices, triangles = mesh
    best = None
    for a, b, c in triangles:
        p0, p1, p2 = vertices[a], vertices[b], vertices[c]
        e1 = [p1[i] - p0[i] for i in range(3)]
        e2 = [p2[i] - p0[i] for i in range(3)]
        p = [direction[1] * e2[2] - direction[2] * e2[1], direction[2] * e2[0] - direction[0] * e2[2],
             direction[0] * e2[1] - direction[1] * e2[0]]
        det = sum(e1[i] * p[i] for i in range(3))
        if abs(det) < 1e-9:
            continue
        t0 = [origin[i] - p0[i] for i in range(3)]
        u = sum(t0[i] * p[i] for i in range(3)) / det
        if not 0 <= u <= 1:
            continue
        q = [t0[1] * e1[2] - t0[2] * e1[1], t0[2] * e1[0] - t0[0] * e1[2], t0[0] * e1[1] - t0[1] * e1[0]]
        v = sum(direction[i] * q[i] for i in range(3)) / det
        if v < 0 or u + v > 1:
            continue
        t = sum(e2[i] * q[i] for i in range(3)) / det
        if t > 0 and (best is None or t < best):
            best = t
    return best


def nearest_ring(rings, x, z):
    return min(range(len(rings)), key=lambda i: math.dist(rings[i][1], (x, z)))


def ivy(rings, meshes, owner):
    """Climbing ivy down a few cut faces: a vine top under the cut's upper edge and 2 m strips
    down to its foot, each standing on the face's collision at the most sunken point of its
    height so no strip floats off the rock (as `crags.py`'s)."""
    out = []
    for column, ((x, z), bench) in enumerate(IVY):
        i = nearest_ring(rings, x, z)
        (bx, bz), (nx, nz), profile = rings[i][1], rings[i][2], rings[i][3]
        id = owner[i]
        if dict(SEGMENTS)[id] != 'rock':
            raise SystemExit(f'ivy column {column} is not on climbable rock')
        vertices, triangles = meshes[id]
        mesh = ([(vx, vy + DATUM / UNITS, vz) for vx, vy, vz in vertices], triangles)
        # The cut's foot and upper edge.
        foot, top = (profile[1][1], profile[2][1]) if bench == 0 else (profile[3][1], profile[4][1])
        floor = DATUM / UNITS + foot
        cap = DATUM / UNITS + top - 0.2 - VINE_TOP_HEIGHT
        yaw = round(math.atan2(-nx, -nz) / math.tau * 65536) % 65536
        # Out from the face toward the floor, from a point well inside the quarry.
        sx, sz = bx - nx * (8 + 3 * bench), bz - nz * (8 + 3 * bench)
        pieces = [('vine-top', cap, VINE_TOP_HEIGHT)]
        k = 1
        while cap - VINE_PITCH * k + VINE_PITCH > floor + 0.3:
            pieces.append(('vine-strip', cap - VINE_PITCH * k, VINE_PITCH))
            k += 1
        for index, (part, base, height) in enumerate(pieces):
            reach = None
            for f in (0.1, 0.5, 0.9):
                y = max(base + height * f, floor + 0.2)
                hit = ray(mesh, (sx, y, sz), (nx, 0, nz))
                if hit is not None:
                    reach = hit if reach is None else max(reach, hit)
            if reach is None:
                raise SystemExit(f'ivy column {column} misses the rock')
            at = (sx + nx * (reach - VINE_OUT), base, sz + nz * (reach - VINE_OUT))
            out.append({
                'id': f'quarry-ivy-{column}-{index}',
                'asset': f'climbing.kit.{part}',
                'position': [round(at[0] * UNITS), round(at[1] * UNITS), round(at[2] * UNITS)],
                'yaw': yaw,
                'scale': [1000, 1000, 1000],
            })
    return out


def write(meshes, placed):
    existing = json.loads(COLLISION.read_text())
    ids = {id for id, _ in SEGMENTS}
    kept = [m for m in existing if not 400 <= m['id'] <= 409]
    entries = kept + [mesh_entry(id, grip, *meshes[id]) for id, grip in SEGMENTS]
    COLLISION.write_text(json.dumps(sorted(entries, key=lambda m: m['id']), separators=(',', ':')) + '\n')
    lines = ['{', '  "version": 1,', '  "area": "redbrand-hollow-cliffs",', '  "placements": [']
    lines += ['    ' + json.dumps(p, separators=(',', ':')) + (',' if i + 1 < len(placed) else '')
              for i, p in enumerate(placed)]
    lines += ['  ]', '}']
    DRESSING_OUT.write_text('\n'.join(lines) + '\n')
    count = sum(len(meshes[id][1]) for id in ids)
    print(f'Wrote {len(ids)} quarry segments ({count} triangles) into {COLLISION.relative_to(ROOT)} '
          f'and {len(placed)} placements into {DRESSING_OUT.relative_to(ROOT)}')


# Blender review: game (x, y, z) metres are Blender (x, -z, y).

def review(meshes, placed):
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

    tints = {'rock': material('Quarry built climbable (1195 3b)', (0.85, 0.45, 0.15)),
             'sheer': material('Quarry built sheer (1195 3b)', (0.2, 0.4, 0.85))}
    grass = material('Quarry built ground (1195 3b)', (0.30, 0.34, 0.22))
    leaf = material('Quarry built ivy (1195 3b)', (0.15, 0.45, 0.12))

    def child(label):
        collection = bpy.data.collections.new(f'{NAME} {label}')
        root.children.link(collection)
        return collection

    def add(collection, name, vertices, faces, mat, colour=None):
        data = bpy.data.meshes.new(name)
        data.from_pydata([(x, -z, y) for x, y, z in vertices], [], faces)
        data.update()
        for polygon in data.polygons:
            polygon.use_smooth = False
        data.materials.append(mat)
        obj = bpy.data.objects.new(name, data)
        if colour:
            obj.color = colour
        collection.objects.link(obj)
        return obj

    if CONTEXT.exists():
        site = json.loads(CONTEXT.read_text())
        cols, rows = site['columns'], site['rows']
        ground_vertices = []
        for r in range(rows):
            for c in range(cols):
                i = r * cols + c
                ground_vertices.append(((site['origin'][0] + c * site['step']) / UNITS,
                                        min(site['ground'][i], site['surface'][i]) / UNITS,
                                        (site['origin'][1] + r * site['step']) / UNITS))
        faces = [(r * cols + c, r * cols + c + 1, (r + 1) * cols + c + 1, (r + 1) * cols + c)
                 for r in range(rows - 1) for c in range(cols - 1)]
        add(child('ground'), 'Quarry ground', ground_vertices, faces, grass)

    walls = child('wall segments')
    for id, grip in SEGMENTS:
        vertices, triangles = meshes[id]
        obj = add(walls, f'Quarry segment {id} ({grip})',
                  [(x, y + DATUM / UNITS, z) for x, y, z in vertices], triangles, tints[grip],
                  colour=tints[grip].diffuse_color)
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
    rings = build_rings()
    groups = segments(rings)
    if len(groups) != len(SEGMENTS):
        raise SystemExit('the outline does not split into the nine segments')
    points = mitre(rings, groups)
    meshes = {}
    owner = {}
    for (id, grip), members in zip(SEGMENTS, groups):
        vertices, triangles = segment_mesh(rings, points, members)
        check_closed(id, triangles)
        meshes[id] = (vertices, triangles)
        for i in members:
            owner.setdefault(i, id)
    rope = meshes[ROPE_SEGMENT][0]
    if min(v[2] for v in rope) < ROPE_FACE_Z - 1e-6 or max(v[1] for v in rope) > ROPE_TOP + 1e-6:
        raise SystemExit('the rope segment no longer has its face as its north edge and its landing on top')
    placed = ivy(rings, meshes, owner)
    write(meshes, placed)
    for (id, grip), members in zip(SEGMENTS, groups):
        (x0, z0), (x1, z1) = rings[members[0]][1], rings[members[-1]][1]
        length = sum(math.dist(rings[a][1], rings[b][1]) for a, b in zip(members, members[1:]))
        print(f'  {id} {grip:5} {length:5.1f} m  ({x0:6.1f}, {z0:6.1f}) -> ({x1:6.1f}, {z1:6.1f})')
    if bpy is not None:
        review(meshes, placed)


if __name__ == "__main__":
    main()
