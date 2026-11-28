# Greyhaven client presentation workloads

Issue #645 extends the zone's operation scaling contracts ([interest](INTEREST_WORKLOADS.md#operation-scaling-contracts-138), [combat](COMBAT_WORKLOADS.md)) to the clients' presentation. It counts work at the boundaries both clients already drive, without a GPU, a browser or a clock: the static scenery's residency streams (`mmorpg_presentation::tiles::Streamer` over the terrain tiles and the static groups, as `mmorpg-wasm`'s `TerrainStream` and `GroupStream` run them for the browser and the native client runs them for its terrain) and the shared `units::UnitPresenter`. Every scenario draws the complete scene with every kit drawn; nothing is hidden to make a count smaller.

```sh
cargo test -p mmorpg-presentation --test presentation_work --locked                      # blocking sentinel
cargo run --release -p mmorpg-presentation --example presentation_work --locked -- --report  # advisory counts
coding-tooling performance work --root .                                                  # declared bounds (performance:work)
```

## Work dimensions

| Dimension | Greyhaven value | Where it is counted |
| --- | --- | --- |
| Authored static scenery N | static groups, their draws and the terrain tiles: the `scene` line of `presentation_work --report` (above) | `StaticScene` |
| Visible/loaded tiles V | near tiles resident around the camera | `tiles::Streamer::resident_count` |
| Animated characters A | presented units | `UnitFrame::units` |
| Changed presentation entities Δ | units whose projection moved this frame | the scenario's moving wolves |
| Draws per frame | static draw records selected (shown groups, near tiles, far cover) and unit draws, before renderer batching | `Statics::frame`, `UnitFrame::draws` |
| Uploaded geometry and bytes | near tiles and groups built, at the browser's packed layout (64 bytes a vertex, 4 an index); unit geometries first drawn (new resource keys) | build callbacks, `Draw::resource_key` |

## Scenarios

`tests/support/presentation_work.rs` runs them; the example prints one `mmorpg.presentation-work/v1` line each. Camera scenarios settle residency at the hub first (and run one orbit lap of 64 frames); unit scenarios run one warm-up frame and then 64 frames.

| Scenario | Builds (groups + tiles) | Evictions | Uploaded bytes | Largest frame draws | Notes |
| --- | ---: | ---: | ---: | ---: | --- |
| stationary frame | 0 | 0 | 0 | 15,724 | 16 near tiles resident |
| camera-only orbit (6 m) | 0 | 0 | 0 | 15,765 | 17 near tiles resident |
| region walk, 512 m east | 233 + 20 | 594 | 7,282,720 | 15,814 | at most 2 builds a frame (budget 10) |

| Scenario | Units | Unit draws | Distinct geometries | Geometry uploads |
| --- | ---: | ---: | ---: | ---: |
| one moving character | 26 | 444 | 180 | 136 |
| dense crowd, 64 wolves (8 moving) | 89 | 1,389 | 180 | 136 |

Units include the viewer and the NPCs and creatures its projection already shows. Unit draws are rebuilt every frame and grow linearly with A whatever Δ is; geometry is shared per shape, so uploads do not grow with A or Δ. The uploads after warm-up come from breathing animal torsos, whose box size is part of their geometry key (one new mesh per breathing family and frame); this is recorded, not yet fixed.

## Contracts

`.performance/contract.json` declares the presentation operations beside the zone's; `scripts/operation-work.py` merges the `presentation_work`, `projection_ingest`, `unit_layout` and `seeded_content` examples' samples and the browser operations' samples (below) into the same evidence (`unit-frame` combines `presentation_work`'s and `unit_layout`'s metrics by equal dimensions):

