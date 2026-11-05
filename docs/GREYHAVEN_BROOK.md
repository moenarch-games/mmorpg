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
