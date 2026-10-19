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
static box re-added where it moves, its riders carried by the adapter as on Rapier), the floor half-spaces, the
terrain heightfield (`add_heightfield`),
rays (`cast_ray`/`cast_ray_first`), capsule sweeps (`cast_shape`, the charge-path rule),
overlaps (`overlap_shape`) and segment checks. **Debris
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
steep chute) `vale-charge-path` (#57: four level-6 Wardens charge at wolves among the Wolfrun
trunks) and `vale-watch-post` (#305: an arcanist and a ranger fight the Redbrand bandits at the
watch post, whose pieces the blasts break and whose colliders leave both worlds) through the real zone simulation, from the same initial state with the same inputs and tick count.

Both arms carry the mill hoist's riders in the adapter the same way: a downward support cast
filtered to the platform finds them, its displacement is added to their move, and a sinking
platform moves before the units, a rising one after them. `vale-mill-hoist` is held to the
scripted-player tolerances below.

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
- in `vale-charge-path`, let some Warden charge;
- in `vale-watch-post`, damage some piece of the post, with identical piece states on both;
- in `vale-mill-ramps`, carry the walker up the cart ramp onto the dock (feet 1.5 m up within 3
  units) and back to the ground, and never let the chute's player onto the dock.

Across backends:

- Scripted workloads (the bot scenarios, `vale-idle-16` and `vale-mill-hoist`): players within **1 unit** (the
  rounding step gameplay sees; measured ≤ 0.15), identical command outcomes and door states,
  and the players' grounded state identical on every tick.
