# Redbrand environment pass

## Breakable cave supplies

Two targetable crates (926–927) and one ordinary barrel (928) stand along the outer
chamber's western wall, at `(49, -139)`, `(49, -138)` and `(49, -137)` metres XZ.
Their feet rest on the authored floor at 11 m. The entrance, south shelf approach,
inner passage and sealed threshold remain clear. These are supplies, not powder barrels.
They reuse the existing no-loot breakage, five-minute clear-spot respawn and shared
client presentation. Breaking the row can produce twenty panels, staves and lids.

`cargo test -p mmorpg-core --test world breakables::redbrand_cave_supplies --locked`
exercises each shipped placement with its real template, terrain and cave triangles,
isolating it from the garrison and neighbouring intact props. Normal melee breaks it;
airborne pieces slow or rebound at the western wall, settle on the cave floor and
despawn. Recovery from breakage reproduces every subsequent canonical tick exactly.
This is the representative gameplay rigid-mesh workload for #719, not a timing benchmark.
The existing protocol combat workload's spawn-index sampling also shifts when the row is
added: `vale-fights-8` emits 1,825,497 bytes (31,188 above its former ceiling) and
`vale-fights-32` tests 431,424 candidates (6,705 above its former ceiling). Those two existing ceilings are
updated; other ceilings and all replay/recovery assertions remain in force.
Native/browser visual acceptance and the full workspace suites were not run.

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
walls' coarse presentation as well as physics collision. Fine detail comes from the
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
physics collision. The Hollow's surrounding cliffs keep their stable IDs and climbability;
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
change in this revision. Physics still owns collision, independently of textures and visual LOD.

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

## Hollow cliffs grown out of the ground (#1195 part 3)

The previous ridges stood on a vertical 3 m base wall under a sloping slab, and their skin
painted a colour band every 1.8 m of height, so they read as a plinth with horizontal stripes.
`content/greyhaven/authoring/hollow_cliffs.py` now authors the seven ridges (400–406). It runs
headless with `python3` or in Blender, reads the ground from `export-site-context`, and replaces
only those seven meshes in `redbrand.json`. `redbrand.py` keeps authoring the cave and leaves
the ridges alone. Each ridge is one closed rock mass. Its foot flares out and is buried 1.1–1.6 m
along an uneven line, its face leans back unevenly, and it ends in a broken crest with a walkable
shoulder. Nothing overhangs.

The footprints at the camp's ground stay within the previous ridges' XZ bounds, which are
pinned. The scenery scatters its grass, flowers and bushes from one seeded stream that rejects
candidates against those bounds, so wider bounds would reshuffle about 1,800 placements across
the region. With the bounds pinned, `nature.json` and every other placement are unchanged.

Climbability now comes from each mesh's `"grip": "rock" | "sheer"` (core
`cave::climbable_rock`) rather than from an ID range. As before, everything climbs except the
back wall 404. Climbable rock has strata jogs, slow buttresses and gullies, and vertical cracks.
The sheer wall is a smooth, steep sweep. The worn handhold routes on these cliffs are spaced
about every 8 m instead of 4 m. Ridges 400 and 403 wear a little climbing-kit ivy as world
dressing (`dressing/hollow-cliffs.json`), placed on their collision like the highland crags'
ivy. The old box-face ivy cover on 400 is gone. Ridge 402 keeps the rope: its west face stays
upright at x = 22 m round the rope, and its 12 m plateau is still the ridge's top.

The landscape rock's tone (`cliffs::landscape_color`) now drifts in world-space noise on planes
sheared by height, so it varies up a face without bands. The rock PBR material draws the grain.
The cave roofs share the same colouring. Live Blender review collection:
**Redbrand Hollow re-model (1195)**.

## The Redbrand quarry (#1195 part 3b)

Owner decision (2026-11-24, from the live-Blender mock-up "Redbrand quarry mock-up (1195 3b)"):
the seven ridges give way to one worked quarry, and the cave becomes an old mine.
`hollow_cliffs.py` now generates one continuous horseshoe wall round a floor of about
38 × 42 m centred at (0, −113), 16 m north of the old camp. The rim stands 11–15 m above the
floor. One 12 m gateway cuts 8 m through the rim on the **west** side between two natural rock
spurs, so the Outpost to the south sees the closed south wall.

- **Faces.** The north and south walls are worked faces: 4.4 m vertical cuts with 2.5 m
  benches, broken by three scree cones. The east wall is one smooth, sheer cut face (404's
  old role). The rim is a grassy shoulder, and the outside falls away as natural rock
  buttresses. The outer slope is shortened toward the south, so its foot keeps off the basin
  rim and the Hollow Road.
- **Segments.** The wall is rings of one blended profile (benched cut, sheer cut, natural rock,
  scree) round the inner foot. It is split into nine closed meshes (400–408, 9–23 m each),
  each with its own `grip`; only 404 is sheer. Arc segments share their seam ring, so a climb
  crosses a seam on one face. The gateway's spurs (400, 408) overlap the arc at the gateway's
  inner corners. There each point takes the height of the face it lies nearest, so the
  corners meet in a mitre. Every height is relative to the 11 m datum (buried 1.2 m), so the
  script needs no exported ground and regenerates byte for byte.
- **Rope.** `ROPES[0]` hangs down the south wall at x = 3.5 m. There the inner foot is a
  straight cut at z = −92.6 m, the face is upright and the rim is a level landing at 13 m,
  the highest point of its segment (406).
