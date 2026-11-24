#!/usr/bin/env python3
"""Generates the world dressing's collision from its art (#683, TECH-008/TECH-009).

Each placement in `content/greyhaven/dressing/*.json` that names a `collider` takes its asset's
`collisionMesh` from the kit's descriptor (a closed, outward-wound triangle mesh in metres in the
asset's frame; or, for a kit that declares only `collisionProxies` boxes, the wolf den's (#963),
its solid boxes as one such mesh), scales it by the placement's `scale` (thousandths per axis), turns it by `yaw`
(65 536 per turn about +Y, as the Outpost layout's) and writes it in units at the placement's
`position`. A placement's `grip` (`rock` or `sheer`, #1195) goes with its mesh: it is what
makes the generated collider climbable. The result, `crates/mmorpg-core/content/collision/greyhaven-dressing.json`, is
committed and reviewed like any content change: the zone reads only that file, so a new art
release changes no collision until this is rerun and its diff accepted.

Run after `scripts/fetch-bundles.sh`, from anywhere, with the standard library only:

    python3 content/greyhaven/authoring/dressing_collision.py
"""
import hashlib
import json
import math
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[3]
DRESSING = ROOT / 'content/greyhaven/dressing'
OUT = ROOT / 'crates/mmorpg-core/content/collision/greyhaven-dressing.json'
# Each kit by its asset-key prefix: the fetched bundle and its descriptor attachment.
KITS = {
    'crag.kit.': (ROOT / 'web/public/scenery/crag.kit', 'crag-descriptor'),
    'wolf-den.kit.': (ROOT / 'web/public/scenery/wolf-den.kit', 'wolf-den-descriptor'),
    'gatherables.kit.': (ROOT / 'web/public/scenery/gatherables.kit', 'gatherables-descriptor'),
    'settlement.props.': (ROOT / 'web/public/scenery/settlement.props', 'settlement-descriptor'),
    'discoveries.kit.': (ROOT / 'web/public/scenery/discoveries.kit', 'discoveries-descriptor'),
    'sandstone.kit.': (ROOT / 'web/public/buildings/sandstone.kit', 'sandstone-descriptor'),
}
UNITS_PER_METRE = 100
# A placement's `grip`: `rock` climbs, `sheer` does not (#1195); without one it does not climb.
GRIPS = ('rock', 'sheer')


def descriptor(bundle, name):
    manifest = json.loads((bundle / 'manifest.json').read_text())
    for asset in manifest['assets']:
        for attachment in asset.get('attachments', []):
            if attachment.get('name') == name:
                data = (bundle / attachment['path']).read_bytes()
                return json.loads(data), hashlib.sha256(data).hexdigest()
    raise SystemExit(f'{bundle} attaches no {name}; run scripts/fetch-bundles.sh')


def boxes_mesh(proxies):
    """A kit's solid `collisionProxies` boxes (the wolf den's, #963), with the walkable decks and
    ladders of the discoveries kit (#913), as one closed, outward-wound triangle mesh in the
    asset's frame. A placed door never opens, so its `dynamic-door` leaf is solid; a `roof` box
    (the sandstone kit's) is the gable prism inside it, its ridge along X at the box's top."""
    vertices, triangles = [], []
    # Each face's corners counter-clockwise seen from outside, as bit masks of (x, y, z).
    faces = [(0, 2, 6, 4), (1, 5, 7, 3), (0, 4, 5, 1), (2, 3, 7, 6), (0, 1, 3, 2), (4, 6, 7, 5)]
    # The prism's eaves (-Z, +Z) and ridge at each end, -X then +X, and its outward faces.
    prism = [(0, 1, 2), (3, 5, 4), (0, 3, 4), (0, 4, 1), (1, 4, 5), (1, 5, 2), (2, 5, 3), (2, 3, 0)]
    for proxy in proxies:
        if proxy.get('tag') == 'roof':
            base = len(vertices)
            (cx, cy, cz), (sx, sy, sz) = proxy['center'], proxy['size']
            for x in (cx - sx / 2, cx + sx / 2):
                vertices += [[x, cy - sy / 2, cz - sz / 2], [x, cy - sy / 2, cz + sz / 2],
                             [x, cy + sy / 2, cz]]
            triangles += [[base + a, base + b, base + c] for a, b, c in prism]
            continue
        if proxy.get('tag') not in ('solid', 'walkable', 'ladder', 'dynamic-door'):
            continue
        base = len(vertices)
        for corner in range(8):
            vertices.append([c + (s / 2 if corner >> (2 - axis) & 1 else -s / 2)
                             for axis, (c, s) in enumerate(zip(proxy['center'], proxy['size']))])
        for a, b, c, d in faces:
            triangles += [[base + a, base + b, base + c], [base + a, base + c, base + d]]
    return {'shape': 'triangle-mesh', 'vertices': vertices, 'triangles': triangles}


