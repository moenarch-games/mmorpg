#!/usr/bin/env python3
"""Authoring source: Redbrand Hollow's seven cliffs as natural rock grown out of the ground (#1195).

Owner decisions #1195.Q3 and Q5 part 3 (2026-11-24): the seven ridges round the Hollow's camp
(colliders 400-406) lose their vertical 3 m base wall and flat roof slab. Each is now one closed
rock mass whose foot flares out and is buried 1.1-1.6 m in the ground along an uneven line, whose
faces lean back irregularly to a broken crest, and whose top is a walkable shoulder to mantle
onto. The ridges keep their footprints at the camp's ground (so the camp floor and its lanes stay
open), their unequal crests and saddles, and their climbability, which now travels with each mesh
as `"grip": "rock" | "sheer"` instead of an ID range: every ridge climbs except the back wall
404. Climbable rock reads rougher than sheer rock (owner Q3): its faces are broken by strata
jogs and vertical cracks, and two of them wear a little climbing ivy; the sheer back wall is a
smooth, steep sweep. Nothing overhangs, so a climber holds on from the foot to the crest.

Ridge 402 keeps the west rope (#621): its west face stays upright at x = 22 m round the rope and
its plateau stays level at 12 m, the ridge's top, so the climber steps off the rope onto it.

1. Export the Hollow's ground (the same arguments as `SITE` and `HALF`):

       cargo run -p mmorpg-scenery --example export-site-context -- \\
           --centre 0,-9700 --half 4400,3000 > artifacts/hollow/site.json

2. Run this file, headless with `python3`, or in Blender (5.2+; the Python console, `__file__`
   set to its path) to review the ridges on the real ground. Both replace the seven ridges in
   `crates/mmorpg-core/content/collision/redbrand.json` (the cave's meshes, authored by
   `redbrand.py`, stay byte for byte) and write the ivy into
   `content/greyhaven/dressing/hollow-cliffs.json`.

In Blender the ridges are drawn in the collection `Redbrand Hollow re-model (1195)`: orange
where climbable, blue where sheer (the viewport's object-colour shading), the ground round them,
the ivy's pieces as thin green panels and the previous ridges (from `origin/main`) in grey
wireframe. Rerunning replaces only that collection's children.
"""
import json
import math
import pathlib
import subprocess

try:
    import bpy
except ImportError:  # headless export
    bpy = None

ROOT = pathlib.Path(__file__).resolve().parents[3]
CONTEXT = ROOT / 'artifacts/hollow/site.json'
COLLISION = ROOT / 'crates/mmorpg-core/content/collision/redbrand.json'
IVY_OUT = ROOT / 'content/greyhaven/dressing/hollow-cliffs.json'
NAME = 'Redbrand Hollow re-model (1195)'
SITE = (0, -9700)
HALF = (4400, 3000)
UNITS = 100
# Every mesh in redbrand.json stands on the Hollow's 11 m datum.
DATUM = 1100