- **Old mine.** The mine entrance (410) is a timbered adit at the foot of the east wall,
  opening west. It has two sets of posts, caps, braces, lagging and half-torn boards over a
  dark recess. Garrick holds it. Cart rails run west from it across the floor and out
  through the gateway, with a branch to a spoil heap in the north-east corner and a mine cart
  on the line. A hand winch stands over an old shaft on the east rim, and a wooden derrick by
  the south wall reaches over stacked cut blocks. These workings are blockout-level
  presentation (`mmorpg-presentation/src/quarry.rs`). The cut blocks and the ivy on a few
  cuts are world dressing (`dressing/hollow-cliffs.json`). Finished mine art would be a later
  proposal.
- **Camp.** The tents, crates, fire, bandits, powder watch and spike pit turned with the
  layout onto the floor. The powder watch keeps its exact spacing. The spike pit is at the
  gateway's inner end. The watch post stands outside the gateway's north spur and looks down
  the approach. The boulder trap rests on the south spur's rim above the gateway: creature
  spawns stand on the terrain, so content now names **perches**, spawns that rest on authored
  rock at a fixed height (`ZoneContent::with_perches`, `greyhaven_vale::perches`).
- **Routes and area.** The Hollow Road's last leg runs west along the foot of the south wall,
  round the south spur and in through the gateway. The Wolfrun Ridge Path ends at the gateway,
  and the night raid's survivors retreat the same way. Area 5's footprint grows to cover the
  quarry ([−47, −153]–[38, −70] m); Greyfang Ridge's now starts at z = −153 m, with seeds
  that keep the Greyfang cave in it. `areas.json` is regenerated. The wider footprints moved
  the scenery's seeded nature scatter across the region (owner-accepted).

The Redbrand cave, its north mouth, the Depths threshold and the cavern spawns are unchanged.
Live Blender review collection: **Redbrand quarry (1195 3b) built**.

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

The focused physics test walks into a closed flap and drops a character onto an eave: the
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

Focused checks: the two shared cave-ceiling/cutaway tests, the physics entrance/walls/shelf/
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
lintel. The entrance clearance below it is unchanged; the focused physics traversal test
also covers this final exported shape. The browser build passed for the chamber-shape
pass before this final upper-edge adjustment.

### Broken outer ridge and textured cutaway roofs

Redbrand's roof now has a lower sealing shoulder and three unequal, tilted rock spines
instead of one domed crown. The coarse closed components remain part of static ID 547 in
`redbrand.json`; their buried roots sit above the unchanged chamber ceiling. The north
entrance, wall contact planes, shelf and approach remain unchanged. The live Blender source
and saved world context contain the revised ridge.

The authored roof exterior now carries the existing rock material, metre-scale UVs and
matching tangents, as its walls already do. Native rendering uses its existing ground-material
path. The browser routes fixed textured roofs into the existing terrain-material batch at
their shared rest transforms, then applies the owning scene part's cutaway visibility.
Moving flames, sails and doors keep their existing presentation path. This also textures
Stillwater's joined rock shelf. No shader, renderer, asset bundle, cache or authority is added.

Focused checks passed: Redbrand's physics entrance/walls/shelf/ceiling/recovery test; both
shared ceiling/cutaway tests (now also checking exterior texture attributes); the five browser
scene-building tests, including actual batched roof visibility; and the browser/WASM build.
Native GPU acceptance and full suites were not run. The previously reported legacy A/B
mesh-bound failures remain unresolved; no Physics Engine parity work is included.
Convention source revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

The final in-game capture is `artifacts/greyhaven-brook/redbrand-crag.png`. Chromium walked
the existing eastern approach to the north mouth with no page errors. An initially exposed
root under the western spine was lowered into the sealing shoulder and the join was
rechecked in-game. The saved Blender world contains that final geometry. Roof surfaces stay
resident independently of visual terrain tile LOD; neither the material batch nor cutaway
visibility changes collision. The straight background escarpment remains a visible landscape
issue for a subsequent art pass, so the full woodland-valley reference is still in progress.

### Weathered escarpment behind Greyfang Ridge

The northern backdrop now uses fixed integer weathering on the authoritative heightfield:
unequal buttresses and gullies, a toe varying by up to eight metres, steep middle faces and
a broken crest. The existing pass lanes, their neighbouring grid cells and the marked hard
climbing route retain their former profiles. Trees are rejected where the actual sampled
face is too steep to support their roots. Both clients draw the same terrain through their
existing presentation; no visual-only collision, new terrain engine or streaming is added.

The pass journey check exposed an old approach crossing the remodelled cave's rock. The
Highland Pass now joins Greyfang Approach beyond the north mouth at (+64, −160) m and bends
to (+52, −176) m before its established switchbacks. Roads, map and tree clearance derive
from those core points. The test starts at that real join and still walks to the stronghold
and back. The barrier check requires ordinary walking to stop below the first pass ledge;
the marked climbing-route check also passes.

Focused checks: both `highlands` gameplay tests and
`the_climbing_route_beside_the_highland_pass_leads_up_the_escarpment` passed, along with the
browser/WASM production build. The live Blender terrain context extends far enough north
to inspect the escarpment and contains 1,331 current nature instances over that larger
preview area. This changes the authoring view, not the playable world's extent.
Full suites and native GPU acceptance were not run. Legacy A/B failures remain outside
this slice. Convention source revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

The final Chromium capture is `artifacts/greyhaven-brook/weathered-escarpment.png`; the player
walked to the north cave approach without page errors. The northern face has broken ribs
and a varying toe. The eastern backdrop and some trees on its slopes still need refinement;
this pass does not establish completion of the overall reference direction.
