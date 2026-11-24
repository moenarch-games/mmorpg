#!/usr/bin/env python3
"""Blender authoring source: the Bandit Highlands' crags as groups of sculpted outcrops (#1195).

Owner decisions #1195.Q2 and Q3 (2026-11-24) replace each of the 18 tiled crag boxes
(`highlands::CRAGS`) with a group of the crag kit's sculpted outcrops, its collision generated
from the art (#683), and a climbable or sheer setting per placement. Climbable rock reads subtly
rougher than sheer rock: a climbable crag is a ledged buttress over talus steps, some with a
little climbing ivy; a sheer crag is a cluster of smooth spires. The spires stay sheer
everywhere (their crowns are too steep to stand on), the buttresses and talus climb, and the
two sheer crags (`SHEER` below, the content's 4 and 12) have no climbable rock at all. Crag 4
keeps the rope up its smooth overhanging face (#621), down to the crown of its buttress.
`GROUPS` and `IVY` below are the layout; the kit's rocks and the ground decide everything else.

1. Export the highlands' ground and bodies (the same arguments as `SITE` and `HALF`):

       cargo run -p mmorpg-scenery --example export-site-context -- \\
           --centre 0,-45000 --half 20000,17000 > artifacts/crags/site.json

2. Run this file, either in Blender (5.2+; the Python console, `__file__` set to its path) to
   review the crags on the real ground, or headless with `python3`. Both write
   `content/greyhaven/dressing/crags.json`: every outcrop as one placement, its foot on the
   lowest ground under its base so no rock floats, with its `grip` (`rock` climbs, `sheer`
   does not) and its own collider. Each crag's first outcrop keeps the crag's collider
   (`6300` + its index in `highlands::CRAGS`); the others take the next free IDs from `6318`.
   The ivy is placed down a near-vertical face of the crag's buttress, on the face's generated
   collision, and has no collider.
3. `python3 content/greyhaven/authoring/dressing_collision.py` generates the colliders, each
   with its placement's grip, from the kit's collision meshes; review its diff.

In Blender each crag is drawn in the collection `Highland crags as outcrop groups (1195)`: the
kit's outcrops (orange object colour where climbable, blue where sheer, in the viewport's
object-colour shading), the ivy, the drawn ground round it and the old box in grey wireframe;
once generated, the committed collision is in red wireframe in its hidden `collision` child.
Rerunning replaces only that collection's crag and collision children.
"""
import json
import math
import pathlib

try:
    import bpy
except ImportError:  # headless export
    bpy = None

ROOT = pathlib.Path(__file__).resolve().parents[3]
CRAG = ROOT / 'web/public/scenery/crag.kit'
CLIMBING = ROOT / 'web/public/scenery/climbing.kit'
CONTEXT = ROOT / 'artifacts/crags/site.json'
OUT = ROOT / 'content/greyhaven/dressing/crags.json'
COLLISION = ROOT / 'crates/mmorpg-core/content/collision/greyhaven-dressing.json'
NAME = 'Highland crags as outcrop groups (1195)'