# The ridges: footprint `(x0, x1, z0, z1)` in metres at the camp's ground, the crest's height
# scale (m above the 11 m datum) and grip. These footprints and heights are the previous ridges'.
RIDGES = {
    400: ((-32, -22, -120, -80), 12, 'rock'),
    401: ((-22, -16, -84, -80), 8, 'rock'),
    402: ((22, 32, -120, -74), 12, 'rock'),
    403: ((16, 22, -96, -86), 9, 'rock'),
    404: ((-22, 22, -120, -110), 14, 'sheer'),
    405: ((-22, -8, -110, -104), 10, 'rock'),
    406: ((8, 22, -110, -105), 10, 'rock'),
}
# The previous ridges' XZ bounds (units, min x, max x, min z, max z). The buried flare is
# trimmed to them: the scenery scatters its grass, flowers and bushes from one seeded stream
# that rejects candidates against these bounds, so wider bounds would reshuffle thousands of
# placements across the whole region for a change under the ground.
BOUNDS = {
    400: (-3212, -2189, -12000, -8000),
    401: (-2200, -1600, -8408, -7990),
    402: (2200, 3227, -12000, -7400),
    403: (1591, 2211, -9600, -8600),
    404: (-2200, 2200, -12021, -10974),
    405: (-2200, -800, -11011, -10385),
    406: (800, 2200, -11013, -10494),
}
# Points round each footprint about this far apart (m), before 402's rope samples, from a
# dense sampling of its outline.
SPACING = 1.25
DENSE = 4096
# The footprint's rounded-box exponent: the previous ridges' outline.
ROUNDNESS = 0.28
# 402's rope (`greyhaven_vale::ROPES[0]`) hangs down its west face at z = -100.5 m; the face
# stays upright for `ROPE_FACE` m either side, and the crest is a level 12 m plateau for
# `ROPE_PLATEAU` m either side, the ridge's highest point.
ROPE_Z = -100.5
ROPE_FACE = 3.5
ROPE_PLATEAU = 8.0
ROPE_TOP = 12.0
# How far each ring leans in from the footprint (m), bottom to top: the buried foot (outward,
# under the ground), the foot at the ground, three rows up the face, the crest's edge and the
# shoulder inside it. Climbable rock steps in more and unevenly; sheer rock is a smooth sweep.
INSET = {
    'rock': (-0.6, 0.45, 0.95, 1.5, 2.05, 2.6, 3.7),
    'sheer': (-0.5, 0.35, 0.6, 0.82, 1.04, 1.25, 2.5),
}
# The face's rows, as fractions of the crest's height above the ground.
ROWS = (0.30, 0.55, 0.78)
# Climbing ivy (#621's kit) down a few climbable faces: (ridge, outward direction (x, z), the
# columns' positions along the face (m, on the long axis), how high up the crest it reaches).
# The faces under it lean back no more than `IVY_LEAN` so the upright strips sit on the rock.
IVY = [
    (400, (1, 0), (-96.0, -95.0, -94.0), 0.8),
    (403, (-1, 0), (-91.5, -90.5), 0.75),
]
IVY_LEAN = 0.25
IVY_HALF_WIDTH = 2.5
VINE_PITCH = 2.0
VINE_TOP_HEIGHT = 1.0
VINE_BELOW_TOP = 0.1
VINE_OUT = 0.05


def context():
    if not CONTEXT.exists():
        raise SystemExit(f'{CONTEXT} is missing; export the site context first (see the docstring)')
    return json.loads(CONTEXT.read_text())


def ground(site, x, z):
    """The lower of the physical ground and the drawn surface at `(x, z)` metres, in metres
    above the 11 m datum."""
    column = round((x * UNITS - site['origin'][0]) / site['step'])
    row = round((z * UNITS - site['origin'][1]) / site['step'])
    if not (0 <= column < site['columns'] and 0 <= row < site['rows']):
        raise SystemExit('the site context does not cover the Hollow; widen HALF')
    index = row * site['columns'] + column
    return (min(site['ground'][index], site['surface'][index]) - DATUM) / UNITS


def wave(*keys):
    """A fixed, smooth pseudo-random value in [-1, 1] for these keys."""
    a = sum(k * m for k, m in zip(keys, (2.713, 1.371, 0.719, 3.17)))
    b = sum(k * m for k, m in zip(keys, (0.931, -2.113, 0.373, -1.57)))
    return 0.6 * math.sin(a) + 0.4 * math.sin(b + 1.3)