def collision_meshes(value):
    """Every entry with a `collisionMesh`, or else `collisionProxies` boxes, by key, wherever
    the descriptor lists it."""
    found = {}
    if isinstance(value, dict):
        if isinstance(value.get('key'), str) and isinstance(value.get('collisionMesh'), dict):
            found[value['key']] = value['collisionMesh']
        elif isinstance(value.get('key'), str) and isinstance(value.get('collisionProxies'), list):
            found[value['key']] = boxes_mesh(value['collisionProxies'])
        for child in value.values():
            found.update(collision_meshes(child))
    elif isinstance(value, list):
        for child in value:
            found.update(collision_meshes(child))
    return found


def units(metres):
    # Half away from zero, the same on every platform.
    scaled = metres * UNITS_PER_METRE
    return int(math.floor(scaled + 0.5)) if scaled >= 0 else -int(math.floor(-scaled + 0.5))


def main():
    kits, sources = {}, {}
    placements = []
    for path in sorted(DRESSING.glob('*.json')):
        placements += json.loads(path.read_text())['placements']
    meshes = []
    for placement in placements:
        if placement.get('collider') is None:
            continue
        asset = placement['asset']
        prefix = next((p for p in KITS if asset.startswith(p)), None)
        if prefix is None:
            raise SystemExit(f'{placement["id"]}: no kit draws {asset}')
        if prefix not in kits:
            bundle, name = KITS[prefix]
            value, digest = descriptor(bundle, name)
            kits[prefix] = collision_meshes(value)
            # The bundle's export identity, from its release's `export.ref.json`.
            release = bundle / 'export.ref.json'
            sources[name] = {
                'descriptor': digest,
                'bundle': json.loads(release.read_text())['sha256'] if release.exists() else None,
            }
        mesh = kits[prefix].get(asset)
        if mesh is None or mesh.get('shape') != 'triangle-mesh':
            raise SystemExit(f'{placement["id"]}: {asset} has no triangle collisionMesh')
        sx, sy, sz = (s / 1000 for s in placement['scale'])
        turn = placement['yaw'] / 65536 * math.tau
        c, s = math.cos(turn), math.sin(turn)
        vertices = []
        for x, y, z in mesh['vertices']:
            x, y, z = x * sx, y * sy, z * sz
            # mmorpg_presentation::geometry::ry: +Z turns toward +X.
            vertices.append([units(c * x + s * z), units(y), units(-s * x + c * z)])
        generated = {
            'id': placement['collider'],
            'position': placement['position'],
            'vertices': vertices,
            'triangles': mesh['triangles'],
        }
        # The placement's climbable or sheer setting (#1195), which the zone's climbing reads.
        grip = placement.get('grip')
        if grip is not None:
            if grip not in GRIPS:
                raise SystemExit(f'{placement["id"]}: unknown grip {grip!r}')
            generated['grip'] = grip
        meshes.append(generated)
    meshes.sort(key=lambda mesh: mesh['id'])
    ids = [mesh['id'] for mesh in meshes]
    if len(set(ids)) != len(ids):
        raise SystemExit('two placements share a collider')
    # One mesh per line, so a review diff shows which rocks changed.
    lines = ['{', f'"generator":"content/greyhaven/authoring/dressing_collision.py",',
             f'"sources":{json.dumps(sources, sort_keys=True, separators=(",", ":"))},', '"meshes":[']
    lines += [json.dumps(mesh, separators=(',', ':')) + (',' if i + 1 < len(meshes) else '')
              for i, mesh in enumerate(meshes)]
    lines += [']', '}']
    OUT.write_text('\n'.join(lines) + '\n')
    print(f'Generated {len(meshes)} collision meshes into {OUT.relative_to(ROOT)}')


if __name__ == '__main__':
    main()
