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

## Rock detail in the distant enclosing cliffs

The coarse terrain level previously drew every face with baked vertex colours. That made
the enclosing escarpments read as flat walls in the whole-area view despite their changed
profiles. `mmorpg-presentation::tiles` now routes its steep coarse triangles into the
existing rock material, with the near layer's face-aligned UVs and tangents while retaining
the terrain's smooth normals. The same triangle positions
and eight-metre sampling remain; gentle ground, roads, fields and skirts retain their
existing flat material. Rock faces replace their original coarse faces rather than adding
a coincident overlay. Both clients reuse their existing textured-ground material batches.

Tile dimensions, near/far selection, residency, prefetch, eviction, chunk coverage and skirts
are unchanged. This is presentation material selection, separate from authoritative
collision and subsequent collision streaming. No shader, renderer, asset bundle or cache
framework is introduced. The authoritative content revision remains
`16996712026689241795`; only presentation identity changes. Protocol/canonical layouts do
not change.

Focused checks passed: the four `terrain_tiles` tests (including surface/texture attributes,
coverage and skirts), the five browser scene-building tests and the browser/WASM build.
Full suites and native GPU acceptance were not run. Legacy A/B CI failures remain unresolved.
Convention source revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

An intermediate pass used a six-metre rock repeat; the authored-scale correction below supersedes it.
Chromium completed the village/bridge crossing without page errors and saved
`artifacts/greyhaven-brook/distant-rock-detail.png`. The wide view still exposes repetitive
cliff shading and overly broad distant paths; the reference art direction remains in progress.

## Keep silhouette and finished elevations

The Blender fortification source now covers the full keep hall with its slate roof rather
than leaving broad flat strips beside the earlier narrow roof. Two projecting stone dormers,
two capped chimney stacks, ridge/eave trim, shutters and window crossbars break up the front.
Side and rear elevations receive recessed, framed windows and return buttresses too.
The hall remains closed, with the existing portal and surrounding lanes unchanged.

The existing roof mesh keeps ID 65000. Four new simplified closed meshes, IDs 65065–65068,
cover the two stacks and dormers; fine window/roof trim remains presentation detail.
The ordinary shared glTF bundle and collision JSON are regenerated by `fortifications.py`;
there are still thirteen bundle resources, and the fortification JSON now has 71 meshes.
Content identity follows the immutable geometry hash; protocol/canonical layouts do not change.

Focused checks passed: native loading of every placed kit entry, deterministic bounded
Greyhaven content reconstruction and the browser/WASM production build. Chromium walked
the village crossing without page errors and captured the loaded keep in
`artifacts/greyhaven-brook/keep-roof-detail.png`. The live Blender project has the same
architecture and now previews terrain rock with the existing packed game maps at six-metre
scale. This local preview is not a replacement for the playable render. Full suites/native
GPU acceptance were not run; the legacy A/B CI failure remains unresolved.

## Fuller woodland edges and spawn clearance

Two appended fixed-seed rim groves continue the woods behind the western ridge and below
Millbrook. They add 145 collidable trunks using the existing tree authoring, road/structure
clearance, water exclusion and shared mixed-species nature kit. Earlier grove ordering stays
unchanged. The live Blender context now contains 1,590 current nature instances using the
same placement matrices as the game.

The content check also caught the produce stall's awning overlapping the spawn plaza by
six centimetres. Its origin moves half a metre south, retaining its compact-square placement
while clearing every spawn slot. The existing full 512-slot spawn test now passes, as does
the Wolfrun climb traversal. The browser/WASM production build passed.

The nine-test `greyhaven_vale` file initially returned four passes and five failures. A
comparison with the preceding committed content reproduced all five failures. The plaza
failure is fixed here; four pre-existing assertions remain unresolved (square palisade gaps,
overlapping conservative structure bounds, an old road-clearance assertion, and contiguous
tree numbering). No assertion was removed or weakened. These are in addition to the already
reported legacy A/B failure; full suites and native GPU acceptance were not run.

The woodland overview is `artifacts/greyhaven-brook/woodland-edges.png`; Chromium completed
the village/bridge crossing with all seven ground layers loaded and no page errors.

## Stone well and winding frame

The simple well prop is replaced in the existing fortification glTF by a stone-lined shaft,
oak winding frame, crank, hanging rope and slate canopy at the same dry (11, 30) m location.
Its old square box is replaced by a closed sixteen-sided static mesh with the same stable
ID 107; roof 65069 blocks the larger overhead canopy. The shaft remains a closed gameplay
prop. The source exports 73 simplified meshes and the same thirteen resource files; both
clients use the existing bundle and procedural fallback paths.

