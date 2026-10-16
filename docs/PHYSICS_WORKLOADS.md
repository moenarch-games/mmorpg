# Actual zone physics adoption workloads

This is the semantics-preserving first adoption slice of MMORPG #46 and physics-engine #190. The zone continues to import the translational integer-state `physics_engine::World`; its units, tick order, CCD, game rules, save schema and player projection policy retain their existing contracts. This dependency update does not select the persistent-f64 solver. That migration still requires failed-step/checkpoint, character/query and applicable CCD prerequisites.

## Reproduce and compare

```sh
cargo run -p mmorpg-protocol --example physics_workload --release --locked
cargo run -p mmorpg-protocol --example physics_workload --release --locked -- --trace artifacts/physics-traces
```

The versioned `mmorpg.physics-workload/v1` example drives the real zone API for 120 completed ticks in three independent trials per workload. Construction is excluded; bootstrap is included. Layouts cover 128/512 stationary players, one/eight locally moving controllers among 512, 512 players on the vale's gravity-supported spawn plaza with a jump, a 64-player overlapping spawn control, negative-coordinate movement plus admission/removal/same-ID reuse and teleport recovery, and the actual vale's 59 creatures/nine NPCs with 16 players. These are workload fixtures, not supported capacity claims.

Every workload captures canonical state at tick 60, reconstructs a second zone and applies identical subsequent commands. Subsequent canonical state must match on every tick. The mutation workload also reconstructs after a deliberate teleport. Interest maintenance counts accumulate around each tick so reconstruction cannot erase prior measured work.

Measure `advance_tick` separately from recipient projection, projection encoding and canonical capture/encoding. Tick time includes gameplay/AI/controllers, physics and interest maintenance; it is not labeled physics-only time. Commands, exhaustive-reference validation, shadow recovery and diagnostic trace writes are outside those measured regions. Project eight recipients each tick and all recipients at the completed horizon; player-only fixtures also require the existing exhaustive projection oracle's bytes. The vale unit fixture uses the ordinary unit/recovery/protocol acceptance rather than pretending the player-only oracle covers creatures.

Optional bounded traces contain length-prefixed canonical and selected player-projection payloads at every tick. Comparing the files byte for byte across the old and new dependency establishes exact observable compatibility; the FNV-1a diagnostic checksum is convenient for inspecting repeated runs, not a replacement for the full comparison or a security hash. Trace capture uses no network and is disabled by default. No trace data, timings or derived engine cache enter production recovery or wire state.

## Evidence