SITE = (0, -45000)
HALF = (20000, 17000)
# The crags by their index in `highlands::CRAGS`, at the centre of their old footprint (units).
# Each outcrop: (asset, offset from the centre (x, z metres), yaw in degrees (+Z toward +X),
# scale (x, y, z thousandths), grip). The first outcrop is the crag's core; a climbable core's
# ledged face (buttress-1's +X, buttress-2's +Z) looks toward the nearest trail.
GROUPS = [
    # Flanking the gorge's top.
    (0, (1500, -29600), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 90, (1000, 1082, 1000), 'rock'),
        ('crag.kit.outcrop-spire', (-3.0, 2.4), 43, (800, 863, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (3.2, -2.8), 65, (800, 663, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-0.8, -4.2), 108, (800, 713, 850), 'rock'),
    ]),
    (1, (6800, -30000), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 270, (1300, 902, 1300), 'rock'),
        ('crag.kit.outcrop-spire', (3.0, 2.4), 213, (800, 719, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-3.2, -2.8), 207, (800, 659, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (0.8, -4.2), 186, (800, 768, 850), 'rock'),
    ]),
    # Along the trail's first leg and its bend west.
    (2, (-4800, -31600), [
        ('crag.kit.outcrop-buttress-1', (0, 0), 67, (1067, 994, 1067), 'rock'),
        ('crag.kit.outcrop-spire', (-3.4, 1.8), 333, (800, 1007, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (3.8, -1.8), 234, (800, 660, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (3.6, 2.4), 93, (800, 662, 850), 'rock'),
    ]),
    (3, (-1800, -37600), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 337, (1300, 902, 1300), 'rock'),
        ('crag.kit.outcrop-spire', (3.4, -1.8), 247, (800, 719, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-1.3, -4.0), 340, (800, 658, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-3.6, -2.4), 217, (800, 795, 850), 'rock'),
    ]),
    # Sheer, with the rope up the overhang of its buttress (#621) the only way up.
    (4, (-13100, -35900), [
        ('crag.kit.outcrop-buttress-1', (0, 0), 90, (1250, 1050, 1250), 'sheer'),
        ('crag.kit.outcrop-spire', (2.8, -3.2), 103, (900, 1000, 900), 'sheer'),
        ('crag.kit.outcrop-spire', (-3.0, -2.6), 81, (800, 780, 800), 'sheer'),
    ]),
    (5, (-12400, -41600), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 102, (1300, 1082, 1300), 'rock'),
        ('crag.kit.outcrop-spire', (-2.5, 3.0), 7, (800, 863, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (2.6, -3.4), 17, (800, 709, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-1.6, -4.0), 95, (800, 684, 850), 'rock'),
    ]),
    (6, (-5200, -40000), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 324, (1300, 812, 1300), 'rock'),
        ('crag.kit.outcrop-talus-1', (4.1, -1.0), 17, (800, 710, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (3.9, 1.8), 338, (800, 894, 850), 'rock'),
    ]),
    # Round the lookout's knoll.
    (7, (16000, -35600), [
        ('crag.kit.outcrop-buttress-1', (0, 0), 128, (1067, 923, 1067), 'rock'),
        ('crag.kit.outcrop-spire', (3.7, 0.9), 129, (800, 935, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-4.2, -0.8), 68, (800, 692, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-2.8, 3.2), 132, (800, 769, 850), 'rock'),
    ]),
    (8, (16400, -42000), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 38, (1300, 992, 1300), 'rock'),
        ('crag.kit.outcrop-spire', (0.0, -3.8), 252, (800, 791, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-4.2, -0.8), 252, (800, 750, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-3.8, 2.0), 207, (800, 742, 850), 'rock'),
    ]),
    (9, (8500, -41700), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 44, (1000, 721, 1000), 'rock'),
        ('crag.kit.outcrop-talus-1', (0.2, 4.2), 276, (800, 753, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-3.6, 2.3), 323, (800, 640, 850), 'rock'),
    ]),
    # East of the trail's southern leg.
    (10, (5800, -45200), [
        ('crag.kit.outcrop-buttress-1', (0, 0), 148, (1067, 994, 1067), 'rock'),
        ('crag.kit.outcrop-spire', (1.3, 3.6), 275, (800, 1007, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-1.2, -4.1), 193, (800, 609, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (2.9, -3.1), 85, (800, 630, 850), 'rock'),
    ]),
    (11, (10000, -48400), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 38, (1300, 902, 1300), 'rock'),
        ('crag.kit.outcrop-spire', (0.0, -3.8), 244, (800, 719, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-0.2, 4.2), 228, (800, 608, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-3.8, 2.0), 193, (800, 812, 850), 'rock'),
    ]),
    # West and east of the stronghold; the western one sheer spires.
    (12, (-9100, -51500), [
        ('crag.kit.outcrop-spire', (0, 0), 12, (1250, 1210, 1250), 'sheer'),
        ('crag.kit.outcrop-spire', (-1.0, 3.1), 79, (1000, 900, 1000), 'sheer'),
        ('crag.kit.outcrop-spire', (0.7, -3.2), 276, (850, 620, 850), 'sheer'),
        ('crag.kit.outcrop-talus-2', (3.3, 0.3), 100, (700, 500, 700), 'sheer'),
    ]),
    (13, (-7600, -58400), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 68, (1300, 1082, 1300), 'rock'),
        ('crag.kit.outcrop-spire', (-3.7, 1.1), 3, (800, 863, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-1.9, -3.8), 125, (800, 787, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (0.8, -4.2), 246, (800, 815, 850), 'rock'),
    ]),
    (14, (8800, -56000), [
        ('crag.kit.outcrop-buttress-1', (0, 0), 210, (1067, 1065, 1067), 'rock'),
        ('crag.kit.outcrop-spire', (1.4, -3.6), 135, (800, 1079, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-1.4, 4.0), 141, (800, 788, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (2.8, 3.3), 154, (800, 719, 850), 'rock'),
    ]),
    (15, (7600, -60400), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 241, (1300, 902, 1300), 'rock'),
        ('crag.kit.outcrop-spire', (3.8, -0.7), 38, (800, 719, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-4.2, 0.9), 82, (800, 658, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-1.3, 4.1), 102, (800, 689, 850), 'rock'),
    ]),
    # Far west and far east.
    (16, (-16800, -46800), [
        ('crag.kit.outcrop-buttress-1', (0, 0), 335, (1067, 923, 1067), 'rock'),
        ('crag.kit.outcrop-spire', (-1.7, -3.4), 250, (800, 935, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (1.7, 3.9), 162, (800, 716, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-2.5, 3.5), 160, (800, 740, 850), 'rock'),
    ]),
    (17, (17600, -51200), [
        ('crag.kit.outcrop-buttress-2', (0, 0), 32, (1300, 1082, 1300), 'rock'),
        ('crag.kit.outcrop-spire', (0.4, -3.8), 150, (800, 863, 800), 'sheer'),
        ('crag.kit.outcrop-talus-1', (-0.7, 4.2), 254, (800, 701, 800), 'rock'),
        ('crag.kit.outcrop-talus-2', (-4.0, 1.5), 300, (800, 876, 850), 'rock'),
    ]),
]
# The sheer crags, by index (`greyhaven_vale::SHEER_CRAGS`): no climbable rock.
SHEER = (4, 12)
# Climbing ivy (#621's kit) on a few climbable crags: (crag, the face's outward direction in its
# core's own frame (x, z), the columns' offsets along the face (m), how high up the core the
# ivy reaches (0-1)). Buttress-2's -X and buttress-1's +Z faces stand nearly upright.
IVY = [
    (0, (-1, 0), (-0.5, 0.5), 0.75),
    (2, (0, 1), (-0.5, 0.5), 0.7),
    (5, (-1, 0), (0.0,), 0.6),
    (7, (0, 1), (-0.5, 0.5), 0.8),
    (10, (0, 1), (0.0,), 0.65),
]
# Each crag's core keeps the crag's collider; the other outcrops number on from here.
FIRST_CRAG = 6300
FIRST_EXTRA = 6318
LAST_ID = 6399
# Ivy strips tile every 2 m; the ragged top is 1 m tall and ends 0.1 m under the reach; a strip
# stands this far out of the rock's collision, where the drawn rock is.
VINE_PITCH = 2.0
VINE_TOP_HEIGHT = 1.0
VINE_BELOW_TOP = 0.1
VINE_OUT = 0.05
UNITS = 100