Focused checks: deterministic bounded content reconstruction, native loading of every kit
entry and the browser/WASM production build passed. Chromium entered over the bridge,
approached both sides of the well and walked around it: contact stopped 1.28–1.29 m from
the centre, including the character radius. No page errors occurred; the loaded art is
captured in `artifacts/greyhaven-brook/well-detail.png`. The procedural fallback also matches
the new body and canopy dimensions.
Full suites and native GPU acceptance were not run. Previously reported content assertions
and legacy A/B failures remain unresolved.

## Broad northern summits and saddles

The enclosing northern escarpment now varies at a broader landscape scale, with a 26–56 m
summit/saddle profile and a toe meandering up to sixteen metres from its former line.
Fissures follow the ribs instead of varying across a two-dimensional grid of small crest
dimples. The extended southern toe meets the existing Vale hills without cutting a step.
The Highland Pass and marked climbing strip retain their original terrain profiles and
full-cell shoulders, blended into the new enclosing rock. The plateau, stronghold and
lookout remain in their existing places; no world area or gameplay system is added.

This remains the same fixed-integer authoritative heightfield, shared by rendering and
Rapier. Its content revision is derived from the changed samples; recovery and visual LOD
paths are unchanged. The live Blender terrain and 1,589 nature instances are refreshed from
the same game export, with the packed game rock maps retained for authoring context.

Focused checks passed: the two Highland gameplay tests (stronghold journey and return,
and a player stopped by the escarpment), format and the browser/WASM production build.
Chromium crossed the village bridge without page errors, loaded all seven ground textures
and captured `artifacts/greyhaven-brook/northern-saddles.png`. The first capture attempt
left no artifact and was confirmed stopped before the successful retry. Full suites and
native GPU acceptance were not run; the previously reported failures remain unresolved.
The wide scene still needs further cliff, ground and lighting refinement to match the
owner's reference. Convention source revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

## Authored basin buttresses and talus

Three rock clusters interrupt bare cliff feet northwest of the keep, beside the western
basin edge and before the southern cliff. Each combines a leaning main slab with two lower
talus pieces, authored in `fortifications.py` as closed, simplified indexed meshes with
stable IDs 65094–65102. Their roots are buried into the existing ground and the upper slabs
lean back into the enclosing cliffs. The placement leaves gates, the Hollow approach,
switchbacks, the South Road and the watercourse clear.

The collision JSON now contains 82 meshes. Rapier loads the new pieces through the existing
immutable static-mesh path; shared presentation draws these exact facets with the existing
rock PBR material and the existing strata/moss landscape palette in both clients. They stay
independent of the settlement glTF and terrain LOD, and remain visible through the shared rock presentation without a new asset dependency.
The settlement glTF/resources are unchanged. The live Blender scene includes the same nine
new pieces with the game rock maps for context. No new format, renderer, cache or spatial
index is introduced; only content identity changes.

Focused checks passed: the existing brook/bridge/ford/gates/culvert/recovery gameplay check,
a new character walk that hits the northwest slab and then returns to the open Hollow
approach, the South Climb traversal, format and the browser/WASM production build. Full suites/native GPU acceptance
were not run; the previously reported content assertions and legacy A/B failures remain.
Chromium walked out through the north gate to the new rocks, loaded all seven ground
textures and captured `artifacts/greyhaven-brook/basin-buttress.png` without page errors.
The overall reference direction remains in progress, particularly ground variation and
lighting. Convention source revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

## Walkable folds in the northern apron

Three gentle terrain mounds interrupt the empty floor north of the settlement. The main
rise is about two metres high on the four-metre lattice, with two lower folds nearby.
Their authored footprints stay clear of the keep/palisade foundations, spawn plaza,
Hollow Road, switchbacks and lake. The settled Outpost floor remains level.

These are fixed-integer core terrain samples, used by Rapier and both clients through the
existing content path. They are not a visual displacement that the character could walk
through. No collider type, wire layout, visual LOD or streaming mechanism changes.
The live Blender terrain and nature context are refreshed from the game export.

