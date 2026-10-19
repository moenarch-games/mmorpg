# Physics backend A/B (physics-engine #330)

Rapier is the zone's physics authority (Atlas 2.1.6). This page covers a **test/bench-only**
seam that runs the same zone simulation on physics-engine's `approximate::World`, so the two
can be compared on MMORPG's real workloads. Nothing here changes production: the zone host,
`mmorpg-wasm` and every default build compile only Rapier
(`mmorpg-game-server/tests/dependency_boundary.rs` checks the manifests and lockfile).

```sh
cargo test -p mmorpg-core --features physics-ab --test physics_ab -- --nocapture   # merge-blocking in CI
cargo run --release -p mmorpg-core --example physics_ab --features physics-ab      # manual timing
```

## Seam

- `physics::World` is an enum over the backends (`crates/mmorpg-core/src/physics.rs`). Rapier's
  world moved unchanged to `physics/rapier.rs`; `physics/engine.rs` is the thin engine adapter.
  No call site changed.
- The non-default `physics-ab` feature compiles the engine arm. `physics::using_backend(backend, ..)`
  selects the backend for every `World::new` on the current thread, so whole zones, and their
  recoveries, are built on it. `World::with_backend` does the same for one world.
- With the feature, `World::spent_nanos` counts wall-clock time spent in the world's calls
  (debris excluded), and `mmorpg_core::physics_ab` exposes read-only probes that ask the
  gameplay queries themselves (line of sight, NPC sight, projectile contact, door occupancy,
  ground height, grounded units).

Units: the engine world uses `Config::length_unit = 100`, so it works in the game's
centimetres directly; its default character settings scale to the same 0.25-unit contact
offset and 5-unit snap the Rapier arm uses. Rapier keeps simulating in metres.

