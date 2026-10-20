# Destructible watch-post presentation (#306)

Both clients now consume the same staged modular timber draws through the existing shared unit presenter. The designated Redbrand watch post has intact, damaged and crumbling plank courses; destroyed pieces disappear except for the exact authored gameplay rubble. Eight pre-cut cosmetic fragments per destroyed piece spread, fall toward the local ground and fade within two seconds. When a projection reports restoration, planks settle into place over 0.6 seconds. This animation follows server completion of timed regrowth or free repair; it never predicts repair progress. Reduced motion snaps and suppresses fragments. A cold connection/interest reset snaps to the current state without replaying collapse. The deck cuts away while the viewer is below it and never reappears while destroyed.

The presenter owns no health, support graph, colliders, regrowth clock, repair policy or outcome. Omitted piece entries mean intact, per the existing projection contract. Immutable cube geometry is shared across every plank/fragment and reused through the existing WASM resource cache/native lowering. No wire format, asset importer, renderer or authoritative dependency changes.

## Focused evidence

- `cargo test -p mmorpg-presentation --locked`: shared presentation tests passed, including transitions, bounded fragments, cold restore and missing-deck/cutaway behavior.
- `bun run build`: actual WASM/TypeScript/Vite production build passed.
- Actual Chromium loaded the normal game with no page errors and framed the watch post: [browser intact](browser-intact.png).
- The actual native offscreen wgpu renderer consumed explicit authored visual-stage fixtures and changed GPU pixels at each stage: [native intact](native-intact.png), [native destroyed with rubble](native-destroyed.png). These are presentation fixtures, not evidence of executing a player demolition/repair journey. Core destruction/repair behavior remains the existing #305 implementation.

```sh
MMORPG_SMOKE_FRAME_DIR=artifacts/structures cargo test -p mmorpg-client --locked --lib replicated_structure_stages_change_native_pixels -- --ignored --nocapture
```

Convention source revision: `6cc05789dab39cee6758b288f83fb48b4ee6dbdb`.