def crag_descriptor():
    manifest = json.loads((CRAG / 'manifest.json').read_text())
    for asset in manifest['assets']:
        for attachment in asset.get('attachments', []):
            if attachment.get('name') == 'crag-descriptor':
                value = json.loads((CRAG / attachment['path']).read_text())
                return {entry['key']: entry for entry in value['outcrops']['entries']}
    raise SystemExit(f'{CRAG} attaches no crag-descriptor; run scripts/fetch-bundles.sh')


def context():
    if not CONTEXT.exists():
        raise SystemExit(f'{CONTEXT} is missing; export the site context first (see the docstring)')
    return json.loads(CONTEXT.read_text())


def ground(site, x, z):
    """The lower of the physical ground and the drawn surface at `(x, z)` units."""
    column = round((x - site['origin'][0]) / site['step'])
    row = round((z - site['origin'][1]) / site['step'])
    if not (0 <= column < site['columns'] and 0 <= row < site['rows']):
        raise SystemExit('the site context does not cover the crags; widen HALF')
    index = row * site['columns'] + column
    return min(site['ground'][index], site['surface'][index])


def frame(yaw_degrees, scale):
    """The asset-to-world turn and scale of a placement, as a function of a local point in
    metres: dressing_collision.py's and `Placed::frame`'s (+Z turns toward +X)."""
    turn = round(yaw_degrees % 360 / 360 * 65536) % 65536 / 65536 * math.tau
    c, s = math.cos(turn), math.sin(turn)
    sx, sy, sz = (v / 1000 for v in scale)

    def apply(point):
        x, y, z = point[0] * sx, point[1] * sy, point[2] * sz
        return (c * x + s * z, y, -s * x + c * z)
    return apply