What the engine arm covers: capsule characters (one `move_characters` batch per tick with
per-character `CharacterSettings`, in game-ID order), fixed boxes (content, doors, world limits,
NPCs, standing props), the floor half-spaces, the terrain heightfield (`add_heightfield`),
rays (`cast_ray`/`cast_ray_first`), overlaps (`overlap_shape`) and segment checks. **Debris
(#288) is not supported by the engine arm**: it stays on Rapier in both arms
(`physics::rapier::step_debris` over the adapter's fixed bodies) and is excluded from timing.

## What the test asserts

`crates/mmorpg-core/tests/physics_ab.rs` replays the bot scenarios `two-bots-move`,
`vale-hub-walk` and `vale-wolf-hunt` (command by command, as the bot runner submits them; the
Rapier arm reproduces each scenario's expected positions exactly) and the four Greyhaven combat
workload fixtures (`tests/support/greyhaven_workload.rs`, shared with `mmorpg-protocol`) through
the real zone simulation, from the same initial state with the same inputs and tick count.

On **each** backend, every run must:

- keep every player and creature state finite;
- keep unit capsules out of content boxes, door leaves and standing props, and above the floor
  and terrain, beyond 0.01 units (both backends keep a 0.25-unit gap; `f32` resolves ~10⁻³ units
  at the zone's 10⁴-unit magnitudes). Allowed: squat creatures' authored floor overlap (capsule
  radius minus half height) and units a fixture places inside geometry at tick 1;
- report every unit that has rested (zero velocity, unmoved) for three ticks as grounded;
- replay bit-identically (canonical snapshots, unit bodies and command outcomes).

Across backends:

- Scripted workloads (the bot scenarios and `vale-idle-16`): players within **1 unit** (the
  rounding step gameplay sees; measured ≤ 0.01), identical command outcomes and door states.
- Every query decision on **identical states** (Rapier's state every 60 ticks, recovered onto
  both backends): line of sight and shots for player–creature pairs within 40 m, NPC sight,
  open and closed door-leaf occupancy, ground height and grounded must agree exactly; shot
  fractions within 10⁻⁴. Grounded is not compared for units a fixture embedded in geometry.
- Restored from its canonical snapshot halfway, the Rapier arm continues exactly (Atlas 2.1.5).
  The engine arm's departure tick is reported, not asserted (engine gap below).

Fighting fixtures and creatures are compared but not bounded; the test prints their
divergences with `--nocapture`.

## Divergences

Measured at the commit the timing table names:

| Workload | Players (max Δxz / Δy) | Creatures at the end | Engine restored halfway |
|---|---|---|---|
| two-bots-move | 0 / 0 | 0 of 74 apart | continues |
| vale-hub-walk | 0.01 / 0 | 0 apart (max 0.55) | continues |
| vale-wolf-hunt | 0 / 0 | 73 apart (max 996) | departs at once |
| vale-idle-16 | 0 / 0 | 0 apart (max 0.55) | departs at once |
| vale-fights-8 | 30.1 / 0.01 (from tick 181) | 0 apart | continues |
| vale-fights-32 | 14,239 / 0.02 (from tick 1) | 32 apart | departs at once |
| vale-crowded-fights-64 | 241 / 120 (from tick 1) | 43 apart | departs at once |

Every query decision on identical states agreed (333–11,846 probes per workload).

Intentional or explained differences:

1. **Head-on characters.** The engine arm uses physics-engine's native sidestep (within ~5° of
   head-on, step to the right) instead of the Rapier arm's `slide_around` workaround (within 60°,
   both step aside plus a 1-unit outward nudge). The physics unit tests for head-on and off-axis
   pairs pass on both. Characters do not push each other on either.
2. **Embedded units.** Rapier's controller leaves a capsule that starts inside geometry where it
   is; physics-engine depenetrates it on its next move. This moves squat creatures (rats:
   radius 25, half height 20, authored 5 units into the floor) up 5 units, lifts creatures
   spawned on hillsides by 0.5–4.6 units, and pushes the 1/7/18 players the fighting fixtures
   place inside props, boxes or hills (feet at y = 0) out of them. From there, who reaches which
   creature, and so the fights, deaths and respawns, legitimately differ.
3. **Grounded.** The engine reports walkable static support within two contact offsets after
   the last move, or snapped to; Rapier reports any static ground within offset + 5 cm. On
   identical states they agreed for every unit not embedded in geometry.
4. **Creature drift.** Creatures wander and chase on a zone RNG they share. Over 2,200 ticks
   (`vale-wolf-hunt`) the small differences above move AI decisions and the RNG stream, so most
   creatures end elsewhere; the scripted hunter's path, fights and command outcomes stay
   identical.
5. **Terrain.** The Rapier arm removes flat heightfield cells and leaves them to the floor
   half-space (parry heightfield contacts let resting bodies sink); `HeightfieldDesc` has no
   cell mask, so the engine keeps them. Ground height reads the same (0 either way). Engine
   overlap queries also see the terrain (Rapier skips box–heightfield pairs); door probes only
   look for units, so occupancy agrees.
6. **Contact gap.** Rapier units never came closer than 0.2498 units to a box; engine units came
   as close as 0.04 (`vale-wolf-hunt`) and 0.00 (`vale-crowded-fights-64`), never inside.

## physics-engine gaps found

1. **Snap state is not rebuildable from game state.** Whether a character may snap down on its
   next move is its grounded result from the previous move, held inside the engine. A world
   rebuilt from the game's bodies (canonical recovery, Atlas 2.1.5) starts every character
   ungrounded and there is no API to restore it, so a character walking downhill does not snap
   on its first tick after a restore (creature 702 in `vale-wolf-hunt`: 0.3 units). This
   contradicts the API contract's "a world rebuilt from game state answers … character moves
   exactly as the uninterrupted one". The adapter does not work around it; priming each
   character with a zero move was tried and depenetrates embedded units an extra time instead.
2. **Depenetration double-counts coincident surfaces.** A capsule inside both the floor
   half-space and the flat heightfield cells at the same height is pushed out by each, summed:
   the rats rise 10 units on their first move instead of 5, then fall back.
3. **No heightfield cell mask** in `HeightfieldDesc` (the kernel's `active_cells` is fixed to
   `None`), so holes or cells left to another collider cannot be expressed.

## Timing

See the table below, produced by the example in release on the commit it names. A sample is
one tick's whole physics path (the step, every gameplay query and body update the tick makes,
as `World::spent_nanos` counts them; debris excluded); setup is building the zone. Timing is
evidence for physics-engine #330, not a gate.

- mmorpg: a0c07d7ab8be0f44f41e6e0d0aadb8ca7e99e4d3
- physics-engine: version = "0.1.0" source = "git+https://github.com/moritzbrantner/physics-engine?rev=3a1b8413d2411cca4670965b20e3889bb2c8779a#3a1b8413d2411cca4670965b20e3889bb2c8779a"
- rapier3d: version = "0.36.1" source = "registry+https://github.com/rust-lang/crates.io-index"
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- profile: release; 2 warmup + 7 measured rounds per backend, alternating the first backend each round
- sample: one tick's whole physics path (step, gameplay queries, body updates), debris excluded; microseconds; setup is building the zone, its physics share in parentheses

| workload | ticks | moving / bodies | backend | median | p95 | p99 | mean | setup (physics) | engine / rapier, median of paired rounds |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
| two-bots-move | 10 | 76 / 1206 | rapier | 1722.7 | 3118.4 | 4705.5 | 1882.2 | 5792.5 (5568.8) |  |
| two-bots-move | 10 | 76 / 1206 | physics-engine | 525.7 | 1701.6 | 2012.4 | 651.6 | 13839.3 (13668.7) | 0.353 |
| vale-hub-walk | 280 | 76 / 1206 | rapier | 1742.6 | 2646.5 | 4828.5 | 1866.7 | 4885.8 (4685.1) |  |
| vale-hub-walk | 280 | 76 / 1206 | physics-engine | 489.8 | 752.6 | 1047.7 | 529.6 | 11950.0 (11794.4) | 0.280 |
| vale-wolf-hunt | 2200 | 75 / 1205 | rapier | 1528.4 | 2420.4 | 2934.3 | 1588.0 | 4461.6 (4307.2) |  |
| vale-wolf-hunt | 2200 | 75 / 1205 | physics-engine | 378.6 | 596.2 | 807.3 | 381.5 | 10831.0 (10667.5) | 0.241 |
| vale-idle-16 | 360 | 90 / 1220 | rapier | 1077.7 | 1430.7 | 1747.3 | 1092.0 | 6455.2 (2965.3) |  |
| vale-idle-16 | 360 | 90 / 1220 | physics-engine | 322.8 | 439.5 | 604.6 | 337.4 | 15895.6 (7857.7) | 0.309 |
| vale-fights-8 | 360 | 82 / 1212 | rapier | 1020.5 | 1388.9 | 1696.7 | 1064.9 | 5850.7 (2822.1) |  |
| vale-fights-8 | 360 | 82 / 1212 | physics-engine | 301.0 | 436.1 | 656.8 | 320.6 | 15263.4 (7565.3) | 0.304 |
| vale-fights-32 | 360 | 106 / 1236 | rapier | 1185.6 | 2751.6 | 6148.7 | 1508.2 | 6843.6 (3456.9) |  |
| vale-fights-32 | 360 | 106 / 1236 | physics-engine | 454.6 | 756.7 | 1000.1 | 496.3 | 16662.8 (8562.3) | 0.384 |
| vale-crowded-fights-64 | 360 | 138 / 1268 | rapier | 1511.4 | 3176.6 | 6960.4 | 1803.2 | 7896.2 (3912.7) |  |
| vale-crowded-fights-64 | 360 | 138 / 1268 | physics-engine | 853.1 | 1334.3 | 1982.5 | 919.6 | 19598.1 (9241.6) | 0.520 |

On this machine the engine arm's per-tick physics path costs 0.24–0.52× Rapier's (paired-round medians; absolute times vary with machine load) (the
crowded fixture, where characters overlap most, is the most expensive), while building the
zone costs about 2.4× more (heightfield preparation and per-body inserts). Note that the
Rapier arm rebuilds its world every step by design (determinism), which is part of its cost.