def shape(id):
    """A ridge's rock: `(vertices in metres, triangles)`, closed and wound outward."""
    (x0, x1, z0, z1), height, grip = RIDGES[id]
    cx, cz = (x0 + x1) / 2, (z0 + z1) / 2
    hx, hz = (x1 - x0) / 2, (z1 - z0) / 2
    along_z = hz > hx
    long_half, short_half = (hz, hx) if along_z else (hx, hz)
    # The previous ridges' rounded-box footprint, sampled evenly along its length so the long
    # faces get as much detail as the ends.
    dense = []
    for j in range(DENSE):
        a = j * math.tau / DENSE
        s, c = math.sin(a), math.cos(a)
        dense.append((cx + hx * math.copysign(abs(s) ** ROUNDNESS, s),
                      cz + hz * math.copysign(abs(c) ** ROUNDNESS, c)))
    lengths = [0.0]
    for j in range(DENSE):
        lengths.append(lengths[-1] + math.dist(dense[j], dense[(j + 1) % DENSE]))
    count = max(24, 2 * round(lengths[-1] / SPACING / 2))
    outline = []
    j = 0
    for i in range(count):
        want = lengths[-1] * i / count
        while lengths[j + 1] < want:
            j += 1
        f = (want - lengths[j]) / (lengths[j + 1] - lengths[j])
        a, b = dense[j], dense[(j + 1) % DENSE]
        outline.append((a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f))
    if id == 402:
        # The rope's face: upright samples at x = 22 m either side of the rope, in the outline's
        # order (it runs -Z down the east side and +Z up the west).
        west = [(x0, z) for z in (ROPE_Z - ROPE_FACE, ROPE_Z, ROPE_Z + ROPE_FACE)]
        outline = [p for p in outline if not (p[0] < cx and abs(p[1] - ROPE_Z) <= ROPE_FACE + 0.3)]
        at = next(i for i, p in enumerate(outline) if p[0] < cx and p[1] > ROPE_Z)
        outline[at:at] = west
    n = len(outline)

    def long(p):
        return p[1] - cz if along_z else p[0] - cx

    def spine(p):
        """The nearest point of the ridge's long axis, kept a short half-width from its ends."""
        t = max(-(long_half - short_half), min(long_half - short_half, long(p)))
        return (cx, cz + t) if along_z else (cx + t, cz)

    def rope_face(p):
        return id == 402 and p[0] <= x0 + 1e-6 and abs(p[1] - ROPE_Z) <= ROPE_FACE + 1e-6

    def crest(p):
        # Unequal peaks and saddles along the ridge, its ends dropping toward the ground.
        t = long(p) / long_half
        end = 0.4 + 0.6 * (1 - abs(t) ** 3)
        top = max(3.2, height * (0.95 + 0.30 * math.sin(t * 3.1 + id * 0.71)
                                 + 0.14 * math.cos(t * 7.3 - id * 0.37)) * end)
        if id == 402:
            top = min(ROPE_TOP, top)
            if abs(p[1] - ROPE_Z) < ROPE_PLATEAU:
                top = ROPE_TOP
        return top

    def ivy_face(p):
        for ridge, (ox, oz), columns, _ in IVY:
            if ridge != id:
                continue
            side = (p[0] - cx) * ox + (p[1] - cz) * oz
            spot = p[1] if ox else p[0]
            if side > 0 and min(columns) - IVY_HALF_WIDTH <= spot <= max(columns) + IVY_HALF_WIDTH:
                return True
        return False

    rows = []
    insets = INSET[grip]
    for j, p in enumerate(outline):
        q = spine(p)
        reach = math.hypot(q[0] - p[0], q[1] - p[1])
        direction = ((q[0] - p[0]) / reach, (q[1] - p[1]) / reach) if reach > 1e-6 else (0, 0)
        # Never lean in past most of the way to the spine, on the narrowest ridges too.
        fit = min(1.0, 0.7 * reach / insets[-1]) if reach > 1e-6 else 0.0
        floor = ground_of(p)
        top = max(crest(p), floor + 2.5)
        rough = 1.0 if grip == 'rock' else 0.35
        # Vertical cracks: every few samples a narrow groove up the face of climbable rock.
        crack = 0.3 if grip == 'rock' and (j * 5 + id) % 7 == 0 else 0.0
        # Buttresses and gullies: the whole face swells and sinks slowly along the ridge.
        rib = rough * 0.45 * wave(id, long(p) * 0.41, 3)
        d = []
        for k, base in enumerate(insets):
            jitter = 0.0
            if 1 <= k <= 5:
                # Strata: whole rows jog in and out together, broken a little along the face.
                jitter = rough * (0.15 * wave(id, k) + 0.10 * wave(id, k, j * 0.37))
                if 2 <= k <= 5:
                    jitter += crack + rib * (k - 1) / 4
            d.append((base + jitter) * fit)
        if ivy_face(p):
            d = [v if k in (0, 6) else min(v, d[1] + IVY_LEAN * (k - 1) / 4) for k, v in enumerate(d)]
        if rope_face(p):
            d = [0.0] * 6 + [d[6]]
        if id == 402 and p[0] < cx:
            d[0] = max(d[0], 0.0)
        # Nothing overhangs: each ring leans in at least as far as the one under it.
        for k in range(1, 7):
            d[k] = max(d[k], d[k - 1])
        # Heights: the buried foot, the foot's uneven line at the ground, the face's rows, the
        # crest's broken edge and the shoulder inside it.
        foot = floor - 1.1 - 0.25 * (1 + wave(id, j * 0.61, 2))
        lip = floor + 0.3 + 0.4 * (1 + wave(id, j * 0.43, 5))
        rise = top - floor
        ys = [foot, lip]
        for k, f in enumerate(ROWS):
            y = floor + rise * f + rough * 0.25 * wave(id, j * 0.29, k + 7)
            ys.append(max(y, ys[-1] + 0.6))
        edge = top + rough * 0.15 * wave(id, j * 0.53, 11)
        ys.append(max(edge, ys[-1] + 0.4))
        ys.append(ys[-1] + 0.15)
        if id == 402:
            ys = [min(y, ROPE_TOP) for y in ys]
            if crest(p) >= ROPE_TOP:
                ys[5] = ys[6] = ROPE_TOP
        if rope_face(p):
            ys[0] = floor - 1.3
        rows.append([(p[0] + direction[0] * v, y, p[1] + direction[1] * v) for v, y in zip(d, ys)])

    bx0, bx1, bz0, bz1 = (v / UNITS for v in BOUNDS[id])
    vertices = [(min(max(x, bx0), bx1), y, min(max(z, bz0), bz1))
                for x, y, z in (rows[j][k] for k in range(7) for j in range(n))]
    if [min(v[0] for v in vertices), max(v[0] for v in vertices),
            min(v[2] for v in vertices), max(v[2] for v in vertices)] != [bx0, bx1, bz0, bz1]:
        raise SystemExit(f'ridge {id} no longer reaches its previous bounds')

    def index(k, j):
        return k * n + j % n

    triangles = []
    for k in range(6):
        for j in range(n):
            a, b, c, e = index(k, j), index(k, j + 1), index(k + 1, j + 1), index(k + 1, j)
            # Split each quad along its shorter diagonal.
            if dist(vertices[a], vertices[c]) <= dist(vertices[b], vertices[e]):
                triangles += [(a, b, c), (a, c, e)]
            else:
                triangles += [(a, b, e), (b, c, e)]
    # The shoulder's and the buried base's caps: strips across the ridge from one end to the other.
    ends = sorted(range(n), key=lambda j: long(outline[j]))
    first, last = ends[0], ends[-1]
    up = [first]
    while up[-1] != last:
        up.append((up[-1] + 1) % n)
    down = [first]
    while down[-1] != last:
        down.append((down[-1] - 1) % n)
    for k, flip in ((6, False), (0, True)):
        for tri in zipper([index(k, j) for j in up], [index(k, j) for j in down],
                          lambda i: long((vertices[i][0], vertices[i][2]))):
            triangles.append(tri[::-1] if flip else tri)
    # Wind every triangle outward: the side rings were built in the outline's turning sense.
    if volume(vertices, triangles) < 0:
        triangles = [t[::-1] for t in triangles]
    check_closed(id, triangles)
    return vertices, triangles


