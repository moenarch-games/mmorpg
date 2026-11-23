#!/usr/bin/env python3
"""Blender authoring source: Greyhaven's sandstone steading in the Highland Stones (#683.Q1).

Owner decision #683.Q1 (2026-11-14) places asset-tooling's red sandstone building kit in one
coherent, distinct rocky area, starting with a small, visually compatible selection of its
compositions. This source places two of them, the `cottage` and the `workshop` (both on the
kit's 6 m span, with the same roof and gables), as a quarrymen's steading in a hollow of the
Highland Stones rock field (Bandit Highlands), under a low crag-kit rock scar on the bank to its
south: about 160 m from the nearest road, 430 m from any NPC and 145 m beyond any creature's
wander radius, so no bot scenario or idle creature reaches it.
`BUILDINGS` and `ROCKS` below are the layout; the site, the kit's compositions and the ground
decide everything else.

1. Export the site's ground and bodies (the same arguments as `SITE` and `HALF`):

       cargo run -p mmorpg-scenery --example export-site-context -- \\
           --centre -23200,-37600 --half 4000,4000 > artifacts/sandstone/site.json

2. Run this file, either in Blender (5.2+; the Python console, `__file__` set to its path) to
   review the steading on the real ground among the boulders, or headless with `python3`. Both
   write `content/greyhaven/dressing/sandstone.json`: every module of every composition as one
   placement, its foot on the lowest ground under the building so no wall floats, and each with
   its own collider from `FIRST_COLLIDER`.
3. `python3 content/greyhaven/authoring/dressing_collision.py` generates those colliders from
   the kit's collision proxies; review its diff.

In Blender the steading is drawn in the collection `Sandstone steading (683)` with the kit's own
modules (or their proxy boxes where the glTF importer cannot read the bundle), over the site's
drawn surface and its boulders' boxes, and, once generated, the committed collision in red
wireframe. Rerunning replaces only that collection.
"""
import json
import math
import pathlib

try:
    import bpy
except ImportError:  # headless export
    bpy = None

ROOT = pathlib.Path(__file__).resolve().parents[3]
KIT = ROOT / 'web/public/buildings/sandstone.kit'
CRAG = ROOT / 'web/public/scenery/crag.kit'
CONTEXT = ROOT / 'artifacts/sandstone/site.json'
OUT = ROOT / 'content/greyhaven/dressing/sandstone.json'
COLLISION = ROOT / 'crates/mmorpg-core/content/collision/greyhaven-dressing.json'
NAME = 'Sandstone steading (683)'

# The clearing in the Highland Stones, in units, and the half size of the exported context.
SITE = (-23200, -37600)
HALF = (4000, 4000)
# (id, composition, footprint-centre x and z in units, yaw in degrees: the door side, the
# composition's +Z, turns toward +X). The two houses face each other across a small yard in
# the bottom of the hollow.
BUILDINGS = [
    ('sandstone-cottage', 'cottage', (-23900, -37050), 135),
    ('sandstone-workshop', 'workshop', (-22500, -37850), -45),
]
# The quarry face: a low rock scar and its talus on the bank south of the hollow, from the crag
# kit, as Eastdown's (#872). (id, asset, x and z in units, yaw in degrees, scale in thousandths);
# each stands on the lowest ground within 1.5 m of its foot.
ROCKS = [
    ('sandstone-scar-ledge-1', 'crag.kit.outcrop-buttress-2', (-23600, -34300), 180, (900, 650, 900)),
    ('sandstone-scar-ledge-2', 'crag.kit.outcrop-buttress-1', (-22200, -34100), 200, (750, 520, 750)),
    ('sandstone-scar-crag', 'crag.kit.outcrop-talus-1', (-24700, -34700), 30, (1000, 600, 1000)),
    ('sandstone-scar-talus-1', 'crag.kit.outcrop-talus-2', (-23000, -35000), 75, (600, 350, 600)),
    ('sandstone-scar-talus-2', 'crag.kit.outcrop-talus-2', (-21500, -34900), 140, (500, 300, 550)),
    ('sandstone-scar-talus-3', 'crag.kit.outcrop-talus-2', (-24100, -35200), 260, (450, 280, 450)),
]
# The colliders generated from the steading's art; one per placed module or rock.
FIRST_COLLIDER = 65200
# How far around a footprint the ground is read for the building's foot, in metres.
MARGIN = 0.6
UNITS = 100


def descriptor():
    manifest = json.loads((KIT / 'manifest.json').read_text())
    for asset in manifest['assets']:
        for attachment in asset.get('attachments', []):
            if attachment.get('name') == 'sandstone-descriptor':
                return json.loads((KIT / attachment['path']).read_text())
    raise SystemExit(f'{KIT} attaches no sandstone-descriptor; run scripts/fetch-bundles.sh')


