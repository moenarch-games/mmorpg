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
| Authored static scenery N | 4,169 static groups (51,132 draws), 96 terrain tiles | `StaticScene` |
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
| `sun-shadow-casters` | frames of a camera walk | at most 2 sun cascades redrawn a frame; a unit's shadow proxy at most 33 % of its triangles near and 12 % further out |
| `seeded-content-entry` | content seeds a fresh page applies | no seeded base part (terrain heights, colliders, solid dressing) built again on the way to entry |

The camera-only, projection-ingest and browser unit layout bounds are blocking, through the `presentation_work` and `mmorpg-wasm` `projection_ingest` and `unit_layout` tests that `cargo test` runs: an incremental-update regression that rebuilds, re-uploads or re-evicts static scenery on a camera-only frame (or never settles) fails the first, a second decode, a re-sent unchanged sheet or an extra byte copy fails the second, and a lookup per pair of units, faster-growing layout bytes or a per-character allocation fails the third, and a find per interpolated unit or an allocating warm interpolation fails `unit_layout`'s interpolation test; a seeded page that builds a terrain, collider or dressing part again fails `mmorpg-wasm`'s `seeded_content` test. Legitimate streaming, LOD and visibility changes stay allowed and the other counts are advisory. The `world-frame` bounds are blocking through `web/tests/frame-stages.test.ts` (`bun test`), which runs the staged frame's stage runner, panel gate, root registry and action-bar cache, the `scenery-frame` bound through `web/tests/scene-building.test.ts` over the real static scene, the `ambient-clips` bound through `web/tests/clip-detail.test.ts` and the `sun-shadow-casters` bounds through `web/tests/shadow-lod.test.ts`. Frame pacing on real hardware stays with the existing wall-clock profilers, `scripts/profile-browser.py` (#546) and the native `outpost_frame_time` (#692); they are compared separately and never gate correctness.

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