def ground_of(p):
    return ground(SITE_DATA, p[0], p[1])


def dist(a, b):
    return math.dist(a, b)


def zipper(a, b, key):
    """Triangles between two chains that start and end at the same points, advancing along
    `key` (the ridge's long axis)."""
    out = []
    i = j = 0
    while i < len(a) - 1 or j < len(b) - 1:
        if j == len(b) - 1 or (i < len(a) - 1 and key(a[i + 1]) <= key(b[j + 1])):
            out.append((a[i], a[i + 1], b[j]))
            i += 1
        else:
            out.append((a[i], b[j + 1], b[j]))
            j += 1
    # The first and last points are shared: drop the degenerate triangles that touch them twice.
    return [t for t in out if len(set(t)) == 3]


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
            raise SystemExit(f'ridge {id} is not closed and consistently wound at edge {a}-{b}')


def mesh_entry(id, vertices, triangles):
    units = [[round(x * UNITS), round(y * UNITS), round(z * UNITS)] for x, y, z in vertices]
    # Drop any triangle that rounding to units leaves without area.
    kept = []
    for a, b, c in triangles:
        u = [units[b][i] - units[a][i] for i in range(3)]
        v = [units[c][i] - units[a][i] for i in range(3)]
        cross = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
        if any(cross):
            kept.append([a, b, c])
    return {'id': id, 'position': [0, DATUM, 0], 'vertices': units, 'triangles': kept,
            'grip': RIDGES[id][2]}


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


