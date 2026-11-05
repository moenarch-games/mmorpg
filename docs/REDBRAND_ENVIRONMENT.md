# Redbrand environment pass

The first beta environment pass (#641) replaces the large stacked cliff slabs
with fractured rock surfaces and the existing `greyhaven.terrain.rock` PBR material
(base colour, normal, occlusion/roughness/metallic). Texture coordinates follow each
face in world metres, including vertical walls and the shelf, with matching tangent
frames. Both clients use the same geometry and packaged material. Other cliffs
reuse the surface treatment; visual terrain tiles and collision residency stay
separate.

Redbrand's seven cave walls were modelled in Blender as stratified profiles, with
recesses and overhangs above head height. The simplified profiles are saved in
`crates/mmorpg-core/content/collision/redbrand.json` and used directly for those
walls' coarse presentation as well as Rapier collision. Fine detail comes from the
separate PBR material, not extra collision triangles. The existing entrance,
body-height wall contact planes, floor, sloped shelf approach and sealed deeper
threshold remain. Geometry changes naturally change the content hash; no wire or
canonical layout changes are involved. Drop zero-area triangles created along
collinear polygon caps when exporting, before core validation.

The live authoring scene is **Redbrand Cave - Beta Environment 641**, with a separate
**Redbrand Authored Rockwork** collection. It preserves the previous collision study.
A local working project is saved under ignored
`artifacts/redbrand/redbrand-beta.blend`; it includes actual surrounding game terrain
as read-only context from the WASM scenery export. Blender's rock shader is a review
material; the game consumes the existing packaged terrain PBR textures. JSON remains
the portable runtime source; neither client needs Blender. The shared
`mmorpg-presentation::cliffs` code supplies the other cliffs' shallow surface detail.

Four wall torches stand above head height in the public cave. Their visible
shafts, cups and flames share native/browser geometry. Each casts warm 9000 lm
light over 16 m, with reduced-motion-aware flicker. The viewer's projected position
controls a smooth eight-metre entrance transition: outdoor exposure approaches
EV100 7.5 in daylight, direct sky illumination is attenuated by up to 1000× and sky
reflection by a further 10×. Night is never brightened past its outdoor exposure.
These are cosmetic defaults, not new gameplay visibility, stealth or time rules.
The debug camera cannot enter or leave this lighting region for the character.
The existing day cycle, atmospheric sky, sun shadows and material provider remain.

The browser batches these static rock surfaces with the existing rock terrain
material, sharing its texture resources. Native lowering uses the same ground
material metadata and GPU texture library. Missing material assets retain the
explicit vertex-colour fallback.

Focused checks are the authored-collision traversal/recovery tests, browser
scene-building tests, native compilation and the browser production build. Native
GPU appearance and the full workspace/browser suites are not implied by these
checks. This is a first rock/cave environment pass, not a claim that every Greyhaven
building, biome or asset has received final beta art.
