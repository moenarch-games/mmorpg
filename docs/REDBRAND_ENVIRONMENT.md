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
controls a smooth entrance transition: outdoor exposure approaches
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

## Broken Hollow ridges

The seven surrounding ridges (400–406) now have unequal peaks, lower saddles, irregular
shoulders and buried feet instead of nearly level crowns. Their established lower camp-facing
footprints remain, so the camp floor and existing approach lanes stay open. The simplified
closed meshes remain in `redbrand.json` under their existing IDs; only those seven meshes
change in this revision. Rapier still owns collision, independently of textures and visual LOD.

Ridge 402 keeps the west rope's attachment plane and its original 12 m rise. A short plateau
at z = -100.5 m supports the pull-up, with separate cap polygons so triangulating the uneven
crown cannot turn that landing into a slope. A focused production-physics test climbs the
real rope, pulls onto this landing and verifies stable standing there. Other rough/sheer
climbability classifications remain unchanged.

Shared presentation varies the exposed ridges' stone strata and adds muted moss on gentler
upper faces using the existing rock PBR material. Cave floors/ceilings retain their stone
colours. Climbing scuffs on authored cliffs now use the existing rounded-rock facet projection
instead of rectangular bounds, preventing marks from floating beyond the sculpted rock.
Ordinary box cliffs and boulders retain their previous presentation paths.

Validation: 19 focused core tests passed (15 basin routes, three authored-collision tests and
the Hollow rope landing), as did the existing handhold geometry test, native compilation and
the browser/WASM production build. Full suites and native GPU acceptance were not run.
The live Blender context includes the new shapes and an approximate moss review material;
its simpler viewport material is not the game's PBR renderer. Shared policy revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.


Chromium walked through the Outpost's north gate and up the existing Hollow switchback to
the camp approach, then captured `artifacts/greyhaven-brook/hollow-ridges.png` without page
errors. The final capture includes the facet-fitted climbing marks. The camp's broad dirt
clearing and other older surroundings still need further visual refinement toward the
owner's reference; this is a completed ridge pass, not final acceptance of the whole area.

## Used camp ground and canvas tents

The camp is now grass with worn approach tracks, a bare fire apron and patches of gravel
and disturbed soil, rather than one continuous rectangular dirt layer. Four small scenic
tracks join the existing Hollow Road to the fire, tent entrances and boarded mine frontage.
They only paint the existing ground. Eighteen low shrubs use an independent presentation
seed and remain walk-through foliage; terrain, creature posts and camp access are unchanged.

`fortifications.py` now authors the existing three tents and four supply crates into the
shared static Greyhaven glTF bundle. Canvas has a colour/normal texture pair, small folds,
bound seams and closed flaps; the crates have plank faces, frames, braces and iron nails.
These remain MIT-authored assets on the existing native/browser import path. No tent
interiors, inventory or new gameplay interactions are introduced.

Each tent retains its ID (420–422), site, footing and 2.2 m peak. A simplified closed
pentagonal prism in `greyhaven-fort.json` replaces its old box; the decorative folds are
independent of this collision. This removes invisible upper box corners while keeping the
closed canvas solid. The crate colliders retain their existing positions and dimensions.
The existing resource manifest records thirteen ordinary glTF resources, including the
new canvas maps. Content revision is derived from the new immutable geometry; protocol and
canonical-state layouts are unchanged.

The focused Rapier test walks into a closed flap and drops a character onto an eave: the
flap blocks movement, while the steep roof sheds the character to the ground instead of
supporting it on an invisible box. That test and native
`every_placed_kit_entry_loads_from_the_live_bundles` passed, as did the browser/WASM build.
All thirteen authored resources match the manifest hashes. The live Blender world includes
the tents, crates, new track centrelines and 697 nearby nature instances. Full suites and
native GPU acceptance were not run. Policy source revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

The final in-game capture is `artifacts/greyhaven-brook/hollow-camp.png`. Chromium reached the
camp approach through the existing Outpost gate and Hollow switchback with no page errors;
the view includes the final grass/dirt balance, new tent/crate art and existing inhabitants.
The clearing remains an established hostile camp, not a new settlement or quest area.

### Cave interior and north-mouth detail

The same Blender source now authors four small hanging formations (stable IDs 660–663)
above the lower chambers and two fractured entrance jambs (664–665). These are coarse,
closed static collision meshes in `redbrand.json`; the walking route and elevated south
shelf retain their headroom. The sealed deeper threshold keeps its existing collider and
now has textured oak boards, framing and iron straps in the ordinary fortifications bundle.

An actual walk to the north entrance exposed a roof-cutaway presentation bug: an interior
camera could see the sky through the hidden exterior roof. The shared presenter now keeps
the roof's downward faces as one-sided textured rock, while the exterior shell still cuts
away. Existing backface culling hides that underside from overhead cameras. Both clients
reuse this geometry; no camera API, movement authority or collision format changed.

Focused checks: the two shared cave-ceiling/cutaway tests, the Rapier entrance/walls/shelf/
ceiling/recovery test, native bundle loading and the browser/WASM build passed. Full suites
and native GPU acceptance were not run. Policy source revision remains unchanged.

Chromium walked the surface route to the north mouth and captured
`artifacts/greyhaven-brook/cave-interior.png` without page errors. The ceiling and hanging
formations are visible from below under the existing torch lighting. The local `.blend`
is saved with a north-entrance view. Larger chamber walls still need an organic-shape art
pass; this does not yet meet the entire reference's visual target.

The next shape pass chamfers both chamber dividers within their former footprints and
recesses the upper wall shoulders with fixed, source-authored variation. Lower perimeter
contact planes, the four-metre passage and shelf footing are preserved. This improves the
interior silhouette without a decorative shell concealing different collision geometry.
The entrance arch's upper edge follows a broken stone profile instead of a rectangular
lintel. The entrance clearance below it is unchanged; the focused Rapier traversal test
also covers this final exported shape. The browser build passed for the chamber-shape
pass before this final upper-edge adjustment.