Baseline consumer revision: `20d34fb` with physics-engine `1b98f84d409796b2a15b84f3fa4ed7f03a11f8bd`. Candidate engine: `0baf3411419fc250273caec24d64654cb30c28ec` (upstream PR #206). All 24 complete canonical/projection binary traces are byte-identical. Existing scenario/protocol goldens are retained.

Advisory median `advance_tick` totals for 120 ticks:

| Workload | Baseline ms | Candidate ms |
| --- | ---: | ---: |
| quiet-128 | 3.280 | 1.116 |
| quiet-512 | 26.921 | 8.662 |
| sparse-512-k1 | 27.349 | 22.956 |
| sparse-512-k8 | 27.138 | 22.900 |
| supported-512 | 56.271 | 45.759 |
| crowded-64 | 3.771 | 2.790 |
| mutation-recovery-32 | 0.704 | 0.701 |
| vale-units-16 | 14.148 | 14.913 |

Quiet and supported cases improve in this local comparison. Sparse and crowded cases remain active-path controls. The actual vale unit case is about 5.4% slower, and mutation/recovery shows little change; neither is reported as a speedup. Shared-machine timings are advisory and do not define a throughput gate. Full-zone work still includes controller and interest population scans. Sparse physics still stages/sorts its admitted bodies, so #190 remains open for further measured maintenance improvements.

Upstream `docs/translational-maintenance.md` records physics-call phase/work/capacity evidence and its independent exhaustive rebuild oracle. Upstream work fences require zero staging, bounds, queries and map reconstruction on unchanged quiet ticks, and zero body-map reconstruction on active successful ticks. Vector capacity is not process RSS or allocator overhead; the consumer does not claim a measured memory reduction from this dependency update.

Fingerprint: Rust 1.98.1, `x86_64-unknown-linux-gnu`, release profile, empty `RUSTFLAGS`, workload v1, three trials, identical inputs and horizons, committed dependency locks on each side; Linux, AMD Ryzen 7 5700X. Local workspace debug verification disables debug symbols/incremental caches to limit disposable disk use; timing comparisons use the same release configuration. Resolved conventions sourceRevision: `e6acb5310afaf15c0cba24f87108f5f4ad1bedc3`.


## Maintenance pin refresh (#61)

PR #62 advanced the consumer pin to physics-engine
`16833b766629c354a6a5925b991eeb74208a242d`. This separately adopts retained
translational staging capacity (#229), broad-phase vector capacity (#230) and
dependency-valid fixed bounds (#249), after the first maintenance adoption above.
The zone continues to use the integer-state `physics_engine::World`. Floating
physics, capsules and new query/contact features are not selected by a pin update;
#46's persistent-solver migration still awaits the engine-owned production f32
contract (physics-engine #258) and applicable migration acceptance.

The baseline consumer is `7c5cde9864c6f5640bff6ee029b00b3786058485`, with the old
`0baf3411419fc250273caec24d64654cb30c28ec` pin. Run the unchanged
`physics_workload --release --locked -- --trace DIRECTORY` command above on each
pin. All eight workloads completed 120 ticks in each of three trials on both
versions. All 24 corresponding length-prefixed canonical/player-projection binary
trace files match byte for byte, including recovery from tick 60. Every non-timing
JSON field also matches: completed horizon, population, movement, projection and
canonical bytes, maintenance inspections and diagnostic trace checksum. Existing
scenario/protocol goldens and the separate combat-workload ceilings are preserved.

Advisory local median `advance_tick` totals for the 120 completed ticks:

| Workload | Old pin ms | New pin ms |
| --- | ---: | ---: |
| quiet-128 | 1.432 | 1.049 |
| quiet-512 | 8.926 | 8.321 |
| sparse-512-k1 | 23.471 | 23.647 |
| sparse-512-k8 | 23.161 | 23.613 |
| supported-512 | 48.626 | 39.902 |
| crowded-64 | 2.885 | 2.776 |
| mutation-recovery-32 | 0.766 | 0.726 |
| vale-units-16 | 17.678 | 15.115 |

The one/eight-moving sparse controls are slightly slower in this sample; they are
retained alongside the other observations. These sequential three-trial local
runs establish no controlled wall-clock regression or general speedup claim. The
blocking evidence is completed work, exact byte compatibility, recovery and
deterministic counter budgets. Engine-owned capacity reuse does not eliminate
whole-population staging or sorting, and retained high-water scratch is not a
measured reduction in process memory. See the producer's
[translational maintenance evidence](https://github.com/moritzbrantner/physics-engine/blob/16833b766629c354a6a5925b991eeb74208a242d/docs/translational-maintenance.md)
for the precise capacity/fixed-bound contracts and their independent oracles.

The intentional lock transaction advances only physics-engine and its two
rust-kernels source identities (`7049e423ef00f5f65e7276281c0845d431ed6300`) and
adds the engine's SHA-256/checkpoint dependency family (`sha2` 0.10.9 and five
transitives). Previously locked registry package versions are retained. Existing
`sha2` 0.11.0 remains the transport dependency; it is not downgraded.

Fingerprint: Rust 1.98.1, `x86_64-unknown-linux-gnu`, release profile, empty
`RUSTFLAGS`, identical workload v1/content/commands, three trials per pin, committed
locks, Linux/AMD Ryzen 7 5700X. Native/WASM/browser acceptance uses the same Rust
authority and unchanged protocol fixtures. Resolved conventions sourceRevision:
`46d8793bb3034326561f876dcc67dbaa5aa1e432`.

Validation passed: workspace format/Clippy/tests/build, core/protocol/WASM builds,
unchanged headless bot/control-plane scenarios, native GPU readback/session resume,
148 browser unit tests, browser production build and all nine Chromium smoke
journeys. The isolated Python runner used Playwright 1.57.0's Ubuntu 24.04 Chromium
build on this Ubuntu 26.04 host (explicit platform selection because that pinned
runner does not recognize Ubuntu 26.04). No acceptance test was skipped.

## Additional engine work and recovery evidence (#70)

Consumer baseline `676fab2632c209bdaffc9ac0cdce376acb32f484` pins
`0baf3411419fc250273caec24d64654cb30c28ec`; the original candidate measurements use
`1d62f70e3588b80e51746ed7d05bb8bbd0bfbfdb`, an ancestor of the retained
`16833b766629c354a6a5925b991eeb74208a242d` pin from PR #62. Both contain the
same translational maintenance implementation; subsequent commits change other
solver paths. This PR retains the newer pin and lock from main. MMORPG still uses the
translational `World`. Upstream #252/#255/#256's parked/contact wake changes belong
to other solver paths and do not establish MMORPG sleep/wake or f64 adoption.

The existing workload now aggregates engine-owned `StepStats.work` through
`ZoneSimulation::tick_work().physics`. These latest-call diagnostics reset
on construction/recovery and at the start of a tick attempt; they never enter
canonical state or player projections. Work is accumulated immediately around
primary-zone ticks, so a reconstruction cannot erase earlier measurements. The
fixed cache metric is peak vector payload, not RSS or full-world memory.

[Raw observations and trace SHA-256 evidence](physics-adoption-work-2026-10-01.json)
cover nine trials per workload per pin, each completing 120 ticks. Both release
binaries were prebuilt. Two alternating-order blocks plus a third block were run;
the third candidate output and candidate trace capture were repeated after truncated
files, as recorded in the fingerprint. All 24 complete canonical/projection binary
traces match byte for byte. Across every trial, publication bytes, recovery checks,
interest maintenance, query counts and staged-body counts match. No scenario or
protocol golden was regenerated. Successful steps reconstruct zero body maps on
both pins.

Baseline fixed preparation is derived from each engine report's
`(body_count - dynamic_bodies) * broad_phase_queries`: that implementation prepares
every fixed body per actual query. This includes fixed NPCs, unlike counting only
content colliders. Candidate preparations and reuse are direct upstream counters.
Baseline capacity-growth/reuse counters are unavailable and are not invented.

| Workload | Fixed preparations before | After | Reused | Before tick ms | After tick ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| quiet-128 | 6 | 6 | 0 | 1.360 | 1.402 |
| quiet-512 | 6 | 6 | 0 | 7.669 | 7.358 |
| sparse-512-k1 | 1,440 | 6 | 1,434 | 27.922 | 28.608 |
| sparse-512-k8 | 1,440 | 6 | 1,434 | 27.767 | 26.912 |
| supported-512 | 68,292 | 252 | 68,040 | 54.947 | 50.700 |
| crowded-64 | 2,310 | 6 | 2,304 | 3.432 | 3.471 |
| mutation-recovery-32 | 1,440 | 12 | 1,428 | 0.869 | 0.845 |
| vale-units-16 | 111,186 | 261 | 110,925 | 18.077 | 17.723 |

Values include bootstrap and measure 120 ticks; timings are advisory medians.
Supported-512 avoids 99.63% of fixed preparations; Vale units avoid 99.77%.
Mutation/recovery prepares 12 fixed bounds because the deliberate teleport creates
a new world; the tick-60 shadow recovery is outside the measured primary world.
Quiet calls already avoid preparation after bootstrap. All-N active staging,
dynamic-bound preparation, sorting and game-owned scans remain. The candidate
retains 28,224 bytes of fixed-cache payload for supported-512 and 29,232 for Vale
units, including nine fixed NPCs. Warmed equal-capacity calls avoid staging and
broad-phase growth; the focused consumer test verifies reuse plus cold-cache recovery
with unchanged canonical continuation.

Validation: workspace format, Clippy, all-feature tests and build; core/protocol/WASM
wasm32 build; all bot and control-plane scenarios; native host/client GPU smoke;
148 Bun tests, TypeScript/Vite production build and all nine real Chromium smoke
journeys. Native smoke rendered 2,320 colors and resumed connection epoch 2.
The first release WASM build emitted an empty object file; a rebuild and subsequent
default build/tests passed without source or gate changes. Chromium 143.0.7499.4
headless shell came from the official Chrome-for-Testing mirror because the
Playwright CDN returned unavailable HTML. Native GPU uses Mesa software Vulkan.

Fingerprint: Rust 1.98.1, x86_64-unknown-linux-gnu, release/default features and empty
RUSTFLAGS, workload v1, committed locks; shared Linux AMD EPYC 9V74 workspace.
Debug checks disable debug symbols/incremental caches. Installed conventions were
retained unchanged; live central policy was inspected at `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
This completes another semantics-preserving adoption slice, not MMORPG #46's solver migration.

Integration preserves main PR #60 combat workload diagnostics and reuses its existing
`ZoneTickWork.physics` surface. All 24 traces and deterministic work counts were
rechecked after integration; the older baseline accessor was measurement-only.

After integration with PR #62, the retained `16833b7` pin passed workspace
format/Clippy/all-feature tests/build and core/protocol/WASM compilation. Three
fresh trials per workload reproduce all 24 PR #62 traces byte for byte and match
every original candidate non-timing work field. These rows are retained under
`retained_pin_verification` in the raw evidence file; original measurements
continue to identify their original engine revision.