Focused checks passed: the slab/Hollow approach gameplay test now also walks onto the
knoll's crown and returns to the level Hollow Road; format and the browser/WASM production
build pass. Chromium walked through the north gate to the rocks and captured the ground
folds in `artifacts/greyhaven-brook/northern-ground.png`, with all seven ground textures
loaded and no page errors. Full suites/native GPU acceptance were not run; the previously
reported content assertions and legacy A/B failures remain. The broader art direction is
still in progress. Convention source revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

## Daylight sky fill and relief contrast

Native and browser lighting now reduce diffuse sky fill smoothly from 0.9 at the horizon
to 0.65 at a fifteen-degree sun and above. Direct sun, exposure, fog, shadow reach and the
zone clock remain unchanged. Night fill stays at 0.9; full cave adaptation retains 0.1.
The browser's character-selection stage keeps its previous studio fill, and its no-sky
world fallback uses the same daylight adjustment as the atmosphere path.

This is a client lighting composition change through the existing render-engine APIs;
there are no new shaders, renderer features or authoritative content changes. The aim is
to retain shadow and surface contrast across the enclosing rock, foliage and settlement
rather than washing their dark faces with almost the same sky fill at every hour.

Focused checks passed: the native sun/moon clock check, format and the browser/WASM
production build. Full suites and native GPU acceptance were not run. Previously reported
content assertions and legacy A/B failures remain unresolved. The overall reference art
direction remains in progress. Convention source revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.
The afternoon in-game view is `artifacts/greyhaven-brook/daylight-contrast.png`. Chromium
completed the village/bridge walk with all seven ground layers loaded and no page errors.
The reference still calls for further asset and ground-cover refinement; this lighting
adjustment is incremental, not visual acceptance of the whole scene.

## Authored fern ground cover

The existing Blender source now includes a reusable fern model with seven curved fronds,
folded paired leaflets and three muted, rough leaf materials. Twenty plants form four
explicit patches on level ground near the northern rocks and grassy rises. Roots begin
below the grass surface, and the patches leave the roads and building approaches clear.
This is passable foliage; no solid collider is added.

The plants are ordinary opaque, two-sided geometry in the existing settlement glTF. Both
clients use the same current import/material path, without leaf-card alpha textures, new
shaders, a vegetation engine or another cache. The bundle still has thirteen resource files;
its mesh buffer and manifest change, while collision JSON and authoritative content revision
`17879384104603456020` remain unchanged. The plants are static in this slice; wind animation
and plant-specific LOD/streaming are not introduced.

Focused checks passed: native loading of every placed kit entry, the browser/WASM production
build and export inspection of the three leaf materials' two-sided/rough settings. Full
suites and native GPU acceptance were not run. Previously reported content assertions and
legacy A/B failures remain unresolved; the reference art direction remains in progress.
Convention source revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.
Chromium walked through the fern patch and onward to the rocks, with all seven ground
textures loaded and no page errors. The final placement is captured in
`artifacts/greyhaven-brook/fern-ground-cover.png`; the same patch is visible in the saved
live Blender scene.

## Mixed woodland at the northern cliff feet

Four appended fixed-seed groves fill parts of the sparse northern backdrop and the ground
behind Redbrand with the existing mixed-species nature kit. Unlike the raised rim groves,
these may root on the low valley floor. Existing support, road, structure and water checks
remain; overlapping old trees are retained, and new trunks leave at least 1.5 m of separation
along one ground axis. The earlier tree order is preserved. No new renderer or spatial index
is introduced.

The pass adds 236 collidable trunks, bringing the zone to 2,202. Export comparison confirmed
all 1,966 earlier trunk IDs, positions and sizes are unchanged. The existing deterministic
content test's expected authored-tree range moves from at most 2,000 to 2,500 for this
requested woodland increase; the runtime content cap is unchanged, and no new performance
gate is added. Core still owns every new trunk. Content identity changes through the
existing immutable hash; no protocol/canonical layout changes occur.

Focused checks passed: deterministic bounded content reconstruction, the walk from Hollow
into the north cave entrance, format and the browser/WASM production build. Chromium
crossed the village bridge and captured `artifacts/greyhaven-brook/northern-woodland.png`
with all seven ground textures loaded and no page errors. The live Blender nature context
now contains 1,827 exact current bundle instances. Full suites/native GPU acceptance were
not run; previously reported content assertions and legacy A/B failures remain. The wider
reference direction remains in progress. Convention source revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

## Weathered timber bridge

