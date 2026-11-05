# Authored static collision

`mmorpg-core::StaticMesh` stores a stable static collider ID, a translation,
integer-centimetre vertices relative to that translation, and indexed triangles.
Game coordinates are X, Y-up, Z. Winding points out of solid rock. Use simplified,
closed surfaces for walls/slabs; do not export decorative render geometry or LODs.
Rapier's fixed triangle meshes resolve contact, capsule movement, rays, projectiles
and dynamic props through the existing world. No new solver or spatial index is used.

The first asset is
[`redbrand.json`](../crates/mmorpg-core/content/collision/redbrand.json), authored
in Blender 5.2.2. It replaces the existing public cave's wall/roof boxes, adds
floor slabs meeting the 11 m terrace and a 2.2 m elevated shelf with a sloping
approach. There is 2 m clearance below the shelf and 2.3 m above it. The entrance,
chambers, garrison and sealed deeper-dungeon threshold retain their existing roles.

`cave::collision_meshes()` embeds the JSON at compile time; `base_definition()`
adds it through `ZoneDefinition::with_static_meshes()`. Outpost layout compilation
preserves those meshes. Validation rejects duplicate/overlapping IDs, invalid
indices, degenerate triangles, out-of-range coordinates and occupied spawn slots.
Conservative bounds support existing content placement and nearby-body selection;
the physics adapter uses the triangles, not their bounding boxes. Placement checks
are deliberately conservative: split large hollow assets into smaller wall/floor
sections rather than using a chamber-sized bounding box around spawn positions.

The content hash includes all mesh geometry in ID order, so edits invalidate old
checkpoints automatically. Recovery and world cloning rebuild the same fixed
Rapier shapes from immutable content. No snapshot/wire layout changes, protocol ID
change, runtime filesystem access or Blender dependency are needed. Native and WASM
load the same geometry. The optional physics-engine A/B backend explicitly refuses
these meshes; parity work is outside this slice.

## Blender editing

Use a separate collection of low-detail meshes. Give each object an integer custom
property `collider_id` from a free static ID range. Blender coordinates are metres,
Z-up: `(game X, game Y, game Z) = (Blender X, Blender Z, -Blender Y) × 100`.
Export evaluated, triangulated geometry with transforms baked and rounded to whole
centimetres. Recalculate outward normals; discard decorative detail. This example
can be run from Blender's Python console to export a collection (set `output` to a
candidate file, then review it before replacing the committed JSON):

```python
import bpy, json
from pathlib import Path
collection = bpy.data.collections['MMORPG Redbrand Collision 639']
output = Path('/tmp/redbrand-collision.json')
meshes = []
for obj in sorted(collection.objects, key=lambda obj: obj['collider_id']):
    evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
    data = evaluated.to_mesh()
    try:
        data.calc_loop_triangles()
        vertices = []
        for vertex in data.vertices:
            p = evaluated.matrix_world @ vertex.co
            vertices.append([round(p.x * 100), round(p.z * 100) - 1100, round(-p.y * 100)])
        triangles = [list(t.vertices) for t in data.loop_triangles]
        if evaluated.matrix_world.determinant() < 0:
            triangles = [[a, c, b] for a, b, c in triangles]
        meshes.append(dict(id=int(obj['collider_id']), position=[0, 1100, 0],
                           vertices=vertices, triangles=triangles))
    finally:
        evaluated.to_mesh_clear()
output.write_text('[\n' + ',\n'.join('  ' + json.dumps(m) for m in meshes) + '\n]\n')
```

The `.blend` is disposable authoring state, consistent with the existing
[layout editor](WORLD_LAYOUT_EDITOR.md); committed JSON is the runtime source.
Run `cargo test -p mmorpg-core --test authored_collision --locked` after changes.
A detailed visual cave asset can be authored independently through asset-tooling;
the current walls/roof keep their existing presentation and the shelf has a shared
coarse visual fallback in both clients. Visual terrain tiles/LOD never load,
unload, replace or choose collision geometry.

## Extension and streaming

Other caves, cliff overhangs and stacked chambers can supply additional JSON
meshes with stable IDs and translations via the same definition API. A region
JSON compiler can emit this representation directly. Split assets by meaningful
static sections; include accessible approaches, floor seams and overhead clearance.
For floors below surrounding terrain, author the corresponding terrain opening
as well: this slice keeps the cave floor flush with the terrace and does not add
heightfield holes or remove the zone's floor half-space.

Everything is loaded with the zone today. Subsequent collision streaming needs a
separate authoritative region-residency contract, deterministic load/unload and
checkpoint reconstruction, safe seam overlap and rules preventing movement into
unavailable geometry. It must remain independent of the client's near/far visual
terrain tiles. No collision streaming, underground generation or navigation mesh
is implemented here.
