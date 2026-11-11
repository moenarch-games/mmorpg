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

`.performance/contract.json` declares three presentation operations beside the zone's; `scripts/operation-work.py` merges the `presentation_work` example's samples into the same evidence:

| Operation | Dimensions | Bound |
| --- | --- | --- |
| `camera-only-frames` | measured frames, orbit radius | no static builds, evictions or uploaded bytes; largest frame draws constant in frames |
| `region-walk` | metres walked | at most `BUILDS_PER_FRAME + GROUP_BUILDS_PER_FRAME` (10) builds a frame |
| `unit-frame` | animated characters | unit draws linear; geometry uploads constant |

Only the camera-only bound is blocking, through the `presentation_work` test that `cargo test` runs: an incremental-update regression that rebuilds, re-uploads or re-evicts static scenery on a camera-only frame (or never settles) fails it. Legitimate streaming, LOD and visibility changes stay allowed and the other counts are advisory. Frame pacing on real hardware stays with the existing wall-clock profilers, `scripts/profile-browser.py` (#546) and the native `outpost_frame_time` (#692); they are compared separately and never gate correctness.
