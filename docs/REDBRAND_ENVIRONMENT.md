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


## North entrance and rounded rock mass

The owner's next landscape revision moves the entrance to the north, away from the Outpost.
The Greyfang Approach leaves Stillwater's rim trail, wraps around the eastern flank and
turns back toward the north mouth. A broad core terrain terrace supports this longer path;
it never depends on decorative ground. The old south entrance is filled with solid rock. The new north mouth has a rough stone arch and an entrance torch.

`content/greyhaven/authoring/redbrand.py` is the Blender source for the new rounded, uneven
rock mass. It exports 17 cave meshes and seven surrounding Hollow ridge meshes directly to the existing collision JSON.
The mound replaces the rectangular walls/roof silhouette, while the two public chambers,
passage, floor support, ceiling and sealed deeper threshold remain. The raised shelf moves
to the south wall so the northern entrance reaches the lower floor without a blocker.
The shelf still provides an overlapping walkable elevation and blocks jumps from below.

Shared presentation draws the wall/floor meshes with the existing rock PBR material. The
mound roof uses the existing cosmetic roof cutaway path with its authored triangles and
rock/moss vertex colours; it does not draw its conservative bounding box. Cave eye adaptation
now follows entry from the north. Neither the visual roof cutaway nor its materials change
Rapier collision. The Hollow's surrounding cliffs keep their stable IDs and climbability;
the eastern ridge retains the existing rope face and its anchor.

Focused checks cover the northern terrain seam, both rooms, closed former mouth, shelf,
ceiling, recovery, longer exterior approach and the established basin routes. The connected
water ribbon is sampled against actual rendered terrain triangles, including both upper
channels. Native compilation and the browser/WASM build remain the client integration checks;
these do not imply native GPU acceptance or a full workspace suite.
