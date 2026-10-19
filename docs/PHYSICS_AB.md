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
static box moved in place with `set_static_position`, its riders carried by the adapter as on Rapier), the floor half-spaces, the
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
  both backends): line of sight, shots, charge paths (a capsule `cast_shape`, #57) and creature routes
  (#307: shape casts and overlap queries on the navigation grid) for player–creature pairs
  within 40 m, NPC sight,
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
| vale-mill-ramps | 1.8 / 2.7 (from tick 19) | 0 apart | continues |
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

None open.

Not engine gaps after all: moving platforms (#56) need no engine feature, since both arms
carry riders in adapter code; a ray starting exactly on a face reads differently only because
the Rapier arm converts centimetres to metres in `f32` (Rapier's solid `cast_ray` also starts
at time 0 on the face). A Rapier-only artefact remains on the cart ramp: its controller tops
the dock's edge 2.5 units high for a tick before settling, so `vale-mill-ramps` allows 3 units.

Fixed at the pinned rev, and the test now
asserts the engine arm's exact continuation:

1. Snap state was not rebuildable from game state (physics-engine #340): snapping is now
   decided from the geometry at the move's start, so a restored zone continues exactly.
2. Depenetration double-counted coincident surfaces (physics-engine #344).
3. No heightfield cell mask (physics-engine #342): `HeightfieldDesc::removed_cells`.
4. Grounded within two contact offsets instead of Rapier's offset + 5 cm prediction
   (physics-engine #347/#348); the adapter's grounded test for a just-placed body uses the
   same prediction.
5. Too-steep slopes climbed part-way (physics-engine #351): a character walking into the 60°
   mill chute rode up to 58 units and slid back; slopes steeper than `max_slope` are now
   walls, as on Rapier.
6. No way to move a static body (physics-engine #355, `World::set_static_position`): the
   engine arm now moves the mill hoist's box in place instead of re-adding it every tick.

## Timing

See the table below, produced by the example in release on the commit it names. A sample is
one tick's whole physics path (the step, every gameplay query and body update the tick makes,
as `World::spent_nanos` counts them; debris excluded); setup is building the zone. Timing is
evidence for physics-engine #330, not a gate.

- mmorpg: 96497c9f181d9a94d3d9dde4b33f7a290b02679e
- physics-engine: version = "0.1.0" source = "git+https://github.com/moritzbrantner/physics-engine?rev=ae64adbdb2d1af74f89f5befe287a8029eeaa072#ae64adbdb2d1af74f89f5befe287a8029eeaa072"
- rapier3d: version = "0.36.1" source = "registry+https://github.com/rust-lang/crates.io-index"
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- profile: release; 2 warmup + 7 measured rounds per backend, alternating the first backend each round
- sample: one tick's whole physics path (step, gameplay queries, body updates), debris excluded; microseconds; setup is building the zone, its physics share in parentheses

| workload | ticks | moving / bodies | backend | median | p95 | p99 | mean | setup (physics) | engine / rapier, median of paired rounds |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
| two-bots-move | 10 | 77 / 1214 | rapier | 971.7 | 1499.4 | 1626.6 | 1044.2 | 3074.7 (2983.0) |  |
| two-bots-move | 10 | 77 / 1214 | physics-engine | 136.2 | 460.4 | 1069.2 | 183.6 | 6659.1 (6556.7) | 0.164 |
| vale-hub-walk | 280 | 77 / 1214 | rapier | 898.1 | 1216.8 | 1457.1 | 915.5 | 2505.1 (2417.2) |  |
| vale-hub-walk | 280 | 77 / 1214 | physics-engine | 128.4 | 255.7 | 355.8 | 165.0 | 5813.4 (5725.3) | 0.182 |
| vale-wolf-hunt | 2200 | 76 / 1213 | rapier | 985.0 | 1351.2 | 1591.8 | 1017.0 | 2484.1 (2389.0) |  |
| vale-wolf-hunt | 2200 | 76 / 1213 | physics-engine | 178.4 | 236.2 | 323.3 | 166.4 | 5764.7 (5678.6) | 0.164 |
| vale-idle-16 | 360 | 91 / 1228 | rapier | 877.6 | 1150.0 | 1338.4 | 876.2 | 5073.0 (2341.8) |  |
| vale-idle-16 | 360 | 91 / 1228 | physics-engine | 145.9 | 269.0 | 353.7 | 175.3 | 11106.1 (5615.5) | 0.201 |
| vale-fights-8 | 360 | 83 / 1220 | rapier | 866.3 | 1122.6 | 1294.6 | 877.6 | 4913.2 (2353.4) |  |
| vale-fights-8 | 360 | 83 / 1220 | physics-engine | 160.9 | 256.6 | 329.5 | 175.5 | 11251.2 (5562.8) | 0.200 |
| vale-fights-32 | 360 | 107 / 1244 | rapier | 969.5 | 1272.2 | 1489.8 | 978.2 | 5057.0 (2501.9) |  |
| vale-fights-32 | 360 | 107 / 1244 | physics-engine | 258.1 | 374.2 | 519.1 | 250.7 | 11805.5 (5961.8) | 0.257 |
| vale-crowded-fights-64 | 360 | 139 / 1276 | rapier | 1006.0 | 1404.6 | 1825.8 | 1017.3 | 5210.2 (2663.6) |  |
| vale-crowded-fights-64 | 360 | 139 / 1276 | physics-engine | 326.9 | 461.3 | 724.4 | 313.5 | 11725.3 (5776.4) | 0.308 |
| vale-ogre-brawl | 360 | 78 / 1215 | rapier | 900.3 | 1260.5 | 1806.0 | 938.1 | 5660.4 (2832.2) |  |
| vale-ogre-brawl | 360 | 78 / 1215 | physics-engine | 143.9 | 254.2 | 402.0 | 170.7 | 11436.9 (5790.9) | 0.182 |
| vale-mill-hoist | 640 | 76 / 1213 | rapier | 948.2 | 1280.1 | 1746.8 | 958.1 | 5176.5 (2321.4) |  |
| vale-mill-hoist | 640 | 76 / 1213 | physics-engine | 183.1 | 241.3 | 354.5 | 169.1 | 11668.6 (5819.1) | 0.177 |
| vale-mill-ramps | 240 | 77 / 1214 | rapier | 826.2 | 1185.1 | 1561.5 | 877.6 | 4913.0 (2329.0) |  |
| vale-mill-ramps | 240 | 77 / 1214 | physics-engine | 117.1 | 242.9 | 379.2 | 151.8 | 11693.3 (5578.9) | 0.173 |
| vale-charge-path | 240 | 79 / 1216 | rapier | 785.5 | 1142.4 | 1526.9 | 835.0 | 7718.5 (2242.7) |  |
| vale-charge-path | 240 | 79 / 1216 | physics-engine | 111.1 | 250.2 | 404.7 | 151.3 | 17359.1 (5369.4) | 0.177 |
| vale-watch-post | 420 | 77 / 1214 | rapier | 954.8 | 1331.2 | 1926.7 | 975.8 | 7545.2 (2410.8) |  |
| vale-watch-post | 420 | 77 / 1214 | physics-engine | 149.5 | 296.2 | 548.6 | 178.7 | 18875.9 (5734.2) | 0.170 |

On this machine the engine arm's per-tick physics path costs 0.16–0.31× Rapier's
(paired-round medians; absolute times vary with machine load; the crowded fixture, where
characters overlap most, is the most expensive). Moving the mill hoist's box in place
(physics-engine #355) brought it back down from 0.15–0.37×; creature navigation (#307) adds a
shape cast per chasing creature and tick, plus A* overlap queries while one is blocked. Building the zone costs about 2.2× more
(heightfield preparation and per-body inserts). Note that the
Rapier arm rebuilds its world every step by design (determinism), which is part of its cost.