The Blender settlement source now dresses the existing brook crossing with individual
bevelled oak planks, iron pegs, collared posts, fitted handrails, diagonal braces and timber
stringers. Grain follows the long axis of the timbers. The planked approaches match the two existing
ramp wedges, with low bedded stone sills at their bank ends. The deck stays at the same
physical height with the same clear crossing.
No authoritative collider, terrain sample or content revision changes in this pass.

The model is in the existing settlement glTF and reuses its wood and iron materials; the
bundle still has thirteen resources. Shared presentation tags the old complete procedural
bridge as an Outpost fallback. It remains drawn while the art is unavailable, and hides only
when the existing settlement readiness flag is true. Water, banks and culverts stay outside
that fallback group. Both clients use the established kit readiness path; no new asset loader,
cache or renderer feature is introduced. Older bridge preview objects are hidden in the live
Blender file so the authored model is shown without overlapping geometry.

Focused checks passed: native loading of every placed kit entry, a shared presentation check
that switches between the procedural crossing and loaded art while retaining culverts,
format and the browser/WASM production build. Full suites and native GPU acceptance were
not run. Previously reported content assertions and legacy A/B failures remain unresolved;
the overall reference art direction remains in progress. Convention source revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.
Chromium walked across and back onto the loaded bridge, with all seven ground textures
loaded and no page errors. The final planked approaches and rail bracing are captured in
`artifacts/greyhaven-brook/timber-bridge.png`; the live Blender file contains the same model.


## Cliff projection and distant surface detail

A coarse vertical face could select a top projection from its smoothed terrain normals.
That collapsed texture variation along its height. Coarse rock now chooses its projection
from the actual triangle direction while retaining the smoothed normals for lighting.
A focused regression reproduces the collapsed scale before the fix and confirms vertical
metre scale afterward. The rock repeat is also restored to the bundle descriptor's authored
2.7 m; the live Blender terrain/buttress preview uses that scale too.

The visual comparison also showed that full normal-map detail produced a strongly repeated
bump pattern on the distant enclosing walls. Coarse rock therefore uses its existing far
material flag to retain colour and roughness textures without the near normal/occlusion maps.
Near terrain, cave surfaces and other fixed rock retain both maps. Native and browser
bind the same material distinction through existing renderer APIs; the browser keeps UVs on
its separate coarse-rock batch. No shader, renderer feature or new texture asset is added.

Triangle positions, terrain tile size, sampling, residency, chunk coverage and skirts remain
unchanged. This is surface projection and detail selection, separate from authoritative
collision and collision streaming. Authoritative content revision remains
`14998568572822554775`; presentation identity follows the changed UV/material fields.
Protocol/canonical layouts do not change.

Focused checks passed: the projection regression, all four terrain-tile checks, all five
browser scene-building checks including uploaded coarse-rock UVs, native ground texture
imports/mip chains, TypeScript checking, format and the browser/WASM production build.
Full suites/native GPU acceptance were not run; the previously reported content assertions
and legacy A/B failures remain. Convention source revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.
The final overview is `artifacts/greyhaven-brook/cliff-projection.png`. Chromium completed
the village crossing without page errors and reported both near `rock` and coarse
`rock-far` batches with all seven texture layers loaded. The large repeated bumps are
reduced; some texture repetition remains in the very wide view, so this is not visual
acceptance of the complete landscape.

### Village brook bends and shingle

The immutable `brook::COURSE` now includes smaller alternating bends between the
bridge and the south drainage opening, and along the downstream reach. The bridge
crossing, culvert point, waterfall connection, water level and channel width retain
those authored positions/values. The same points drive the authoritative bed cut,
water volume and shared visual ribbon; neither client invents a separate route.
Two-metre ribbon subdivisions give each edge small world-space variations inside
the authoritative width. Adjacent segments share their endpoint positions, and the
wet mud overlay follows those edges.

Shared presentation adds scattered low shingle clusters beside the village brook.
The existing blob shape and rock material are reused in one combined draw. Pebbles
are partly buried in the sampled terrain, skip steep supports and the bridge
landings, and are decorative rather than movement blockers. Their tops are roughly
12 cm above the ground, with the mud overlay covering their bases. These are small
bank details; the water surface and its junction with the coarse terrain still need
further refinement. This pass does not add hydrology, collision streaming or a water
renderer.

