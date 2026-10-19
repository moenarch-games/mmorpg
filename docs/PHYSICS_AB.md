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
  rounding step gameplay sees; measured ≤ 0.01), identical command outcomes and door states,
  and the players' grounded state identical on every tick.
- Every query decision on **identical states** (Rapier's state every 60 ticks, recovered onto
  both backends): line of sight and shots for player–creature pairs within 40 m, NPC sight,
  open and closed door-leaf occupancy, ground height and grounded must agree exactly; shot
  fractions within 10⁻⁴. Grounded is not compared for units a fixture embedded in geometry.
- Restored from its canonical snapshot halfway, each arm continues exactly (Atlas 2.1.5).

Fighting fixtures and creatures are compared but not bounded; the test prints their
divergences with `--nocapture`.

## Divergences

Measured at the commit the timing table names:

| Workload | Players (max Δxz / Δy) | Creatures at the end | Engine restored halfway |
|---|---|---|---|
| two-bots-move | 0 / 0 | 0 of 74 apart (max 0.34) | continues |
| vale-hub-walk | 0.01 / 0 | 0 apart (max 0.55) | continues |
| vale-wolf-hunt | 0 / 0 | 73 apart (max 996) | continues |
| vale-idle-16 | 0 / 0 | 0 apart (max 0.55) | continues |
| vale-fights-8 | 30.1 / 0.01 (from tick 181) | 0 apart | continues |
| vale-fights-32 | 14,239 / 0.02 (from tick 1) | 32 apart | continues |
| vale-crowded-fights-64 | 241 / 120 (from tick 1) | 43 apart | continues |

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
3. **Grounded.** Both report static ground within the contact offset plus 5 cm (Rapier's ground
   prediction; physics-engine #348; the engine also requires it walkable). On identical states
   they agree for every unit not embedded in geometry; inside geometry the depenetration
   differs (2.).
4. **Creature drift.** Creatures wander and chase on a zone RNG they share. Over 2,200 ticks
   (`vale-wolf-hunt`) the small differences above move AI decisions and the RNG stream, so most
   creatures end elsewhere; the scripted hunter's path, fights and command outcomes stay
   identical.
5. **Terrain.** Both arms remove flat heightfield cells and leave them to the floor
   half-space (parry heightfield contacts let resting bodies sink; the engine arm passes the
   same cells as `HeightfieldDesc::removed_cells`). Engine overlap queries also see the
   terrain (Rapier skips box–heightfield pairs); door probes only look for units, so
   occupancy agrees.
6. **Contact gap.** Rapier units never came closer than 0.2498 units to a box; engine units came
   as close as 0.04 (`vale-wolf-hunt`) and 0.00 (`vale-crowded-fights-64`), never inside.

## physics-engine gaps found

None open. The gaps found so far are fixed at the pinned rev, and the test now
asserts the engine arm's exact continuation:

1. Snap state was not rebuildable from game state (physics-engine #340): snapping is now
   decided from the geometry at the move's start, so a restored zone continues exactly.
2. Depenetration double-counted coincident surfaces (physics-engine #344).
3. No heightfield cell mask (physics-engine #342): `HeightfieldDesc::removed_cells`.
4. Grounded within two contact offsets instead of Rapier's offset + 5 cm prediction
   (physics-engine #347/#348); the adapter's grounded test for a just-placed body uses the
   same prediction.

## Timing

See the table below, produced by the example in release on the commit it names. A sample is
one tick's whole physics path (the step, every gameplay query and body update the tick makes,
as `World::spent_nanos` counts them; debris excluded); setup is building the zone. Timing is
evidence for physics-engine #330, not a gate.

- mmorpg: aef3bfe8d64a4f9a0dc66665b7df246913476d8c
- physics-engine: version = "0.1.0" source = "git+https://github.com/moritzbrantner/physics-engine?rev=e8aac2fa847fad6c5c72920be83f934b00aa7d7e#e8aac2fa847fad6c5c72920be83f934b00aa7d7e"
- rapier3d: version = "0.36.1" source = "registry+https://github.com/rust-lang/crates.io-index"
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- profile: release; 2 warmup + 7 measured rounds per backend, alternating the first backend each round
- sample: one tick's whole physics path (step, gameplay queries, body updates), debris excluded; microseconds; setup is building the zone, its physics share in parentheses

| workload | ticks | moving / bodies | backend | median | p95 | p99 | mean | setup (physics) | engine / rapier, median of paired rounds |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
| two-bots-move | 10 | 76 / 1206 | rapier | 1419.8 | 1969.0 | 2484.0 | 1516.5 | 4781.6 (4636.4) |  |
| two-bots-move | 10 | 76 / 1206 | physics-engine | 200.4 | 718.5 | 816.7 | 259.0 | 10214.1 (10071.3) | 0.169 |
| vale-hub-walk | 280 | 76 / 1206 | rapier | 1121.0 | 1500.4 | 1806.1 | 1156.5 | 3213.9 (3111.0) |  |
| vale-hub-walk | 280 | 76 / 1206 | physics-engine | 147.7 | 206.9 | 317.5 | 157.3 | 7161.0 (7060.7) | 0.136 |
| vale-wolf-hunt | 2200 | 75 / 1205 | rapier | 1441.6 | 2052.5 | 2450.8 | 1493.8 | 3301.0 (3186.9) |  |
| vale-wolf-hunt | 2200 | 75 / 1205 | physics-engine | 171.6 | 241.9 | 337.3 | 179.1 | 7928.6 (7795.1) | 0.118 |
| vale-idle-16 | 360 | 90 / 1220 | rapier | 1257.4 | 1697.7 | 2052.8 | 1284.1 | 7737.2 (3790.8) |  |
| vale-idle-16 | 360 | 90 / 1220 | physics-engine | 186.8 | 270.0 | 462.3 | 196.0 | 17548.1 (8611.1) | 0.150 |
| vale-fights-8 | 360 | 82 / 1212 | rapier | 1114.9 | 1524.3 | 1954.9 | 1160.0 | 6956.0 (3257.5) |  |
| vale-fights-8 | 360 | 82 / 1212 | physics-engine | 158.0 | 230.3 | 380.9 | 167.1 | 15690.6 (7637.0) | 0.141 |
| vale-fights-32 | 360 | 106 / 1236 | rapier | 1272.2 | 1845.2 | 2398.3 | 1360.2 | 7655.8 (3581.0) |  |
| vale-fights-32 | 360 | 106 / 1236 | physics-engine | 267.1 | 421.7 | 648.2 | 291.8 | 16422.9 (8064.1) | 0.217 |
| vale-crowded-fights-64 | 360 | 138 / 1268 | rapier | 1719.2 | 2663.8 | 3249.6 | 1771.5 | 9449.2 (4593.7) |  |
| vale-crowded-fights-64 | 360 | 138 / 1268 | physics-engine | 441.6 | 714.2 | 997.3 | 466.1 | 20967.6 (10809.9) | 0.263 |

On this machine the engine arm's per-tick physics path costs 0.12–0.26× Rapier's
(paired-round medians; absolute times vary with machine load; the crowded fixture, where
characters overlap most, is the most expensive), while building the zone costs about 2.2×
more (heightfield preparation and per-body inserts). Note that the
Rapier arm rebuilds its world every step by design (determinism), which is part of its cost.