def ivy(meshes):
    """Climbing ivy up a few climbable faces: a ragged top under the reach and 2 m strips down
    to the ground, each standing on the face's collision at the most sunken point of its height
    so no strip floats off the rock (as `crags.py`'s)."""
    out = []
    for ridge, (nx, nz), columns, reach in IVY:
        if RIDGES[ridge][2] != 'rock':
            raise SystemExit(f'ridge {ridge}: ivy marks climbable rock only')
        vertices, triangles = meshes[ridge]
        mesh = ([(x, y + DATUM / UNITS, z) for x, y, z in vertices], triangles)
        (x0, x1, z0, z1), *_ = RIDGES[ridge]
        cx, cz = (x0 + x1) / 2, (z0 + z1) / 2
        yaw = round(math.atan2(nx, nz) / math.tau * 65536) % 65536
        for column, spot in enumerate(columns):
            bx, bz = (cx, spot) if nx else (spot, cz)
            # The crest above the column: the highest hit straight down onto the rock.
            drop = ray(mesh, (bx, 60.0, bz), (0, -1, 0))
            if drop is None:
                raise SystemExit(f'ridge {ridge}: ivy column {column} misses the rock')
            top = 60.0 - drop
            floor = DATUM / UNITS + ground(SITE_DATA, bx + nx * 6, bz + nz * 6)
            cap = floor + (top - floor) * reach - VINE_BELOW_TOP - VINE_TOP_HEIGHT
            pieces = [('vine-top', cap, VINE_TOP_HEIGHT)]
            k = 1
            while cap - VINE_PITCH * k + VINE_PITCH > floor:
                pieces.append(('vine-strip', cap - VINE_PITCH * k, VINE_PITCH))
                k += 1
            for index, (part, base, height) in enumerate(pieces):
                sunk = None
                for f in (0.1, 0.5, 0.9):
                    y = max(base + height * f, floor + 0.2)
                    hit = ray(mesh, (bx + nx * 20, y, bz + nz * 20), (-nx, 0, -nz))
                    if hit is not None:
                        out_by = 20 - hit
                        sunk = out_by if sunk is None else min(sunk, out_by)
                if sunk is None:
                    continue
                at = (bx + nx * (sunk + VINE_OUT), base, bz + nz * (sunk + VINE_OUT))
                out.append({
                    'id': f'hollow-cliff-{ridge}-ivy-{column}-{index}',
                    'asset': f'climbing.kit.{part}',
                    'position': [round(at[0] * UNITS), round(at[1] * UNITS), round(at[2] * UNITS)],
                    'yaw': yaw,
                    'scale': [1000, 1000, 1000],
                })
    return out