The current content revision is `17140007599132759617` (automatically hashed);
protocol/canonical layouts are unchanged. The live Blender context contains the
updated terrain, exact shared brook ribbon and 11,760-triangle shingle mesh.
Focused checks: both `village_brook` gameplay tests and the shared terrain/water
clearance test passed, along with the browser/WASM production build. Convention
source revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.
The final close view is `artifacts/greyhaven-brook/brook-banks.png`: Chromium crossed
the bridge with all seven ground layers loaded and no page errors. The bend and
uneven edge are visible, but a hard dark strip beneath the water still makes the
water/ground junction look artificial. Resolving that junction remains necessary;
this small bank-detail pass is not acceptance of the complete stream or landscape.

### Shallow village water and modelled reeds

The broad dark strip under the brook was confirmed to be its shadow: temporarily
excluding blended materials from the provider's shadow pass removed it in the same
playable view. Toggling a three.js mesh's `castShadow` flag was inconclusive because
the provider's custom pass collects materials directly. Both diagnostic dependency
edits were restored; no provider/shader or dependency-pin changes are shipped.
TODO: blended water still casts an opaque shadow through the existing material path;
per-object shadow controls need to be respected by the rendering provider.

Greyhaven's narrow village brook now uses an explicit `BED_DEPTH` of 35 cm rather
than the previous 65 cm cut. This reduces the exaggerated separation between the
water surface and its shallow banks while retaining an authoritative terrain bed.
A 25 cm trial failed the existing clearance assertion beside the protected spawn
plaza and was discarded; 35 cm passes. The upper river cut, lake depth, water levels,
channel width, bridge and culverts retain their existing definitions.

The six existing reed patches now have nine curved, folded leaves each, with brown
seed heads on the stems. The opaque leaf faces show from both sides and use the
existing shared procedural geometry/material batching. Stems sample their individual
root positions on the real terrain. The live Blender context includes these exact
game meshes, the revised bed and the repositioned shingle.

Current content revision: `5606192899733454897`; protocol/canonical layouts are
unchanged. Both `village_brook` gameplay tests passed, including the bridge, ford,
gates, culvert and recovery. The shared water/terrain-clearance test and production
browser/WASM build passed. Chromium completed the crossing with all seven terrain
layers loaded and no page errors. The final view is
`artifacts/greyhaven-brook/brook-shallows.png`; the broad dark strip is reduced, but
the renderer's opaque water-shadow behaviour remains. Full suites and native GPU
acceptance were not run. Convention source revision remains
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

### Cottage and brook ground cover

The settlement Blender source now contains a reusable `grass_clump` model: nine
curved blades with folded centres, muted green and dry-tip material variants, and
opaque two-sided leaf surfaces. It adds 106 clumps in nine small authored patches
beside the cottages and brook. The explicit game-space anchors were sampled from
the existing WASM `surfaceY` export at content revision `5606192899733454897`, with
roots 3.5 cm below that surface. Existing road and structure footprints were given
30/35 cm clearance margins when choosing the anchors. If these local ground heights
or buildings change, revisit the anchors in `GROUND_COVER` in the Blender source.

These are ordinary passable foliage in the existing settlement glTF, not a new
scatter/cache/vegetation framework. The frozen Outpost grass package and its
provenance remain unchanged. The export has 20 material primitives and the same 13
resources (the existing textures plus the geometry buffer); no new texture package
or collision geometry is introduced. The authoritative content revision remains
`5606192899733454897` and protocol/canonical layouts are unchanged. The current live
Blender world contains the new clumps.

The native live-bundle import check and browser/WASM production build passed.
Full suites and native GPU acceptance were not run. Convention source revision:
`625cb04ca568aebc8d45c90033e107a05e9bd2a0`.
The requested fresh in-game close-up is
`artifacts/greyhaven-brook/latest-brook-ground-cover.png`, captured after the bridge
crossing with all seven terrain layers and no page errors. The clumps' spacing is
still fairly regular; loosening those groupings remains part of the art pass.

## Revised landscape target (owner brief, 2026-11-06)

The concept is the geographic and visual target; the latest wide game capture is the
baseline. Prioritize large shapes before decorative detail:

1. Replace the uniform crater walls and repetitive rocks with irregular cliffs,
   slopes, scree, gullies and traversable terraces.
2. Make the western valley, slopes and northern wilderness substantially more wooded,
   with varied species/heights and natural clusters.
3. Put Stillwater against the western cliffs in forest, connect it to the existing
   waterfall/brook, and remove the river that circles the valley.
4. Replace Redbrand's large rear rock mass with a small cave hideout, palisade,
   campfire and a few shelters; retain its approved far-side entrance orientation.
