# Greyhaven's Waterworn Gorge

The village occupies a rounded, asymmetric basin with an adjoining northern lobe. Fixed-seed integer weathering varies the cliff outline and shoulders. The heightfield is still authoritative: scenery samples the same terrain, and the six existing switchback lanes override the weathering. This is separate from visual terrain tile LOD and the authored Redbrand Cave meshes.

`mmorpg-core/src/content/greyhaven_vale/brook.rs` authors Stillwater's upper inflow and rim outlet, the west-cliff waterfall and a narrow brook through the village, a timber footbridge, and a low drainage opening through the southern palisade. Its centimetre centreline supplies the shallow terrain bed and a `WaterChannel` footprint. Existing wading/swimming rules apply; foam motion is cosmetic and exerts no current. The upstream river enters the existing Stillwater ellipse; its outlet loops around the rim to the waterfall. Both upper channels share the lake's surface height. Neither water shape introduces another movement solver.

The bridge deck, approaches, rails and culvert are ordinary stable-ID colliders/ramps. Channel points and width participate in the immutable content revision. Rebuilding or recovering a zone therefore uses the same terrain, water and colliders; there is no protocol or canonical-state layout change.

`content/greyhaven/outpost-layout.json` adds three modest cottages using the existing collision authoring and textured Outpost kit. Four named residents reuse existing rigged villagers; they have no new quest or vendor interaction. Elden's daily route uses the open lane beside the northwest cottage.

Both clients use `mmorpg-presentation/src/brook.rs` for water ribbons, moving foam, wet banks, reeds and bridge timber. New caves continue to use the separate authored collision geometry described in [Redbrand Cave](REDBRAND_ENVIRONMENT.md). No collision streaming or hydrology system is introduced here.

## Focused validation

- `cargo test -p mmorpg-core --test vale_basin --test village_brook --locked`: all six climbs, rim trails, cave approach, bridge, shallow water exit, gate, resident alley, low culvert and checkpoint continuation.
- `cargo check -p mmorpg-client --locked`: native integration compiled.
- `cd web && bun run build`: WASM and browser production build.
- `cd web && bun test tests/scene-building.test.ts`: shared scene integration (before the final basin silhouette adjustment).
- Chromium gameplay walk across the bridge and an in-game overview capture; no page errors. Native GPU acceptance and full suites were not run.

The live Blender scene is `Greyhaven - Waterfall and Village 513`; the saved working project and browser capture are local artifacts under `artifacts/greyhaven-brook/`. Blender uses actual terrain samples and packaged cottage meshes; resident empties mark game NPC positions. The playable build remains the reference for vegetation, characters, lighting and water animation.