def foot(site, rock, centre, apply):
    """The lowest ground under the rock's base ring, so no part of its foot floats (units)."""
    base = min(v[1] for v in rock['collisionMesh']['vertices'])
    ring = [v for v in rock['collisionMesh']['vertices'] if v[1] < base + 1e-6]
    points = [apply(v) for v in ring] + [(0, 0, 0)]
    return min(ground(site, centre[0] + round(p[0] * UNITS), centre[1] + round(p[2] * UNITS))
               for p in points)


def placements(kit, site):
    out = []
    extra = FIRST_EXTRA
    for crag, centre, rocks in GROUPS:
        if crag in SHEER and any(grip != 'sheer' for *_, grip in rocks):
            raise SystemExit(f'crag {crag} is sheer but has climbable rock')
        for index, (asset, (dx, dz), yaw, scale, grip) in enumerate(rocks):
            rock = kit[asset]
            at = (centre[0] + round(dx * UNITS), centre[1] + round(dz * UNITS))
            apply = frame(yaw, scale)
            if index == 0:
                collider = FIRST_CRAG + crag
            else:
                collider, extra = extra, extra + 1
            if collider > LAST_ID:
                raise SystemExit('the crags have run out of collider IDs')
            role = asset.removeprefix('crag.kit.outcrop-')
            out.append({
                'id': f'highland-crag-{crag}-{index}-{role}',
                'asset': asset,
                'position': [at[0], foot(site, rock, at, apply), at[1]],
                'yaw': round(yaw % 360 / 360 * 65536) % 65536,
                'scale': list(scale),
                'collider': collider,
                'grip': grip,
            })
    return out


def world_mesh(kit, placed):
    """A placement's collision mesh in world metres: (vertices, triangles)."""
    rock = kit[placed['asset']]
    apply = frame(placed['yaw'] / 65536 * 360, placed['scale'])
    x, y, z = (v / UNITS for v in placed['position'])
    vertices = [tuple(a + b for a, b in zip(apply(v), (x, y, z)))
                for v in rock['collisionMesh']['vertices']]
    return vertices, rock['collisionMesh']['triangles']


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


def ivy(kit, site, placed):
    """Climbing ivy down a near-vertical face of a few climbable crags' cores: a ragged top
    under the reach and 2 m strips down to the ground, each standing on the face's collision at
    the most sunken point of its height so no strip floats off the rock."""
    cores = {p['collider'] - FIRST_CRAG: p for p in placed if p['collider'] < FIRST_EXTRA}
    out = []
    for crag, (ox, oz), offsets, reach in IVY:
        core = cores[crag]
        if core['grip'] != 'rock':
            raise SystemExit(f'crag {crag}: ivy marks climbable rock only')
        mesh = world_mesh(kit, core)
        apply = frame(core['yaw'] / 65536 * 360, (1000, 1000, 1000))
        nx, _, nz = apply((ox, 0, oz))
        sx, sz = -nz, nx
        x0, y0, z0 = (v / UNITS for v in core['position'])
        top = max(v[1] for v in mesh[0])
        cap = y0 + (top - y0) * reach - VINE_BELOW_TOP - VINE_TOP_HEIGHT
        yaw = round(math.atan2(nx, nz) / math.tau * 65536) % 65536
        for column, offset in enumerate(offsets):
            bx, bz = x0 + sx * offset, z0 + sz * offset
            floor = ground(site, round((bx + nx * 3) * UNITS), round((bz + nz * 3) * UNITS)) / UNITS
            pieces = [('vine-top', cap, VINE_TOP_HEIGHT)]
            k = 1
            while cap - VINE_PITCH * k + VINE_PITCH > floor:
                pieces.append(('vine-strip', cap - VINE_PITCH * k, VINE_PITCH))
                k += 1
            for n, (part, base, height) in enumerate(pieces):
                # The face's most sunken point over the piece's height, from 20 m out.
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
                    'id': f'highland-crag-{crag}-ivy-{column}-{n}',
                    'asset': f'climbing.kit.{part}',
                    'position': [round(at[0] * UNITS), round(at[1] * UNITS), round(at[2] * UNITS)],
                    'yaw': yaw,
                    'scale': [1000, 1000, 1000],
                })
    return out