5. Add northeastern abandoned mining ruins, a ruined watchtower, collapsed buildings,
   mine entrance, carts and a bridge across a ravine, connected by natural paths.
6. Use muted ground and stone, darker forests, directional light/shadow and atmospheric
   depth. Preserve existing gameplay, structures, collision and terrain ownership.

Keep new geographic data in committed JSON where practical. Compare matching wide
Blender/game views and refine the largest differences. Use focused geographic/gameplay
checks and the existing performance baseline/profiler; do not add a new performance gate.
This larger target remains in progress; the earlier prop passes are not completion.

### Western Stillwater checkpoint

Stillwater's centre is now game `(-87,-53)` metres. The existing shrine and shoreline
geometry are placed by one core-owned half-turn with offsets `(-32,-108)` metres.
Those offsets preserve the 4 m terrain lattice; an initial off-grid placement was
rejected after it obstructed the shrine swim. Authored collision JSON and stable IDs
are retained. The same placement moves the shrine kit, water/air pocket, quest area,
shore rocks and lake creatures. Hollowpine Reach's eastern name boundary is moved
west of the relocated altar so the named shrine remains disjoint. Three northern
Wolfrun wolf posts move south within the woods; no creatures or quests are added.

`content/greyhaven/landscape.json` now stores the village/inflow/outlet polylines and
the Lakeshore/Wolfrun Ridge paths. `landscape.rs` parses this committed data once;
terrain cuts, authoritative water and shared presentation read those same points.
The outlet stays west of Greyhaven rather than encircling the valley. Existing terrain
lanes remain authoritative under the lake's rounded shore apron. The dock is on the
northern shore; its existing plank ramp now follows the dock position and quarter turn
rather than retaining a fixed eastern-shore coordinate. The lake placement metadata
still uses the existing Rust constants; further landscape authoring consolidation is
pending. No new renderer, spatial index or collision-streaming system is introduced.

All 13 focused core checks passed (`sunken_shrine`, `water`, `village_brook`), including
swimming to the relic, the air pocket, roof walk, wading/diving/drowning, bridge/ford/
gate/culvert/recovery and a full walk from Greyhaven through Wolfrun to the dock.
The browser/WASM production build passed. Current content revision is
`5643289760283480598`; protocol/canonical layouts are unchanged. Convention source
revision remains `625cb04ca568aebc8d45c90033e107a05e9bd2a0`. Full suites, native GPU
acceptance and the revised landscape performance comparison have not run.

### First-version ridge and basin pass

`landscape.json` now includes `basin.lobes` (centres, ellipse radii and shoulder widths
in Q10) and `northernRidge.sections`. Each northern section has an X coordinate and
eight `[Z, height]` points, in centimetres, ordered south to north. Adjacent sections
interpolate on the existing 4 m authoritative heightfield. The profiles form uneven
foothills, rock faces, benches, crowns and lower saddles instead of a continuous
narrow wall. Seeded, directional gullies weather those profiles. The basin's broader
shoulders retain the village floor and fade back to the original waterfall attachment.

The Highland Pass lanes keep their heights. Their shoulders extend the ledges' own
heights, rather than preserving the old 40 m wall around every lane. The approach
stays clear and the marked hard climbing face retains its established profile.
Tree support checks cover the full expanded ridge face. No extra terrain lattice,
collision mesh system, renderer, LOD system or performance gate is added.

Focused checks passed: all three Highland tests (the stronghold round trip, barrier
beside the pass, and a new walk across the western rock terrace), the three village/
brook/lake-route checks, format and the browser/WASM build. The final shared content
revision is `2342607497514274074`; protocol/canonical layouts are unchanged. Full
suites, native GPU acceptance and a frame-time comparison against the performance
baseline have not run. The same-camera in-game overview is
`artifacts/greyhaven-brook/first-landscape-version.png`; the live Blender scene is
saved in `artifacts/greyhaven-brook/connected-landscape.blend` (local authoring state).
Convention source revision: `625cb04ca568aebc8d45c90033e107a05e9bd2a0`.

The review image is checked in as [the first-version overview](images/greyhaven-first-landscape.png).

The owner requested a first GitHub version and a pause after pushing it. The denser
forest pass, smaller Redbrand hideout, northeastern mining ruins/ravine bridge and
remaining material/atmosphere work are deferred for review. This checkpoint is not
completion or visual acceptance of the full revised concept brief.