def write(meshes, placed):
    existing = json.loads(COLLISION.read_text())
    kept = [m for m in existing if m['id'] not in RIDGES]
    entries = kept + [mesh_entry(id, *meshes[id]) for id in sorted(RIDGES)]
    COLLISION.write_text(json.dumps(sorted(entries, key=lambda m: m['id']), separators=(',', ':')) + '\n')
    lines = ['{', '  "version": 1,', '  "area": "redbrand-hollow-cliffs",', '  "placements": [']
    lines += ['    ' + json.dumps(p, separators=(',', ':')) + (',' if i + 1 < len(placed) else '')
              for i, p in enumerate(placed)]
    lines += ['  ]', '}']
    IVY_OUT.write_text('\n'.join(lines) + '\n')
    count = sum(len(meshes[id][1]) for id in RIDGES)
    print(f'Wrote {len(RIDGES)} Hollow ridges ({count} triangles) into {COLLISION.relative_to(ROOT)} '
          f'and {len(placed)} ivy pieces into {IVY_OUT.relative_to(ROOT)}')


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

    tints = {'rock': material('Hollow cliff climbable (1195)', (0.85, 0.45, 0.15)),
             'sheer': material('Hollow cliff sheer (1195)', (0.2, 0.4, 0.85))}
    grass = material('Hollow ground (1195)', (0.30, 0.34, 0.22))
    old = material('Hollow previous ridge (1195)', (0.5, 0.5, 0.5))
    leaf = material('Hollow ivy (1195)', (0.15, 0.45, 0.12))

    def child(label):
        collection = bpy.data.collections.new(f'{NAME} {label}')
        root.children.link(collection)
        return collection

    def add(collection, name, vertices, faces, mat, wire=False, colour=None):
        data = bpy.data.meshes.new(name)
        data.from_pydata([(x, -z, y) for x, y, z in vertices], [], faces)
        data.update()
        # Faceted, as the game draws the rock.
        for polygon in data.polygons:
            polygon.use_smooth = False
        data.materials.append(mat)
        obj = bpy.data.objects.new(name, data)
        if wire:
            obj.display_type = 'WIRE'
        if colour:
            obj.color = colour
        collection.objects.link(obj)
        return obj

    site = SITE_DATA
    cols, rows = site['columns'], site['rows']
    ground_vertices = []
    for r in range(rows):
        for c in range(cols):
            x = (site['origin'][0] + c * site['step']) / UNITS
            z = (site['origin'][1] + r * site['step']) / UNITS
            ground_vertices.append((x, DATUM / UNITS + ground(site, x, z), z))
    faces = [(r * cols + c, r * cols + c + 1, (r + 1) * cols + c + 1, (r + 1) * cols + c)
             for r in range(rows - 1) for c in range(cols - 1)]
    add(child('ground'), 'Hollow ground', ground_vertices, faces, grass)

    ridges = child('ridges')
    for id in sorted(RIDGES):
        vertices, triangles = meshes[id]
        grip = RIDGES[id][2]
        obj = add(ridges, f'Hollow cliff {id} ({grip})',
                  [(x, y + DATUM / UNITS, z) for x, y, z in vertices], triangles, tints[grip],
                  colour=tints[grip].diffuse_color)
        obj['collider_id'] = id
        obj['grip'] = grip

    ivy_collection = child('ivy')
    for p in placed:
        x, y, z = (v / UNITS for v in p['position'])
        height = VINE_TOP_HEIGHT if p['asset'].endswith('vine-top') else VINE_PITCH
        turn = p['yaw'] / 65536 * math.tau
        sx, sz = math.cos(turn) * 0.5, -math.sin(turn) * 0.5
        quad = [(x - sx, y, z - sz), (x + sx, y, z + sz), (x + sx, y + height, z + sz),
                (x - sx, y + height, z - sz)]
        add(ivy_collection, p['id'], quad, [(0, 1, 2, 3)], leaf)

    try:
        before = json.loads(subprocess.run(
            ['git', '-C', str(ROOT), 'show', 'origin/main:crates/mmorpg-core/content/collision/redbrand.json'],
            capture_output=True, check=True, text=True, timeout=30).stdout)
    except (OSError, subprocess.SubprocessError, ValueError):
        before = []
    previous = child('previous')
    for mesh in before:
        if mesh['id'] in RIDGES:
            px, py, pz = mesh['position']
            add(previous, f'Hollow previous ridge {mesh["id"]}',
                [((px + x) / UNITS, (py + y) / UNITS, (pz + z) / UNITS) for x, y, z in mesh['vertices']],
                [tuple(t) for t in mesh['triangles']], old, wire=True)
    previous.hide_viewport = True


SITE_DATA = None


def main():
    global SITE_DATA
    SITE_DATA = context()
    meshes = {id: shape(id) for id in sorted(RIDGES)}
    placed = ivy(meshes)
    write(meshes, placed)
    if bpy is not None:
        review(meshes, placed)


main()