def context():
    if not CONTEXT.exists():
        raise SystemExit(f'{CONTEXT} is missing; export the site context first (see the docstring)')
    return json.loads(CONTEXT.read_text())


def lowest(site, centre, half_extents, yaw):
    """The lowest physical ground or drawn surface under a footprint and its margin, units."""
    turn = math.radians(yaw)
    c, s = math.cos(turn), math.sin(turn)
    hx, hz = half_extents[0] + MARGIN, half_extents[1] + MARGIN
    low = None
    steps = 8
    for i in range(steps + 1):
        for j in range(steps + 1):
            x, z = -hx + 2 * hx * i / steps, -hz + 2 * hz * j / steps
            wx = centre[0] + (c * x + s * z) * UNITS
            wz = centre[1] + (-s * x + c * z) * UNITS
            column = round((wx - site['origin'][0]) / site['step'])
            row = round((wz - site['origin'][1]) / site['step'])
            if not (0 <= column < site['columns'] and 0 <= row < site['rows']):
                raise SystemExit('the site context does not cover the steading; widen HALF')
            index = row * site['columns'] + column
            height = min(site['ground'][index], site['surface'][index])
            low = height if low is None else min(low, height)
    return low


def placements(kit, site):
    out = []
    collider = FIRST_COLLIDER
    for building, name, centre, yaw in BUILDINGS:
        composition = kit['compositions'][name]
        footprint = composition['footprint']
        foot = lowest(site, centre, (footprint['width'] / 2, footprint['depth'] / 2), yaw)
        turn = math.radians(yaw)
        c, s = math.cos(turn), math.sin(turn)
        counts = {}
        for item in composition['items']:
            x, y, z = item['translation']
            part = item['key'].removeprefix('sandstone.kit.')
            counts[part] = counts.get(part, 0) + 1
            degrees = (yaw + item['yawDegrees']) % 360
            out.append({
                'id': f'{building}-{part}-{counts[part]}',
                'asset': item['key'],
                'position': [
                    centre[0] + round((c * x + s * z) * UNITS),
                    foot + round(y * UNITS),
                    centre[1] + round((-s * x + c * z) * UNITS),
                ],
                'yaw': round(degrees / 360 * 65536) % 65536,
                'scale': [1000, 1000, 1000],
                'collider': collider,
            })
            collider += 1
    for rock, asset, (x, z), yaw, scale in ROCKS:
        out.append({
            'id': rock,
            'asset': asset,
            'position': [x, lowest(site, (x, z), (1.5 - MARGIN, 1.5 - MARGIN), 0), z],
            'yaw': round(yaw % 360 / 360 * 65536) % 65536,
            'scale': list(scale),
            'collider': collider,
        })
        collider += 1
    return out


def write(placed):
    lines = ['{', '  "version": 1,', '  "area": "sandstone-steading",', '  "placements": [']
    lines += ['    ' + json.dumps(p, separators=(',', ':')) + (',' if i + 1 < len(placed) else '')
              for i, p in enumerate(placed)]
    lines += ['  ]', '}']
    OUT.write_text('\n'.join(lines) + '\n')
    print(f'Wrote {len(placed)} placements, colliders {FIRST_COLLIDER}-'
          f'{FIRST_COLLIDER + len(placed) - 1}, into {OUT.relative_to(ROOT)}')


# Blender review: game (x, y, z) units are Blender (x, -z, y) metres.

def blender_point(x, y, z):
    return (x / UNITS, -z / UNITS, y / UNITS)