| Operation | Dimensions | Bound |
| --- | --- | --- |
| `camera-only-frames` | measured frames, orbit radius | no static builds, evictions or uploaded bytes; largest frame draws constant in frames |
| `region-walk` | metres walked | at most `BUILDS_PER_FRAME + GROUP_BUILDS_PER_FRAME` (10) builds a frame |
| `unit-frame` | animated characters | unit draws linear; geometry uploads constant; browser layout entity lookups and bytes linear; warm-frame layout allocations constant (at most 8) |
| `client-interpolate` | projected units (viewer included) | entity records visited by one frame's interpolation linear; warm-frame allocations none |
| `projection-ingest` | projections ingested (local and online) | at most 1 decode a projection; no unchanged sheet views; no bytes beyond the network payload; sheet views constant |
| `world-frame` | warm frames | at most 8 presenter stage calls a frame; no panel redraws or render-root list rebuilds while nothing changed; one action-bar decode per projection |
| `scenery-frame` | warm frames | no instance-batch view built: each resident instance batch keeps one shown and one hidden renderer batch |
| `ambient-clips` | clips placed, visible near, visible middle | clips sampled in a warm frame constant in placed clips and linear in visible near ones (at most 8 at 4 near and 12 middle) |
| `scene-matrix-frame` | animals placed, animals posed a frame | one walk of the scene's world matrices a frame; animal subtrees recomputed and skeleton updates linear in posed animals, constant in placed ones; one skeleton per animal |
| `projection-rules` | projections ingested | no client rule deserialises an ingested projection's page view |
| `sun-shadow-casters` | frames of a camera walk | at most 2 sun cascades redrawn a frame; a unit's shadow proxy at most 33 % of its triangles near and 12 % further out; the placed kits keep off-screen shadow casters (#1038) for at most 20 % more placements |
| `seeded-content-entry` | content seeds a fresh page applies | no seeded base part (terrain heights, colliders, solid dressing) built again on the way to entry |
| `hud-texts` | frames showing each projection | at most 1 `hudTexts` call into WASM a projection, shared by the combat and class HUDs, constant in frames (#1278) |

The camera-only, projection-ingest and browser unit layout bounds are blocking, through the `presentation_work` and `mmorpg-wasm` `projection_ingest` and `unit_layout` tests that `cargo test` runs: an incremental-update regression that rebuilds, re-uploads or re-evicts static scenery on a camera-only frame (or never settles) fails the first, a second decode, a re-sent unchanged sheet or an extra byte copy fails the second, and a lookup per pair of units, faster-growing layout bytes or a per-character allocation fails the third, and a find per interpolated unit or an allocating warm interpolation fails `unit_layout`'s interpolation test; a seeded page that builds a terrain, collider or dressing part again fails `mmorpg-wasm`'s `seeded_content` test. Legitimate streaming, LOD and visibility changes stay allowed and the other counts are advisory. The `world-frame` bounds are blocking through `web/tests/frame-stages.test.ts` (`bun test`), which runs the staged frame's stage runner, panel gate, root registry and action-bar cache, the `scenery-frame` bound through `web/tests/scene-building.test.ts` over the real static scene, the `ambient-clips` bound through `web/tests/clip-detail.test.ts`, the `scene-matrix-frame` bounds through `web/tests/held-matrices.test.ts`, the `projection-rules` bound through `web/tests/projection-rules.test.ts` the `sun-shadow-casters` bounds through `web/tests/shadow-lod.test.ts` and the `hud-texts` bound through `web/tests/hud-texts.test.ts`. Frame pacing on real hardware stays with the existing wall-clock profilers, `scripts/profile-browser.py` (#546) and the native `outpost_frame_time` (#692); they are compared separately and never gate correctness.

### Browser evidence (#1120)

Every declared operation emits evidence: `coding-tooling performance work` reports the whole contract unavailable when one operation, scale point or metric has no sample. A browser operation's measurement lives once, in a helper under `web/tests/support/work/` that runs the real browser code and returns counters. Its bun test asserts the bounds through that helper, and `web/scripts/work-evidence.ts` calls the same helper at every scale point the contract declares and writes the samples, which `scripts/operation-work.py` merges with the Rust examples'. A new browser operation adds its helper, calls it from its test and adds its id and metrics to `MEASUREMENTS` in `web/scripts/work-evidence.ts`. The entry point builds the release `mmorpg-wasm` module first (as the test preload does), so it needs `bun install` in `web/` and the fetched asset bundles.

The whole check, `coding-tooling performance work --root .`, runs under full validation only (the `work-contract` job of `.github/workflows/validate.yml`, through `scripts/ci-work-contract.sh`); it fails on an invalid contract, missing evidence or a crossed bound. Ordinary PR and merge-queue runs leave it out because of its release builds; the bun tests and Rust tests keep asserting the bounds there.

## Projection ingest (#1009)

Each session owns one `mmorpg-wasm` `ProjectionStore`. A projection is decoded once, into the store: the local host's is encoded and decoded inside WASM, so its bytes never reach the page, and an online one crosses once, as the network payload. The page gets a view whose bag (inventory, equipment, stats) and quest sheets are null unless they changed since the previous view; `replication.ts` keeps the earlier copy. The unit presenter and its sound cues read the decoded projection from the store by id; it retains two seconds of projections.

```sh
cargo test -p mmorpg-wasm --test projection_ingest --locked                 # blocking sentinel
cargo run --release -p mmorpg-wasm --example projection_ingest --locked      # samples for the contract
```

| Ticks per path | Decodes per projection | Unchanged sheet views | Extra boundary bytes | Sheet views |
| ---: | ---: | ---: | ---: | ---: |
| 16, 32, 64 | 1 | 0 | 0 | 4 |

Before #1009 each projection was decoded twice (into a complete page object, then again by the presenter), both sheets were materialised for the page every tick, and its bytes crossed the boundary once more for the presenter (and twice for a local projection).

## Browser unit layout (#1057)

`mmorpg-wasm`'s `Units` lays each unit frame out as typed-array records in WASM memory (`crates/mmorpg-wasm/src/unit_layout.rs`): `u32` words, `f64` numbers and the draws' `f32` model matrices, which `web/src/world/unit-presenter.ts` reads through views it rebuilds only when memory grows or a buffer moves. Draw ids, resource keys, clip names and every other string cross once, when first interned; the page keeps its renderer nodes by draw id and updates them in place, without parsing, matrix copies or new node objects. Each push indexes the projection's records by entity, so sampling is one lookup a unit. Imported model requests stay fresh objects each frame, because the model kit keeps their tint and layers by reference.

```sh
cargo test -p mmorpg-wasm --test unit_layout --locked                 # blocking sentinel
cargo run --release -p mmorpg-wasm --example unit_layout --locked      # samples for the contract
```

| Characters | Entity lookups | Layout bytes | Warm-frame allocations |
| ---: | ---: | ---: | ---: |
| 8 | 33 | 48 480 | 3 |
| 16 | 41 | 60 192 | 3 |
| 32 | 57 | 83 616 | 3 |
| 64 | 89 | 130 464 | 3 |

The allocator counts the adapter's own sampling and layout, not the shared presenter's frame, whose draws still own their strings. The three allocations of a warm frame intern the breathing torsos' new geometry keys (above). Before #1057 the frame crossed as one JSON string, every sampled and imported unit searched the projection's records linearly, and the page parsed it and copied every matrix into a new node.

## Snapshot interpolation (#1058)

Both clients draw units with `mmorpg-client-model`'s `SnapshotHistory`, 2 ticks behind the newest projection (Atlas 6.1.i1058). Each push sorts the projection's record indices by entity once (into a buffer reused from a dropped projection), so a frame's sample walks the two bracketing projections side by side: it visits each of their records once, and it writes the poses into a buffer the caller keeps. The browser's `Units` samples it inside WASM from the projections it was pushed; the page only sends how far the frame lags the newest projection and, while it predicts, its own body. `unit_layout`'s `interpolation` workload measures that adapter step at 9 to 64 projected units:

| Projected units | Records visited a frame | Warm-frame allocations |
| ---: | ---: | ---: |
| 9, 17, 33, 64 | 2 × units | 0 |

Before #1058 the native client found each unit's next record with a linear search (quadratic in projected units) and the browser built a map and new unit objects every frame in TypeScript, then sent the sampled units back across the boundary.

## Browser draw calls at the fixed profile locations (#683)

#683 (world art separated from collision) was measured with `scripts/profile-browser.py` at its fixed locations. **Before** is `1593c054`, the parent of #683's first slice (#764). **After** is `b39cbca3` (main, 2026-11-24). Both builds ran with the current script on Chromium 159, headless, ANGLE Vulkan, on an RTX 3060 Ti at 1440×900 and pixel ratio 1:

```sh
python3 scripts/profile-browser.py --no-sweep --out report.json   # ANGLE Vulkan, the script's default since 2026-11-27
```

Two runs were made of each build, and the table shows their mean. For rAF p95, it shows the worse run. One "after" run hid `EXT_disjoint_timer_query_webgl2`, because the "before" page has no timer queries. The rAF interval is frame pacing. The main-thread frame is the page's `?profile` CPU time. GPU time per pass (#1106) exists only in the "after" build. The "before" build used today's rolling asset bundles, and its Ranger figure was copied from "after", because that commit's Ranger fetch no longer resolves.

| Location | WebGL draws | three.js calls | Triangles (M) | rAF p50 / p95 ms | Main-thread frame p50 / p95 ms | GPU p50 / p95 ms (after) |
|---|---|---|---|---|---|---|
| `idle` | 11.8k → 32.9k | 1297 → 627 | 8.4 → 9.0 | 16.7 / 16.8 → 16.7 / 33.4 | 10.4 / 13.8 → 21.5 / 26.6 | 17.3 / 19.5 |
| `move-rotate` | 9.7k → 32.2k | 1296 → 564 | 6.8 → 8.6 | 16.7 / 16.8 → 16.7 / 33.4 | 11.3 / 15.3 → 20.1 / 30.5 | 14.4 / 20.9 |
| `dense-woods` | 9.6k → 25.2k | 713 → 381 | 9.9 → 8.1 | 16.7 / 16.8 → 16.7 / 33.3 | 8.8 / 11.6 → 14.9 / 19.1 | 15.7 / 17.8 |
| `dense-hub` | 10.5k → 23.7k | 1132 → 443 | 10.7 → 7.4 | 16.7 / 16.8 → 16.7 / 16.8 | 10.1 / 13.3 → 14.6 / 19.3 | 14.8 / 16.7 |
| `western-lake` | 6.9k → 18.6k | 621 → 285 | 5.5 → 4.7 | 16.7 / 16.8 → 16.7 / 33.3 | 8.3 / 11.8 → 11.6 / 16.8 | 16.2 / 17.2 |
| `overview` | 9.9k → 27.7k | 900 → 529 | 7.4 → 7.7 | 16.7 / 16.8 → 16.7 / 16.8 | 9.2 / 13.3 → 14.2 / 16.9 | 14.7 / 16.4 |

**Result:** the 60 FPS median holds at every location, but "after" doesn't hold 60 FPS at all of them:
- At `idle` and `move-rotate`, at least 5 % of frames miss a vsync (rAF p95 33 ms). The main-thread frame median is above 16.7 ms at both, and GPU time is near or above it.
- `dense-woods` and `western-lake` miss in at least one run.
- `dense-hub` and `overview` hold 60 FPS.

"Before" held 16.8 ms p95 everywhere.

**Where the draws come from:** three.js-level calls about halved, mostly because the shadow passes fell from about 990 to 150 calls at `idle`. The rise in WebGL draws is all `WEBGL_multi_draw` sub-draws of the static-scenery `BatchedMesh`es (`web/src/world/static-batches.ts`), which draw every instance in every view. They went from about 6.6k to 21.9k per frame, and the count is the same at every location. The two largest batches have about 2.9k and 2.75k sub-draws, each drawn twice a frame. Over the same span, the main-thread `render` phase at `idle` went from 5.4 to 15.3 ms p50. Its top self time is three's `updateMatrixWorld`, `multiplyMatrices` and `traverse` (about 16 %). The range has 220 commits, 10 of them #683's, and this measurement doesn't attribute the change to individual commits.

**Cell culling (#1113, 2026-11-26):** `StaticBatches` (kit modules, walk-in buildings) now culls by 64 m ground cell. A drawn batch stays one per material and hides the instances of the cells outside the main view, rebuilding its draw list only in a frame a cell enters or leaves it and testing nothing while the view stands still (`CellCulledBatch`); shadow batches split per cell, so a sun cascade skips the cells outside it. Splitting the drawn batches per cell as well was tried and rejected: it cut the same draws but added 120–250 main-pass calls at `idle`, about 2 ms more issue time. `static-batch-views` in `.performance/contract.json` bounds a view's work. Measured with `scripts/profile-browser.py`'s scenarios on Vulkan ANGLE (RTX 3060 Ti, 1440×900, load average 2–3), baseline and change interleaved, two runs each (p50s):

| Location | WebGL draws / frame | FPS | Main-thread frame (ms) | GPU (ms) | main-opaque calls |
| --- | --- | --- | --- | --- | --- |
| `idle` | 29.6–30.8k → 16.9k | 27.5–27.8 → 28.7–28.8 | 23.6–24.4 → 20.4–21.3 | 16.0–17.0 → 15.5–15.7 | 442 → 442–445 |
| `dense-woods` | 22.5–22.7k → 7.2k | 35.3–36.3 → 39.0–40.0 | 17.8–18.1 → 17.7–18.2 | 18.9–20.1 → 17.1–17.6 | 179 → 151 |
| `dense-hub` | 24.1–24.3k → 9.3–9.4k | 35.8–36.3 → 39.5–39.8 | 18.9–19.0 → 18.2–18.3 | 17.6–17.9 → 16.0 | 341 → 333 |
| `western-lake` | 22.2k → 5.7k | 36.0–36.7 → 37.0–38.7 | 15.5–15.7 → 16.1–17.0 | 17.5–17.8 → 15.5–15.9 | 125 → 81 |
| `overview` | 24.2–25.3k → 7.7k | 35.2–36.7 → 36.2–37.8 | 19.4–20.4 → 20.5–21.6 | 17.5–18.7 → 17.0–17.7 | 506 → 506 |

The sub-draws were cheaper than their count suggested: removing two thirds of them saves 1–2.5 ms of GPU time and a few FPS, not a whole vsync. The frame stays bound by the main thread issuing three.js calls and three-lighting's per-frame setup between passes (about 4 ms at `idle`, render-engine's `#renderPasses` scene walk).

## Per-frame matrices and projection reads (#1257)

The browser's frame rate had fallen from about 61 to 39 fps at `idle` between 2026-11-07 and -27. A V8 profile of a readable (unminified) build attributed the main-thread cost the page itself controls:

- **Two walks of the scene's world matrices a frame.** `onceAcrossPasses` (`web/src/world/lighting.ts`) brings them up to date for all passes, and render-engine's own frame asked the scene again; the second walk is skipped (`scene-matrix-frame`).
- **Ambient animals' bones.** Every animal's skeleton (about 1,900 nodes, most hidden or holding a pose) was recomposed every frame, and each of an animal's two to eight skinned meshes carried its own copy of the same skeleton, so its bone matrices were recomputed and its bone texture uploaded once per mesh. Each animal now shares one skeleton per skin, and `HeldMatrices` (`web/src/world/held-matrices.ts`) recomputes its subtree, and its bone texture, only in a frame it moved or sampled its clip. Texture uploads at `idle` fell from about 147 to 38 a frame (`scene-matrix-frame`).
- **Projection views deserialised again.** A dozen client rules (action bar, chat log, combat text, quest views, ...) took the page's projection view and deserialised the whole projection through `serde-wasm-bindgen`, about 32,500 calls into the page a frame. `ProjectionStore` now remembers each view it hands out with its decoded projection, which the rules read: 2,700 import calls and 510 export calls a frame remain (`projection-rules`).
- Grass cells no longer recompose their identity matrices, and the sword arm fit skips its three rig lookups in frames without a sword clip.

Imported unit models were left alone: their unmerged skins bind the same joints with different inverse bind matrices, so they cannot share a skeleton. What remains of the render phase is three's per-draw issue and render-engine's whole-scene walks per pass (material assignment, caster lists, one `projectObject` walk per cascade and two for the main pass).

Measured with `scripts/profile-browser.py --no-sweep` (Chromium, ANGLE Vulkan, RTX 3060 Ti, 1440×900, pixel ratio 1), minified builds of `17682571` (before) and this change (after), interleaved, two runs each, mean of the p50s:

| Location | FPS | Main-thread frame (ms) | `render` phase (ms) | GPU (ms) |
| --- | --- | --- | --- | --- |
| `idle` | 37.2 → 41.6 | 22.6 → 20.4 | 16.2 → 15.0 | 16.8 → 17.4 |
| `move-rotate` | 48.1 → 54.9 | 17.8 → 16.3 | 11.7 → 10.8 | 12.6 → 12.9 |
| `dense-woods` | 42.8 → 44.5 | 13.4 → 13.2 | 9.1 → 8.4 | 16.3 → 16.8 |
| `dense-hub` | 52.0 → 54.5 | 13.9 → 13.3 | 9.3 → 8.9 | 13.3 → 13.8 |
| `western-lake` | 48.0 → 51.1 | 12.6 → 10.9 | 7.8 → 7.3 | 14.4 → 15.4 |
| `overview` | 49.5 → 53.8 | 15.0 → 13.2 | 10.1 → 9.6 | 13.3 → 14.0 |

`idle` still misses 60 fps: its main-thread frame is above 16.7 ms and its GPU time near it.
