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
NPCs, standing props), ramps (#59, `Shape::Wedge` turned about Y), moving platforms (#56, a
static box re-added where it moves; carrying riders is unsupported), the floor half-spaces, the
terrain heightfield (`add_heightfield`),
rays (`cast_ray`/`cast_ray_first`), overlaps (`overlap_shape`) and segment checks. **Debris
(#288) is not supported by the engine arm**: it stays on Rapier in both arms
(`physics::rapier::step_debris` over the adapter's fixed bodies) and is excluded from timing.

## What the test asserts

`crates/mmorpg-core/tests/physics_ab.rs` replays the bot scenarios `two-bots-move`,
`vale-hub-walk` and `vale-wolf-hunt` (command by command, as the bot runner submits them; the
Rapier arm reproduces each scenario's expected positions exactly) and the four Greyhaven combat
workload fixtures (`tests/support/greyhaven_workload.rs`, shared with `mmorpg-protocol`) and the
`vale-ogre-brawl` script (#297: three players by the Hill Ogre; one provokes it and runs past
another standing in its way, a third walks into its side) and `vale-mill-hoist` (#56: a
player rides the mill hoist up and walks off onto the windmill's gallery) and `vale-mill-ramps`
(#59: one player walks up the dock's cart ramp, back down and across it, another walks into the
steep chute) through the real zone simulation, from the same initial state with the same inputs and tick count.

`vale-mill-hoist` is **unsupported on physics-engine** (no moving support, gap below): the
engine arm refuses its first step with a rider with an `unsupported` error, which the test
asserts; Rapier alone keeps the shared invariants, replays and continues exactly, and its
rider ends on the gallery. Nothing is substituted for the missing capability.

On **each** backend, every run must:

- keep every player and creature state finite;
- keep unit capsules out of content boxes, door leaves and standing props, and above the floor
  and terrain, beyond 0.01 units (both backends keep a 0.25-unit gap; `f32` resolves ~10⁻³ units
  at the zone's 10⁴-unit magnitudes). Allowed: squat creatures' authored floor overlap (capsule
  radius minus half height) and units a fixture places inside geometry at tick 1;
- report every unit that has rested (zero velocity, unmoved) for three ticks as grounded;
- replay bit-identically (canonical snapshots, unit bodies and command outcomes);
- in `vale-ogre-brawl`, keep every player's capsule out of the ogre's (beyond 0.01 units), shove
  a player aside at least once and stomp;
- in `vale-mill-ramps`, carry the walker up the cart ramp onto the dock (feet 1.5 m up within 3
  units) and back to the ground, and never let the chute's player onto the dock.

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
| two-bots-move | 0 / 0 | 0 of 75 apart (max 0.34) | continues |
| vale-hub-walk | 0.01 / 0 | 0 apart (max 0.55) | continues |
| vale-wolf-hunt | 0 / 0 | 73 apart (max 996) | continues |
| vale-idle-16 | 0 / 0 | 0 apart (max 0.55) | continues |
| vale-fights-8 | 30.1 / 0.01 (from tick 301) | 5 apart | continues |
| vale-fights-32 | 13,953 / 0.02 (from tick 1) | 34 apart | continues |
| vale-crowded-fights-64 | 241 / 120 (from tick 1) | 43 apart | continues |
| vale-ogre-brawl | 31.3 / 1.9 (from tick 27) | 3 apart (max 21.5) | continues |
| vale-mill-hoist | unsupported on physics-engine | | |
| vale-mill-ramps | 33.9 / 59.2 (from tick 10) | 0 apart | continues |

Every query decision on identical states agreed (333–11,846 probes per workload).

Intentional or explained differences:

1. **Head-on characters.** The engine arm uses physics-engine's native sidestep (within ~5° of
   head-on, step to the right) instead of the Rapier arm's `slide_around` workaround (within 60°,
   both step aside plus a 1-unit outward nudge). The physics unit tests for head-on and off-axis
   pairs pass on both. Characters do not push each other on either: a large creature's shove
   (#297) is a gameplay forced move decided before the step, the same on both. In
   `vale-ogre-brawl` the player walking into the ogre's side slides around it differently, so
   players and the ogre part by up to 31 units; every shove and stomp still happens on both.
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

Two open:

- **Climbing too-steep slopes** (#59). On a wedge steeper than `max_slope` (the mill chute,
  60°, 1.5 m high), a character walking into it rides part-way up and slides back, over and
  over (feet oscillating 0–58 units above the ground), where Rapier's controller
  (`max_slope_climb_angle` 45°) stops it at the foot. Neither lets it onto the top, so the
  invariant holds, but positions part by up to 59 units. Rapier API: `KinematicCharacterController`
  with `max_slope_climb_angle = min_slope_slide_angle = π/4`. Minimal repro:
  `physics::tests::a_unit_walks_up_a_shallow_ramp_onto_the_dock_but_not_up_a_steep_one`
  (a `Shape::Wedge` 208 units high over a 120-unit run, rising toward a capsule walking
  into it at 8 units per tick; compare its height per tick). The cart ramp itself agrees within 1.6 units (the
  engine tops out at the dock edge 2.5 units lower than Rapier's controller overshoots).

- **Moving support** (#56). A character standing on a moving body is not carried: support,
  grounded and snapping count only static bodies, a static body cannot move, and an external
  (kinematic) body neither supports nor carries a character. MMORPG needs a moving platform
  (a box the game moves each tick to a scheduled centre) that carries the characters standing
  on it and is walkable support while it moves. The Rapier arm does it with
  `KinematicCharacterController::move_shape` plus a downward `cast_shape` filtered to the
  platform's collider to find riders, whose desired translation gains the platform's
  displacement (the platform moves before the units when it sinks, after them when it rises).
  Minimal repro: `physics::tests::physics_engine_refuses_to_carry_a_rider` (a 2 m box at
  rest flush with a floor, a capsule on it, the box moved 5 units: the rider must follow).
  The engine arm moves a platform by removing and re-adding its static box every tick it
  moves, which also costs time (below).

Fixed at the pinned rev, and the test now
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

- mmorpg: 71c7b0ea9fc6043f882b8c2473043ec184e60f01
- physics-engine: version = "0.1.0" source = "git+https://github.com/moritzbrantner/physics-engine?rev=e8aac2fa847fad6c5c72920be83f934b00aa7d7e#e8aac2fa847fad6c5c72920be83f934b00aa7d7e"
- rapier3d: version = "0.36.1" source = "registry+https://github.com/rust-lang/crates.io-index"
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- profile: release; 2 warmup + 7 measured rounds per backend, alternating the first backend each round
- sample: one tick's whole physics path (step, gameplay queries, body updates), debris excluded; microseconds; setup is building the zone, its physics share in parentheses

| workload | ticks | moving / bodies | backend | median | p95 | p99 | mean | setup (physics) | engine / rapier, median of paired rounds |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
| two-bots-move | 10 | 77 / 1211 | rapier | 1504.0 | 2261.9 | 2680.5 | 1590.3 | 4460.9 (4321.4) |  |
| two-bots-move | 10 | 77 / 1211 | physics-engine | 203.7 | 710.5 | 838.7 | 262.7 | 10990.8 (10854.7) | 0.166 |
| vale-hub-walk | 280 | 77 / 1211 | rapier | 1227.1 | 1668.0 | 1954.6 | 1243.8 | 3569.5 (3449.8) |  |
| vale-hub-walk | 280 | 77 / 1211 | physics-engine | 200.1 | 527.0 | 665.6 | 293.2 | 8005.2 (7899.7) | 0.238 |
| vale-wolf-hunt | 2200 | 76 / 1210 | rapier | 1382.4 | 1876.5 | 2204.0 | 1426.9 | 3328.9 (3194.1) |  |
| vale-wolf-hunt | 2200 | 76 / 1210 | physics-engine | 356.9 | 483.8 | 623.5 | 300.1 | 7871.0 (7762.3) | 0.212 |
| vale-idle-16 | 360 | 91 / 1225 | rapier | 1185.6 | 1538.8 | 1750.5 | 1177.4 | 6600.8 (3044.8) |  |
| vale-idle-16 | 360 | 91 / 1225 | physics-engine | 189.9 | 529.6 | 628.2 | 299.6 | 14915.2 (7423.3) | 0.254 |
| vale-fights-8 | 360 | 83 / 1217 | rapier | 1199.3 | 1515.3 | 1730.2 | 1206.1 | 6387.7 (2939.6) |  |
| vale-fights-8 | 360 | 83 / 1217 | physics-engine | 192.0 | 523.9 | 657.9 | 292.5 | 14911.7 (7391.3) | 0.243 |
| vale-fights-32 | 360 | 107 / 1241 | rapier | 1326.4 | 1695.8 | 1988.6 | 1334.6 | 6319.2 (3003.7) |  |
| vale-fights-32 | 360 | 107 / 1241 | physics-engine | 314.5 | 677.4 | 800.8 | 400.0 | 14929.5 (7625.5) | 0.301 |
| vale-crowded-fights-64 | 360 | 139 / 1273 | rapier | 1371.8 | 1944.8 | 2333.0 | 1392.8 | 6703.6 (3263.2) |  |
| vale-crowded-fights-64 | 360 | 139 / 1273 | physics-engine | 456.9 | 831.2 | 962.0 | 522.0 | 15452.1 (7838.3) | 0.377 |
| vale-ogre-brawl | 360 | 78 / 1212 | rapier | 1203.7 | 1610.1 | 1905.7 | 1237.8 | 6812.0 (3142.8) |  |
| vale-ogre-brawl | 360 | 78 / 1212 | physics-engine | 191.5 | 487.8 | 602.3 | 286.3 | 14854.9 (7462.1) | 0.231 |
| vale-mill-hoist | 640 | 76 / 1210 | rapier | 1304.2 | 1674.7 | 1948.4 | 1296.7 | 6732.9 (3119.3) |  |
| vale-mill-hoist | 640 | | physics-engine | unsupported: moving support: a platform that carries characters | | | | | |
| vale-mill-ramps | 240 | 77 / 1211 | rapier | 1075.2 | 1437.5 | 1702.1 | 1126.7 | 6065.5 (2864.5) |  |
| vale-mill-ramps | 240 | 77 / 1211 | physics-engine | 176.8 | 479.7 | 585.2 | 262.4 | 14375.2 (7154.8) | 0.233 |

On this machine the engine arm's per-tick physics path costs 0.17–0.38× Rapier's
(paired-round medians; absolute times vary with machine load; the crowded fixture, where
characters overlap most, is the most expensive). The mill hoist (#56) raised it from
0.12–0.26×: the engine arm re-adds the hoist's static box every tick the hoist moves (in 280
of every 620 ticks), since physics-engine cannot move a static body. Building the zone costs about 2.2× more
(heightfield preparation and per-body inserts). Note that the
Rapier arm rebuilds its world every step by design (determinism), which is part of its cost.
