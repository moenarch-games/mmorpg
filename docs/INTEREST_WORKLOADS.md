# Interest projection workloads

Core uses an XZ spatial index to discover candidate units before applying the inclusive 4,500-unit (45 m) distance rule and the deterministic priority cap. The live server publishes through `ZoneSimulation::snapshot_for_player`, so it uses the index automatically. `project_for_player` returns the same snapshot with deterministic query counters for workload analysis.

Cells are one interest radius wide. Each query visits nine neighboring cells and checks exact distance. The relevant units are then ordered viewer first, its target next, then by ascending `(squared XZ distance, kind, id)`, and the first `MAX_VISIBLE_ENTITIES` (64) are kept. The protocol then packs as many of them as fit one datagram in that order ([PROTOCOL.md](PROTOCOL.md#datagram-byte-budget)). Negative coordinates use floor division; cell arithmetic and distance arithmetic cannot overflow at the supported integer coordinate limits. Height does not affect the visibility policy. This remains relevance filtering, not line of sight or stealth authorization.

The derived index stores unit references (players, living creatures, corpses and NPCs) in occupied cells and a unit-to-cell membership map. Admission, removal, despawns, respawns and released spirits update it immediately. After a successful physics step, core reads authoritative physics positions of players and living creatures and changes bucket memberships only when a unit's cell changes, including collision corrections. Creature aggro and assist searches use the same index. Recovery reconstructs it from canonical state. Index contents and work counters never enter checkpoints, replay hashes, or the wire format.

## Reproduce

```sh
cargo test -p mmorpg-core --test zone --locked interest::
cargo run -p mmorpg-protocol --example interest_workload --locked
```

The example emits three JSON lines under `mmorpg.interest-workload/v4` (v4 renames the maintenance counter to `units_inspected_for_maintenance` and counts `visible_records` as the records the budget packs; v3 added `relevant_records`, the in-radius count before the cap). Each workload restores 512 stationary players on content without creatures or NPCs, so it measures player visibility only, advances one real physics tick, then projects once for every player. It also encodes an independent exhaustive projection (the same radius, order and cap, without buckets) for every recipient and fails if any output byte differs. Maintenance counters measure only the tick, excluding recovery construction; query counters measure the publication sweep. No public network, clock, randomness, or GPU is involved.

The baseline is the exhaustive XZ rule: 512 × 512 = 262,144 distance tests per complete publication sweep. The comparison uses identical authoritative state and encoding. Smaller distance-test counts are preferable; snapshot sizes must remain identical for equivalent visibility.

| Workload | Layout | Indexed distance tests | Baseline distance tests | Relevant records | Published records | Total snapshot payload bytes | Largest payload bytes |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| sparse-grid-512 | 23-column grid, 28 m apart, spanning ±308 m | 10,626 | 262,144 | 4,338 | 4,338 | 119,258 | 244 |
| dense-hub-512 | 23-column grid at 64-unit spacing; everyone in range | 262,144 | 262,144 | 262,144 | 24,576 | 544,256 | 1,063 |
| vale-spawn-512 | Greyhaven Vale collision content, full 32 × 16 hub spawn plaza (1 m apart) | 262,144 | 262,144 | 262,144 | 24,576 | 544,256 | 1,063 |

Every workload performs 4,608 query bucket visits per publication sweep. The stationary sparse workload requires 512 player position inspections but zero full index rebuilds, bucket inserts, removes, or moves during the tick. The core regression asserts zero membership writes for same-cell movement and two moves for two crossing players, including negative cell coordinates. Tree lookups, candidate sorting, allocations, encoding and physics have costs not captured by distance-test counts. These measurements prove reduced discovery and maintenance work, not a supported player throughput. Dense crowds still cost quadratic candidate discovery; the cap bounds output, not discovery. Physics throughput needs separate profiling in the owning engine.

Payload bytes above count only the MMO visible snapshot (wire version 5: 55 fixed bytes for the header, self and target sections and the event and entity counts, 14 bytes per event and 21 bytes per published entity; these idle workloads have no events), excluding the shared session frame, QUIC, TLS and UDP overhead. Budget packing publishes at most 48 records without events, so the largest projection is 1,063 bytes, inside the 1,077-byte budget; wire version 4 published up to 64 16-byte records (1,058 bytes), and wire version 3 used 25-byte records and no cap (up to 12,834 bytes, which the pinned whole-datagram transport could not deliver). Dense relevance beyond the budget is omitted by deterministic priority, never truncated by transport. The canonical payload of each workload is 31,276 bytes: it references its content by revision and fingerprint instead of embedding the colliders (the version 4 vale checkpoint was 26,910 bytes with them and without unit state), and it is not sent to clients.

## Regression evidence

The core gate compares indexed and exhaustive results across inclusive radius edges, diagonal edges, vertical separation, negative cells, extreme coordinates, a fixed scattered layout, dense layouts beyond the cap, movement across cells, collision resolution, failed physics steps, admission, removal and checkpoint recovery. The protocol gate runs the shared exhaustive reference and indexed projection through deterministic multi-tick sequences and compares complete snapshots and encoded bytes after each transition; positions outside the compact wire range fail closed identically, and players running on content without walls stay encodable because the world limits stop them. It also asserts exact sparse/dense query work and membership maintenance counts without wall-clock thresholds. Stable ordering, acknowledgements, content revisions and read-only query behavior are checked alongside visibility.

Recorded measurement environment: Rust 1.98.1, `Cargo.lock` SHA-256 `36adf21ccc47ab0a19e8fd6e8792cb1fbd0605d9ac70070d656b6fdec10a0157`, core schema 5, wire version 5, and Greyhaven Vale content revision 3. The counts are architecture-independent integer operations and byte lengths.

## Operation scaling contracts (#138)

`.performance/contract.json` (schema 2) declares the zone's natural operations, the dimensions they scale with and deterministic growth bounds; `coding-tooling performance work --root . --json` checks them against counters that `scripts/operation-work.py` collects from the `operation_work` example of `mmorpg-protocol` the `creature_tick_work` example of `mmorpg-core` and the `grounded_work` and `mover_pairs_work` examples of `sim-foundation` (the `performance:work` capability). Every sampled projection is also compared with the exhaustive visibility oracle, so a smaller count can never come from a wrong answer. Wall-clock time plays no part in these bounds; they run on every PR and merge-queue run that changes Rust or browser code (#1256).

| Operation | Dimensions | Bound |
| --- | --- | --- |
| `project-one-viewer` | zone population, local neighbourhood | exact distance tests constant in population and linear in local; visible records and built entity records at most 64; one zone-view build; no sheet built for an unchanged viewer |
| `publish-sparse` | recipients at a fixed density (21-player clusters three cells apart) | total exact distance tests and built entity records linear in recipients; one zone-view build per tick for every recipient; no sheet built for unchanged viewers |
| `publish-dense-hub` | recipients in one interest radius | the largest recipient's tests linear in recipients (total work is honestly quadratic); at most 64 visible and 64 built entity records per recipient; one zone-view build per tick; no sheet built for unchanged viewers |
| `tick-maintenance` | stationary population, players crossing a cell boundary | no full index rebuild; bucket writes constant in population and linear in movers; position re-reads linear |
| `creature-tick` | creatures asleep far from the player, creatures fighting it | creature visits constant in sleepers and linear in fighters; no interest query allocates; warm-tick heap allocations constant in sleepers (#1086, from `mmorpg-core`'s `creature_tick_work` example; see [combat workloads](COMBAT_WORKLOADS.md)) |
| `grounded-reads` | units standing still, steady ticks | no support cast on a steady tick (each read answers from the move's own support test); at most one cast per unit on a tick whose static geometry changed after the moves (#1148, from `sim-foundation`'s `grounded_work` example) |
| `mover-pairs` | walking units at a fixed density, asleep units away from them | slide-around examines each walker's own and neighbouring cells' movers only: linear in walkers (at most 9 each in the samples; 512 walkers, one per cell, examine 4,324 bodies where the full scan examined 262,144), constant in sleepers (#1149, from `sim-foundation`'s `mover_pairs_work` example) |
| `deferred-reward-drain` | deferred zone payouts waiting (#1112) | entries visited and paid constant in the backlog, at most the tick's grant bound (16 in the samples); an empty backlog visits none |
| `incident-receipt-expiry` | players holding receipts, content receipts each holds (#1128) | on the tick that ends an incident, one visit per player plus one per expired receipt: linear in players, constant in the content receipts they hold; a tick that ends no incident visits none |

Publication work (#1056) is sampled the way the zone host publishes: every recipient is published to, acknowledges, and after one tick is published to again with its sheet encodings held. A projection tests candidates by position first and builds entity records only for the relevant units inside the priority cap. The doors, damaged pieces and spent veins every viewer sees are built once per tick and shared. The bag, quest, world and waystone sheets carry stamps of the core state they come from (`Revisioned` values take a fresh process-unique stamp on every mutable access), so `project_for_publish` leaves out a sheet whose stamp the publisher's `SheetEncodingCache` already holds and the encoder sends its cached bytes without encoding or hashing them again. The loot, ability, party and boss loot sheets follow per-tick combat state and are still built every tick. Each warm publication is compared byte for byte with a cold pack of the complete projection. Heap allocations per publication are not counted yet: a counting allocator needs the workspace's `unsafe_code` lint relaxed for one test binary (#1068).

The checks hold at the declared scale points only; they are executable architectural budgets, not a proof of asymptotic complexity. Physics work is counted by the zone's physics step statistics (`StepStats`).

The client presentation has its own operations in the same contract: see [presentation workloads](PRESENTATION_WORKLOADS.md) (#645).

## Runtime budgets (#1256)

Beside the deterministic bounds, `.performance/runtime.json` budgets wall-clock runtime of named scenarios (owner decision, Atlas 7.1.i1256). Its `native` section times the `runtime_scenarios` example of `mmorpg-protocol` in release: Greyhaven zone ticks with 16, 64 and 256 running players, and one publication round (every viewer projected and encoded) at 256, as p50/p95 milliseconds. A value above its absolute ceiling is an error; one worse than main's latest baseline by more than the section's `warnRatio` is a warning only. Timings never decide correctness.

```sh
python3 scripts/runtime_budget.py native --out artifacts/runtime/native.json     # measure and check the ceilings
python3 scripts/runtime_budget.py native --baseline <main's native.json>          # also warn on regressions
```

CI runs it in the `runtime-budgets` job of `.github/workflows/validate.yml` wherever the work contract runs; each push to `main` measures the baseline and stores it as a cache entry that later PR and merge-queue runs restore. The ceilings leave several times the headroom over a 2026-11-27 measurement (Ryzen 7 5700X: tick p95 0.33, 0.83 and 4.1 ms; publication p95 8.0 ms) so hosted-runner noise never fails a run; the 256-player tick stays within one 60 Hz tick.
