# Interest projection workloads

Core uses an XZ spatial index to discover candidate units before applying the inclusive 4,500-unit (45 m) distance rule and the deterministic priority cap. The live server publishes through `ZoneSimulation::snapshot_for_player`, so it uses the index automatically. `project_for_player` returns the same snapshot with deterministic query counters for workload analysis.

Cells are one interest radius wide. Each query visits nine neighboring cells and checks exact distance. The relevant units are then ordered viewer first, its target next, then by ascending `(squared XZ distance, kind, id)`, and the first `MAX_VISIBLE_ENTITIES` (64) are kept. The protocol then packs as many of them as fit one datagram in that order ([PROTOCOL.md](PROTOCOL.md#datagram-byte-budget)). Negative coordinates use floor division; cell arithmetic and distance arithmetic cannot overflow at the supported integer coordinate limits. Height does not affect the visibility policy. This remains relevance filtering, not line of sight or stealth authorization.

The derived index stores unit references (players, living creatures, corpses and NPCs) in occupied cells and a unit-to-cell membership map. Admission, removal, despawns, respawns and released spirits update it immediately. After a successful physics step, core reads authoritative physics positions of players and living creatures and changes bucket memberships only when a unit's cell changes, including collision corrections. Creature aggro and assist searches use the same index. Recovery reconstructs it from canonical state. Index contents and work counters never enter checkpoints, replay hashes, or the wire format.

## Reproduce

```sh
cargo test -p mmorpg-core --test interest --locked
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

`.performance/contract.json` (schema 2) declares the zone's natural operations, the dimensions they scale with and deterministic growth bounds; `coding-tooling performance work --root . --json` checks them against counters that `scripts/operation-work.py` collects from the `operation_work` example of `mmorpg-protocol` (the `performance:work` capability). Every sampled projection is also compared with the exhaustive visibility oracle, so a smaller count can never come from a wrong answer. Wall-clock time plays no part.

| Operation | Dimensions | Bound |
| --- | --- | --- |
| `project-one-viewer` | zone population, local neighbourhood | exact distance tests constant in population and linear in local; visible records at most 64 |
| `publish-sparse` | recipients at a fixed density (21-player clusters three cells apart) | total exact distance tests linear in recipients |
| `publish-dense-hub` | recipients in one interest radius | the largest recipient's tests linear in recipients (total work is honestly quadratic); at most 64 visible records |
| `tick-maintenance` | stationary population, players crossing a cell boundary | no full index rebuild; bucket writes constant in population and linear in movers; position re-reads linear |

The checks hold at the declared scale points only; they are executable architectural budgets, not a proof of asymptotic complexity. Physics work stays with `physics-engine`'s own counters.
