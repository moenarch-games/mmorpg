#!/usr/bin/env python3
"""Generates the world dressing's collision from its art (#683, TECH-008/TECH-009).

Each placement in `content/greyhaven/dressing/*.json` that names a `collider` takes its asset's
`collisionMesh` from the kit's descriptor (a closed, outward-wound triangle mesh in metres in the
asset's frame), scales it by the placement's `scale` (thousandths per axis), turns it by `yaw`
(65 536 per turn about +Y, as the Outpost layout's) and writes it in units at the placement's
`position`. The result, `crates/mmorpg-core/content/collision/greyhaven-dressing.json`, is
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
}
UNITS_PER_METRE = 100


def descriptor(bundle, name):
    manifest = json.loads((bundle / 'manifest.json').read_text())
    for asset in manifest['assets']:
        for attachment in asset.get('attachments', []):
            if attachment.get('name') == name:
                data = (bundle / attachment['path']).read_bytes()
                return json.loads(data), hashlib.sha256(data).hexdigest()
    raise SystemExit(f'{bundle} attaches no {name}; run scripts/fetch-bundles.sh')


def collision_meshes(value):
    """Every entry with a `collisionMesh`, by key, wherever the descriptor lists it."""
    found = {}
    if isinstance(value, dict):
        if isinstance(value.get('key'), str) and isinstance(value.get('collisionMesh'), dict):
            found[value['key']] = value['collisionMesh']
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
        meshes.append({
            'id': placement['collider'],
            'position': placement['position'],
            'vertices': vertices,
            'triangles': mesh['triangles'],
        })
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
