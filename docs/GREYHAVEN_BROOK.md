# Greyhaven's Waterworn Gorge

The village occupies a rounded, asymmetric basin with an adjoining northern lobe. Fixed-seed integer weathering varies the cliff outline and shoulders. The heightfield is still authoritative: scenery samples the same terrain, and the six existing switchback lanes override the weathering. This is separate from visual terrain tile LOD and the authored Redbrand Cave meshes.

`mmorpg-core/src/content/greyhaven_vale/brook.rs` authors the small west-cliff spring, a narrow waterfall-fed brook through the village, a timber footbridge, and a low drainage opening through the southern palisade. Its centimetre centreline supplies the shallow terrain bed and a `WaterChannel` footprint. Existing wading/swimming rules apply; foam motion is cosmetic and exerts no current. The spring is an ordinary elliptical water volume. Neither water shape introduces another movement solver.

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
