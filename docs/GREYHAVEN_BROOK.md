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