def write(placed):
    lines = ['{', '  "version": 1,', '  "area": "highland-crags",', '  "placements": [']
    lines += ['    ' + json.dumps(p, separators=(',', ':')) + (',' if i + 1 < len(placed) else '')
              for i, p in enumerate(placed)]
    lines += ['  ]', '}']
    OUT.write_text('\n'.join(lines) + '\n')
    rocks = [p for p in placed if 'collider' in p]
    print(f'Wrote {len(rocks)} outcrops ({sum(p["grip"] == "rock" for p in rocks)} climbable) and '
          f'{len(placed) - len(rocks)} ivy pieces into {OUT.relative_to(ROOT)}')


# Blender review: game (x, y, z) units are Blender (x, -z, y) metres.

def blender_point(x, y, z):
    return (x / UNITS, -z / UNITS, y / UNITS)


def review(site, placed):
    from mathutils import Matrix

    root = bpy.data.collections.get(NAME) or bpy.data.collections.new(NAME)
    if root.name not in bpy.context.scene.collection.children:
        bpy.context.scene.collection.children.link(root)
    # Replace only this script's crag and collision collections; the kit library is reused.
    for child in list(root.children):
        if child.name.startswith((f'{NAME} crag ', f'{NAME} collision')):
            for obj in list(child.objects):
                bpy.data.objects.remove(obj, do_unlink=True)
            bpy.data.collections.remove(child)
    library = bpy.data.collections.get(f'{NAME} kit')
    if library is None:
        library = bpy.data.collections.new(f'{NAME} kit')
        root.children.link(library)
    library.hide_viewport = library.hide_render = True

    def material(name, colour, alpha=1.0):
        mat = bpy.data.materials.get(name) or bpy.data.materials.new(name)
        mat.use_nodes = True
        shader = next(n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED')
        shader.inputs['Base Color'].default_value = (*colour, 1)
        shader.inputs['Roughness'].default_value = 0.9
        mat.diffuse_color = (*colour, alpha)
        return mat

    keys = sorted({p['asset'] for p in placed})
    modules = {}
    for key in keys:
        found = [o for o in library.objects if o.get('kit_key') == key]
        if not found:
            bundle = CRAG if key.startswith('crag.kit.') else CLIMBING
            before = set(bpy.data.objects)
            bpy.ops.import_scene.gltf(filepath=str(bundle / 'scenes' / key / 'default.gltf'))
            found = list(set(bpy.data.objects) - before)
            for obj in found:
                for collection in list(obj.users_collection):
                    collection.objects.unlink(obj)
                library.objects.link(obj)
                obj['kit_key'] = key
        modules[key] = [(o.data, o.matrix_world.copy()) for o in found if o.type == 'MESH']

    tints = {'rock': material('Crag climbable (1195)', (0.85, 0.45, 0.15)),
             'sheer': material('Crag sheer (1195)', (0.2, 0.4, 0.85))}
    grass = material('Crag ground (1195)', (0.30, 0.34, 0.22))
    old = material('Crag old box (1195)', (0.5, 0.5, 0.5))
    wire = material('Crag collision (1195)', (0.9, 0.1, 0.1))
    generated = {}
    if COLLISION.exists():
        generated = {m['id']: m for m in json.loads(COLLISION.read_text())['meshes']}
    # The generated collision, hidden until the reviewer shows it.
    collision = bpy.data.collections.new(f'{NAME} collision')
    root.children.link(collision)
    collision.hide_viewport = collision.hide_render = True
    boxes = {}
    highlands = ROOT / 'crates/mmorpg-core/src/content/greyhaven_vale/highlands.rs'
    import re
    text = highlands.read_text()
    block = text[text.index('pub const CRAGS'):text.index('/// Level ground')].replace('_', '')
    for index, found in enumerate(re.findall(
            r'\(\[(-?\d+), (-?\d+)\], \[(-?\d+), (-?\d+)\], (-?\d+)\)', block)):
        boxes[index] = tuple(map(int, found))

    for crag, centre, _ in GROUPS:
        collection = bpy.data.collections.new(f'{NAME} crag {FIRST_CRAG + crag}')
        root.children.link(collection)
        # The drawn ground 24 m round the crag.
        half, step = 12, 1
        cx, cz = centre
        columns = 2 * half // step + 1
        vertices = []
        for r in range(columns):
            for c in range(columns):
                x, z = cx + (c * step - half) * UNITS, cz + (r * step - half) * UNITS
                vertices.append(blender_point(x, ground(site, x, z), z))
        faces = [(r * columns + c, r * columns + c + 1, (r + 1) * columns + c + 1, (r + 1) * columns + c)
                 for r in range(columns - 1) for c in range(columns - 1)]
        mesh = bpy.data.meshes.new(f'crag {crag} ground')
        mesh.from_pydata(vertices, [], faces)
        mesh.materials.append(grass)
        collection.objects.link(bpy.data.objects.new(f'crag {FIRST_CRAG + crag} ground', mesh))
        # The old box, for comparison.
        if crag in boxes:
            x0, z0, x1, z1, height = boxes[crag]
            base = min(ground(site, x, z) for x in (x0, x1) for z in (z0, z1))
            mesh = bpy.data.meshes.new(f'crag {crag} old box')
            corners = [blender_point(x, y, z) for x in (x0, x1) for y in (base, base + height) for z in (z0, z1)]
            mesh.from_pydata(corners, [], [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6),
                                            (0, 2, 6, 4), (1, 5, 7, 3)])
            mesh.materials.append(old)
            obj = bpy.data.objects.new(f'crag {FIRST_CRAG + crag} old box', mesh)
            obj.display_type = 'WIRE'
            collection.objects.link(obj)
        prefix = f'highland-crag-{crag}-'
        for placement in [p for p in placed if p['id'].startswith(prefix)]:
            x, y, z = placement['position']
            sx, sy, sz = (s / 1000 for s in placement['scale'])
            frame_ = (Matrix.Translation(blender_point(x, y, z))
                      @ Matrix.Rotation(placement['yaw'] / 65536 * math.tau, 4, 'Z')
                      @ Matrix.Diagonal((sx, sz, sy, 1)))
            for data, matrix in modules[placement['asset']]:
                obj = bpy.data.objects.new(placement['id'], data)
                obj.matrix_world = frame_ @ matrix
                if 'grip' in placement:
                    obj.color = tints[placement['grip']].diffuse_color
                    obj['grip'] = placement['grip']
                    obj['collider_id'] = placement['collider']
                collection.objects.link(obj)
            item = generated.get(placement.get('collider'))
            if item:
                px, py, pz = item['position']
                mesh = bpy.data.meshes.new(f'collider {item["id"]}')
                mesh.from_pydata([blender_point(px + vx, py + vy, pz + vz) for vx, vy, vz in item['vertices']],
                                 [], [tuple(t) for t in item['triangles']])
                mesh.materials.append(wire)
                obj = bpy.data.objects.new(f'collider {item["id"]}', mesh)
                obj.display_type = 'WIRE'
                collision.objects.link(obj)


def main():
    kit = crag_descriptor()
    site = context()
    placed = placements(kit, site)
    placed += ivy(kit, site, placed)
    write(placed)
    if bpy is not None:
        review(site, placed)


main()