- Every query decision on **identical states** (Rapier's state every 60 ticks, recovered onto
  both backends): line of sight, shots and charge paths (a capsule `cast_shape`, #57) for player–creature
  pairs within 40 m, NPC sight,
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
| vale-mill-hoist | 0.01 / 0.15 | 3 apart (max 3.4) | continues |
| vale-mill-ramps | 33.9 / 59.2 (from tick 10) | 0 apart | continues |
| vale-charge-path | 0 / 0 | 0 apart (max 0.34) | continues |
| vale-watch-post | 0 / 0 | 14 apart (max 671) | continues |

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

One open:

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

Not engine gaps after all: moving platforms (#56) need no engine feature, since both arms
carry riders in adapter code; a ray starting exactly on a face reads differently only because
the Rapier arm converts centimetres to metres in `f32` (Rapier's solid `cast_ray` also starts
at time 0 on the face). The engine arm re-adds a moving platform's static box every tick it
moves until physics-engine #352 moves a static body in place.

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

- mmorpg: 22eb9a4110f227dc87f31b359212e26fd9c70736
- physics-engine: version = "0.1.0" source = "git+https://github.com/moritzbrantner/physics-engine?rev=e8aac2fa847fad6c5c72920be83f934b00aa7d7e#e8aac2fa847fad6c5c72920be83f934b00aa7d7e"
- rapier3d: version = "0.36.1" source = "registry+https://github.com/rust-lang/crates.io-index"
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- profile: release; 2 warmup + 7 measured rounds per backend, alternating the first backend each round
- sample: one tick's whole physics path (step, gameplay queries, body updates), debris excluded; microseconds; setup is building the zone, its physics share in parentheses

| workload | ticks | moving / bodies | backend | median | p95 | p99 | mean | setup (physics) | engine / rapier, median of paired rounds |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
| two-bots-move | 10 | 77 / 1214 | rapier | 1492.2 | 3886.6 | 9526.6 | 1878.1 | 4209.4 (4074.1) |  |
| two-bots-move | 10 | 77 / 1214 | physics-engine | 199.3 | 655.4 | 2362.1 | 281.1 | 10759.3 (10626.1) | 0.154 |
| vale-hub-walk | 280 | 77 / 1214 | rapier | 1115.3 | 1468.3 | 1772.6 | 1122.1 | 3154.2 (3039.1) |  |
| vale-hub-walk | 280 | 77 / 1214 | physics-engine | 186.7 | 456.3 | 588.9 | 261.6 | 7018.9 (6924.0) | 0.234 |
| vale-wolf-hunt | 2200 | 76 / 1213 | rapier | 1254.3 | 1736.0 | 2058.8 | 1303.4 | 2897.9 (2785.9) |  |
| vale-wolf-hunt | 2200 | 76 / 1213 | physics-engine | 325.7 | 430.3 | 571.9 | 273.1 | 6837.1 (6727.9) | 0.209 |
| vale-idle-16 | 360 | 91 / 1228 | rapier | 1111.6 | 1538.0 | 1897.8 | 1118.9 | 5973.2 (2823.8) |  |
| vale-idle-16 | 360 | 91 / 1228 | physics-engine | 184.9 | 487.1 | 691.5 | 282.1 | 14017.7 (6816.8) | 0.255 |
| vale-fights-8 | 360 | 83 / 1220 | rapier | 959.9 | 1394.6 | 1848.8 | 1002.5 | 5662.5 (2433.5) |  |
| vale-fights-8 | 360 | 83 / 1220 | physics-engine | 170.4 | 420.0 | 534.2 | 238.2 | 11827.3 (5869.4) | 0.238 |
| vale-fights-32 | 360 | 107 / 1244 | rapier | 987.4 | 1356.7 | 1847.8 | 1009.9 | 4867.6 (2328.5) |  |
| vale-fights-32 | 360 | 107 / 1244 | physics-engine | 240.1 | 513.3 | 692.1 | 303.7 | 11239.6 (5542.1) | 0.301 |
| vale-crowded-fights-64 | 360 | 139 / 1276 | rapier | 988.0 | 1446.3 | 1894.8 | 1019.7 | 5460.5 (2627.7) |  |
| vale-crowded-fights-64 | 360 | 139 / 1276 | physics-engine | 330.8 | 594.3 | 832.8 | 377.7 | 11521.5 (5638.1) | 0.368 |
| vale-ogre-brawl | 360 | 78 / 1215 | rapier | 932.6 | 1435.7 | 2211.0 | 995.2 | 5029.4 (2301.9) |  |
| vale-ogre-brawl | 360 | 78 / 1215 | physics-engine | 186.2 | 495.5 | 760.2 | 257.0 | 11284.7 (5863.9) | 0.231 |
| vale-mill-hoist | 640 | 76 / 1213 | rapier | 1215.9 | 1552.6 | 1828.4 | 1211.6 | 6128.7 (2767.2) |  |
| vale-mill-hoist | 640 | 76 / 1213 | physics-engine | 324.0 | 499.2 | 720.4 | 285.6 | 14002.5 (6781.4) | 0.242 |
| vale-mill-ramps | 240 | 77 / 1214 | rapier | 1074.5 | 1436.9 | 1775.0 | 1108.1 | 6060.1 (2816.3) |  |
| vale-mill-ramps | 240 | 77 / 1214 | physics-engine | 162.9 | 420.1 | 535.4 | 237.0 | 13812.3 (6991.3) | 0.217 |
| vale-charge-path | 240 | 79 / 1216 | rapier | 976.3 | 1334.9 | 1555.4 | 1011.7 | 8981.3 (2741.8) |  |
| vale-charge-path | 240 | 79 / 1216 | physics-engine | 139.9 | 431.0 | 550.1 | 230.3 | 21033.5 (6445.2) | 0.227 |
| vale-watch-post | 420 | 77 / 1214 | rapier | 1143.8 | 1505.0 | 1819.1 | 1152.5 | 9106.3 (2708.8) |  |
| vale-watch-post | 420 | 77 / 1214 | physics-engine | 170.2 | 406.4 | 506.8 | 237.8 | 20239.9 (6378.8) | 0.208 |

On this machine the engine arm's per-tick physics path costs 0.15–0.37× Rapier's
(paired-round medians; absolute times vary with machine load; the crowded fixture, where
characters overlap most, is the most expensive). The mill hoist (#56) raised it from
0.12–0.26×: the engine arm re-adds the hoist's static box every tick the hoist moves (in 280
of every 620 ticks), since physics-engine cannot move a static body. Building the zone costs about 2.2× more
(heightfield preparation and per-body inserts). Note that the
Rapier arm rebuilds its world every step by design (determinism), which is part of its cost.