Shared convention resolution: `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

## Settlement art revision

The owner review of the first overview requested a more finished keep, less uniform terrain,
less rectangular defences, usable water joins, a better well position and a livelier square.

`content/greyhaven/authoring/fortifications.py` authors the stone keep, slate roofs, four
covered timber watchtowers, iron-bound open gate frames and faceted palisade in Blender.
It exports ordinary glTF with colour/normal textures to the committed
`web/public/authored/greyhaven-fort/` bundle. Both clients reuse their existing static bundle
loaders and shared Outpost placement. There is no new renderer, importer or asset cache.
The source script also exports simplified closed triangle meshes to
`crates/mmorpg-core/content/collision/greyhaven-fort.json`. The old wall IDs remain; short
additional wall sections, roofs and rock outcrops have fixed IDs. Rapier consumes triangles,
while conservative mesh bounds are used only for content placement checks. The rendered
triangles remain visible as a fallback if the art bundle cannot load.

The keep remains a closed building. Watchtowers have solid timber bases and covered lookout
platforms; this revision does not add tower interiors or ladder gameplay. The remaining
closed blockout houses now use the existing textured cottage kit too.

Physical knolls and rock outcrops break up the basin's flat floor and rim. Foundation areas,
spawn slots and the existing route lanes remain usable. Presentation relief is suppressed
inside the built settlement so buildings, residents and the channel share the actual ground.
The brook has a wider, deeper sampled bed; its waterfall starts on exposed floor and its
southern drain precedes the rising switchback. Road overlays subdivide and mask at the ford
rather than spanning the water. A focused presentation test samples the real rendered
terrain triangles across the entire water ribbon.

The well now stands at (11, 30) m, clear of the crossing and brook. A smaller visible gathering
square leaves the original spawn grid intact. A produce stall and eight more named residents
bring the total NPC catalog to 21 (20 in the Outpost, plus Farmer Osric). Four small flower
beds dress cottage fronts. Residents retain ordinary existing looks and interactions.

Revision checks: the 16 core basin/brook tests (including all four gates and a wall push),
`brook_water_clears_the_actual_terrain_triangles`,
`named_greyhaven_residents_have_presets_and_classes_their_weapons`, native
`every_placed_kit_entry_loads_from_the_live_bundles`, format and the browser/WASM production
build passed. The updated browser overview is `artifacts/greyhaven-brook/in-game-polished.png`.
The keep/gate/watchtower geometry and six authored colour/normal images are committed;
Blender is needed only to edit/re-export them, never to start the game.


## Connected water and north-side cave approach

The upper river enters Stillwater from the northeast; a separate outlet leaves the northwest
shore, runs behind Redbrand Hollow, follows the western rim and meets the waterfall's lip. Those two authored
channels use Stillwater's existing 11.15 m surface. Their beds are carved into core terrain;
nearby tree placement and presentation relief respect the same banks. Existing roads cross
as shallow fords. This removes the disconnected spring puddle without changing the shrine.
Flowing foam shows the authored directions; there is still no hydrology simulation.

The basin now has uneven crests and gullies instead of one level cliff lip. Named route
corridors, the village foundations, the lake and the farm apron retain their working levels.
All authored collision surfaces are passed through to scenery, including the small rock
crags and exact timber fallback surfaces, rather than only the original cave ID ranges.

Redbrand's north mouth and longer eastern approach are described in
[the environment authoring notes](REDBRAND_ENVIRONMENT.md#north-entrance-and-rounded-rock-mass).


For this connected-landscape revision, 20 focused core tests passed (cave collision and
recovery, both chambers, the old-mouth blocker, all basin routes, the longer cave approach,
water joins, upper ford and village traversal). The presentation clearance test covers the
lower brook and both upper channels; an additional core assertion keeps the outlet outside
Hollow wall bounds. Native compilation and the browser/WASM production build passed.
The live Blender context is saved as `artifacts/greyhaven-brook/connected-landscape.blend`.
A wide browser capture timed out on the software renderer; a smaller Chromium overview
then succeeded without page errors at `artifacts/greyhaven-brook/connected-landscape.png`.
Native GPU acceptance and full suites were not run. The source convention revision remains the one recorded above.


## Woodland-valley reference: first landscape pass

The owner's supplied reference guides the ongoing art work (Atlas 5.1.i513-4). Seven composed
ridge summits add unequal rock spurs and saddles above the original terrace. They are core
heightfield content, so walking and climbing use the same ground that both clients draw.
The protected foundations, switchbacks, farm, lake and road corridors retain their levels.
A full terrain-cell buffer around the water banks prevents taller adjoining triangles from
clipping the narrow upper channels.

Seeded patches of low scrub dress the shoulders using the existing nature bundle. Bushes
remain walk-through foliage and their separate seed does not reroll older placements.
The dirt tracks have a darker earth palette and a varying worn width inside the unchanged
road corridor; the small gathering patch now has a rounded, broken edge. Near and distant
road colour use the same worn-width function. These are presentation changes, independent
of authoritative movement and terrain LOD selection.

This is the first reference-directed pass, not a claim that the complete reference's visual
quality has been reached. Architecture details, woodland composition and lighting continue.

For this first reference pass, all 15 basin traversal tests and the shared presentation water
clearance test passed after fixing an observed outlet/terrain intersection. The browser/WASM
production build passed. Chromium walked the bridge and captured the updated overview with
all seven ground textures loaded and no page errors. The same Blender file now includes 512
instances of the actual game nature assets using shared WASM placement matrices. The
preview terrain materials remain simpler than the in-game PBR terrain. Full suites and native
GPU acceptance were not run; shared policy source revision is unchanged.

## Woodland composition and daylight pass

Three patchy groves now frame Stillwater's eastern shore, the Hollow's distant backdrop and
the southwest ridge. They append to core's existing jittered trunk generator with independent
fixed seeds, preserving earlier collider identities. Terrain height, road clearance, building
clearance and wet banks reject unsuitable placements; the highest spurs remain bare. This
adds 167 physical trunks (1,831 total). The existing bounded-content tree-count expectation
was updated to include the groves. Its procedural-footing assertion now distinguishes
conservative authored mesh bounds, bridge/culvert parts and explicitly placed cottage
plinths from boxes that derive their footing from terrain.

Shared nature presentation now mixes more conifers with broadleaf trees and varies canopy
height without widening the trunk footprint. Existing nature assets and their LOD selection
remain in use. The live Blender context contains 679 nearby nature instances at the exact
shared game matrices. Ground colours are muted olive/earth tones. Both clients use a
half-stop darker noon and reduced sky fill, retaining the existing day/night clock and cave
adaptation. The browser's fallback sky lighting receives the same adjustment.

This revision passed 16 focused basin/brook gameplay tests, the bounded deterministic
content test, native compilation, the browser/WASM production build and four existing
browser presentation tests. Chromium walked the bridge and captured
`artifacts/greyhaven-brook/woodland-lighting.png` with all ground layers loaded and no page
errors. The broad view remains hazier than the supplied reference; further atmosphere,
architecture and ground modelling are ongoing. No full suite or native GPU acceptance ran.
Policy source revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

## Millbrook architecture pass

The existing barn, farmhouse and windmill now share the authored Greyhaven bundle instead
of their procedural blockouts. `fortifications.py` models oxblood plank walls, barn bracing,
a closed loft hatch, limewashed farmhouse walls, timber windows, a chimney and slate roofs.
Four additional colour/normal PNGs describe painted boards and lime plaster. All geometry
is repository-authored and MIT-licensed; the existing glTF/static-bundle import path remains
unchanged. The windmill retains the shared animated sails and reduced-motion behaviour.
Blender shows a separate stationary sail preview, deliberately excluded from the export.

The buildings keep Millbrook's established footprint and 11 m terrace. Barn/farmhouse bodies
retain collider IDs 200/201. The square windmill box is replaced by a closed, tapered
20-sided static triangle mesh under its original ID 202. Roofs, chimney and axle housing
have simplified meshes 65060–65064 in `greyhaven-fort.json`, independent of the detailed
visual mesh. Rapier consumes these through the existing authored-collision loader. The
resource manifest contains hashes for the normal glTF resources; content revision still
comes from immutable gameplay content, with no protocol/canonical-layout change.

The focused Rapier test `millbrook_windmill_has_a_solid_round_footprint` walks through a
former square-box corner and then into the curved wall, checking the character reaches the
tower, stays out of the solid and can slide around it. That test and all 15 basin routes
passed. Native `every_placed_kit_entry_loads_from_the_live_bundles` and the final browser/WASM
production build passed; all eleven authored bundle resources match the manifest hashes.
Full suites and native GPU acceptance were not run. The buildings remain closed; this pass
does not add interiors, quests or farming rules. Shared policy revision is unchanged.

Chromium loaded the authored farm models and existing animated sails and captured
`artifacts/greyhaven-brook/millbrook-models.png` without page errors. This is an actual
in-game daytime view; the live Blender file also contains the new architecture. The wider
atmosphere and some older farm dressing (including the loading dock) remain for later art
passes within the same PR.

## Connected village tracks and clearer air

Two presentation-only foot tracks now connect the compact gathering patch to the four
existing road ends across the protected spawn lawn. Their slightly winding centrelines and
1.2 m half-widths are ordinary `Scenery::roads` entries appended by layout composition.
They paint the existing flat, walkable ground; core content, spawn capacity and movement
rules are unchanged. Near and distant ground use those same road entries. Seeded patches
of the existing masked path material blend the tracks into turf, and the gathering patch
uses a less pale earth colour.

Both clients use a 6 km atmospheric visibility distance with a more neutral haze tint and
milder colour separation. This reduces the uniform blue-grey veil over the nearby village
while retaining distant aerial perspective. Sky fill is slightly higher than in the previous
pass so shadowed timber and plaster remain readable. The sun, day/night clock, tone mapper,
cave adaptation and renderer are unchanged.

Native compilation and the browser/WASM production build passed. This is a presentation
pass; authoritative geometry and the successful farm/cave traversal checks from the earlier
passes are not rerun. Full suites and native GPU acceptance were not run. Shared convention
source revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

Chromium walked west/east across the gathering lawn, returned to the centre and crossed the
brook bridge without page errors. The updated in-game overview is
`artifacts/greyhaven-brook/village-tracks.png`; the live Blender terrain/road context was
refreshed from the same WASM scenery. The regular cliff/camp silhouettes are still visibly
short of the owner's reference and remain active modelling work.

The Hollow's nearly level enclosing walls have since been reshaped into broken ridges with
mossy shoulders; the rope landing and camp access remain usable. See the
[rock authoring notes](REDBRAND_ENVIRONMENT.md#broken-hollow-ridges) for collision, climbing
cue projection and the focused checks.

## Stillwater bank and outlet pass

The upper outlet now bends around Hollow in a sequence of broad, unequal reaches instead
of following long parallel straight lines. Its endpoints, lake level and waterfall join
remain the same. Core uses those points for both the water volume and the sampled bed;
shared presentation and the live Blender context read the same course.

Eight low rock clusters break up Stillwater's west, north and south banks. The Blender
source in `content/greyhaven/authoring/fortifications.py` exports their twenty-four closed,
coarse meshes as stable IDs 65070–65093 in `greyhaven-fort.json`. Rapier loads them through
the existing immutable static-mesh path; both clients texture those exact facets with the
existing rock material and moss palette. These are not detailed rendering meshes, new
water volumes or another asset bundle. The dock and shrine approach stay clear, and the
rocks avoid the existing lurker spawn points. The lake ellipse and its deep basin remain
unchanged; the outcrops break its visible shoreline rather than replacing water physics.

The stream banks now have unequal widths and a textured height blend into the surrounding
ground, using the existing masked mud layer. No new shader, renderer, streaming system or
movement authority was introduced. Content revision follows the changed immutable geometry;
protocol and canonical layouts are unchanged.

Focused checks passed: `cargo test -p mmorpg-core --test village_brook --locked`,
`cargo test -p mmorpg-presentation --lib brook_water_clears_the_actual_terrain_triangles --locked`,
and the browser/WASM production build. Full suites and native GPU acceptance were not run.
Convention source revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

Chromium walked the Lakeshore switchback to the dock approach without page errors. The
capture `artifacts/greyhaven-brook/stillwater-banks.png` shows the textured bank clusters,
reeds and open dock; a final small outlet bend adjustment followed the capture. The live
Blender context includes all twenty-four bank rocks and the current shared terrain/nature
placements. The existing blocky cover over the Sunken Shrine remains a visible art issue
for the next terrain pass; the full reference direction remains in progress.

## Continuous rock cover above the Sunken Shrine

The seventeen separate roof boxes over the flooded cave made a visible staircase through
Stillwater's eastern shore. `content/greyhaven/authoring/sunken_shore.py` now authors closed,
coarse rock sections with matching cross-sections at cell boundaries. Their stable IDs
580–596 remain unchanged. The resulting `sunken-shore.json` is embedded by
`shrine::collision_meshes()` and added to the same authoritative static-mesh definition as
Redbrand and the fortifications. The old roof-box generator is removed.

The cover rises out of the lake into the shore as a continuous shelf, widens over the air
room and meets dry ground beyond it. The existing tunnel ceiling and higher air-room
vault, shrine kit, flooded route, air pocket and relic quest are retained. Shared roof
presentation uses the same rock/moss palette in world space across adjacent cells;
the existing exterior cutaway still works. The shrine kit retains its vaulted interior
ceiling; a duplicate rock underside is omitted there. Redbrand keeps its one-sided textured
ceiling.

Focused checks: all six `sunken_shrine` gameplay tests passed, including swimming to the
relic, breath recovery and checkpoint continuation. The surface-walk test now also walks
above several cells and the air room, checking support, dry state and no relic retrieval.
The two shared cave cutaway/ceiling tests passed. Full suites and native GPU acceptance
were not run. Convention source revision remains unchanged.
The final browser/WASM build passed. Chromium again reached the dock approach without
page errors; `artifacts/greyhaven-brook/stillwater-shelf.png` shows the joined shore cover
instead of the stepped boxes. The live Blender project contains the shelf and 699 current
nearby nature instances. The in-game capture precedes the final interior-only ceiling
deduplication; the exterior geometry is unchanged by it.

## Eastern ridge and supported background woodland

The eastern ridge now has an unequal toe, buttresses, gullies and a broken crown on the
same authoritative heightfield as the northern escarpment. The Marsh Pass lanes, their
neighbouring terrain cells and the marked hard climbing strip retain their previous
profiles. Where the north and east landforms overlap outside the valley corner, core joins
their heights rather than cutting the eastern crest down to the northern toe. No water,
region boundary, quest or movement solver changes.

Physical trunks are rejected on unsupported ridge faces, including isolated peaks. The
floating-tree candidates traced in the cave view were decorative background trees. Their
placement now checks the drawn ground, including the relief beyond the playable boundary,
and rejects steep or sharply varying footing. The existing background-forest target is
filled from more suitable candidates; this does not add client collision or tie authority
to visual terrain LOD.

Focused checks: the Marsh Pass/island round trip, reed ford and ordinary-walking barrier
passed; the marked marsh climbing route passed; the browser/WASM production build passed.
Full suites and native GPU acceptance were not run. The existing legacy A/B failure remains
unresolved. Convention source revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

The final export retains 700 decorative background trees after slope rejection and
replacement sampling. The wider live Blender context contains 1,457 current nature
instances. Chromium completed the village crossing without page errors and captured the
whole area in `artifacts/greyhaven-brook/greyhaven-eastern-ridge.png`. The distant cliff
materials and overall lighting still look flatter than the owner's reference; the broader
visual direction remains in progress.