def review(kit, site, placed):
    from mathutils import Matrix

    old = bpy.data.collections.get(NAME)
    if old:
        for child in list(old.children):
            for obj in list(child.objects):
                bpy.data.objects.remove(obj, do_unlink=True)
            bpy.data.collections.remove(child)
        for obj in list(old.objects):
            bpy.data.objects.remove(obj, do_unlink=True)
        bpy.data.collections.remove(old)
    root = bpy.data.collections.new(NAME)
    bpy.context.scene.collection.children.link(root)

    def child(name):
        collection = bpy.data.collections.new(f'{NAME} {name}')
        root.children.link(collection)
        return collection

    def material(name, colour):
        mat = bpy.data.materials.get(name) or bpy.data.materials.new(name)
        mat.use_nodes = True
        shader = next(n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED')
        shader.inputs['Base Color'].default_value = (*colour, 1)
        shader.inputs['Roughness'].default_value = 0.9
        mat.diffuse_color = (*colour, 1)
        return mat

    # The drawn surface.
    ground = child('ground')
    columns, rows, step = site['columns'], site['rows'], site['step']
    vertices = [blender_point(site['origin'][0] + c * step, site['surface'][r * columns + c],
                              site['origin'][1] + r * step)
                for r in range(rows) for c in range(columns)]
    faces = [(r * columns + c, r * columns + c + 1, (r + 1) * columns + c + 1, (r + 1) * columns + c)
             for r in range(rows - 1) for c in range(columns - 1)]
    mesh = bpy.data.meshes.new(f'{NAME} surface')
    mesh.from_pydata(vertices, [], faces)
    mesh.materials.append(material('Sandstone steading ground (683)', (0.30, 0.34, 0.22)))
    ground.objects.link(bpy.data.objects.new(f'{NAME} surface', mesh))

    def box(collection, name, centre, half, mat):
        mesh = bpy.data.meshes.new(name)
        corners = [(sx, sy, sz) for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)]
        mesh.from_pydata(corners, [], [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1),
                                        (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)])
        mesh.materials.append(mat)
        obj = bpy.data.objects.new(name, mesh)
        obj.location = blender_point(*centre)
        obj.scale = (half[0] / UNITS, half[2] / UNITS, half[1] / UNITS)
        collection.objects.link(obj)
        return obj

    # The boulders and other bodies already standing there.
    bodies = child('bodies')
    stone = material('Sandstone steading boulders (683)', (0.42, 0.42, 0.40))
    for body in site['bodies']:
        box(bodies, f'body {body["id"]}', body['centre'], body['half'], stone)

    # The kit's modules, imported once and drawn at every placement.
    modules = {}
    library = child('kit modules')
    library.hide_viewport = library.hide_render = True
    sources = [(KIT, asset['key']) for asset in kit['assets']]
    sources += [(CRAG, asset) for asset in sorted({rock[1] for rock in ROCKS})]
    for bundle, key in sources:
        path = bundle / 'scenes' / key / 'default.gltf'
        before = set(bpy.data.objects)
        try:
            bpy.ops.import_scene.gltf(filepath=str(path))
        except RuntimeError as error:
            print(f'{key}: glTF import failed ({error}); drawing its proxies')
        imported = [obj for obj in set(bpy.data.objects) - before]
        for obj in imported:
            for collection in list(obj.users_collection):
                collection.objects.unlink(obj)
            library.objects.link(obj)
        modules[key] = [(obj.data, obj.matrix_world.copy()) for obj in imported if obj.type == 'MESH']
    proxies = {asset['key']: asset.get('collisionProxies', []) for asset in kit['assets']}
    red = material('Sandstone steading proxy (683)', (0.55, 0.25, 0.18))
    steading = child('placements')
    for placement in placed:
        x, y, z = placement['position']
        sx, sy, sz = (s / 1000 for s in placement['scale'])
        frame = (Matrix.Translation(blender_point(x, y, z))
                 @ Matrix.Rotation(placement['yaw'] / 65536 * math.tau, 4, 'Z')
                 @ Matrix.Diagonal((sx, sz, sy, 1)))
        parts = modules.get(placement['asset'])
        if parts:
            for data, matrix in parts:
                obj = bpy.data.objects.new(placement['id'], data)
                obj.matrix_world = frame @ matrix
                obj['collider_id'] = placement['collider']
                steading.objects.link(obj)
            continue
        for proxy in proxies.get(placement['asset'], []):
            cx, cy, cz = proxy['center']
            sx, sy, sz = proxy['size']
            obj = box(steading, placement['id'], (0, 0, 0), (sx * 50, sy * 50, sz * 50), red)
            obj.matrix_world = frame @ Matrix.Translation((cx, -cz, cy)) @ Matrix.Diagonal(
                (sx / 2, sz / 2, sy / 2, 1))

    # The committed collision generated from this placement, once dressing_collision.py ran.
    if COLLISION.exists():
        ids = {p['collider'] for p in placed}
        generated = [m for m in json.loads(COLLISION.read_text())['meshes'] if m['id'] in ids]
        collision = child('collision')
        wire = material('Sandstone steading collision (683)', (0.9, 0.1, 0.1))
        for item in generated:
            px, py, pz = item['position']
            mesh = bpy.data.meshes.new(f'collider {item["id"]}')
            mesh.from_pydata([blender_point(px + vx, py + vy, pz + vz) for vx, vy, vz in item['vertices']],
                             [], [tuple(t) for t in item['triangles']])
            mesh.materials.append(wire)
            obj = bpy.data.objects.new(f'collider {item["id"]}', mesh)
            obj.display_type = 'WIRE'
            collision.objects.link(obj)


def main():
    kit = descriptor()
    site = context()
    placed = placements(kit, site)
    write(placed)
    if bpy is not None:
        review(kit, site, placed)


main()
