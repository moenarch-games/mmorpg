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
| vale-mill-ramps | 1.6 / 2.5 (from tick 19) | 0 apart | continues |
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

- mmorpg: 299e830852c535d664167d8e197b83f8c58f1da6
- physics-engine: version = "0.1.0" source = "git+https://github.com/moritzbrantner/physics-engine?rev=86156c2445e1e814cb7b0e52fa2a294fdf991f4f#86156c2445e1e814cb7b0e52fa2a294fdf991f4f"
- rapier3d: version = "0.36.1" source = "registry+https://github.com/rust-lang/crates.io-index"
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- profile: release; 2 warmup + 7 measured rounds per backend, alternating the first backend each round
- sample: one tick's whole physics path (step, gameplay queries, body updates), debris excluded; microseconds; setup is building the zone, its physics share in parentheses

| workload | ticks | moving / bodies | backend | median | p95 | p99 | mean | setup (physics) | engine / rapier, median of paired rounds |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
| two-bots-move | 10 | 77 / 1214 | rapier | 1088.2 | 1566.6 | 2259.6 | 1194.3 | 3326.2 (3215.1) |  |
| two-bots-move | 10 | 77 / 1214 | physics-engine | 149.6 | 519.7 | 529.3 | 190.2 | 7861.4 (7756.8) | 0.162 |
| vale-hub-walk | 280 | 77 / 1214 | rapier | 1105.4 | 1449.7 | 1852.1 | 1121.6 | 2997.3 (2888.5) |  |
| vale-hub-walk | 280 | 77 / 1214 | physics-engine | 159.5 | 306.2 | 416.6 | 198.3 | 7256.6 (7144.2) | 0.177 |
| vale-wolf-hunt | 2200 | 76 / 1213 | rapier | 1336.3 | 1914.2 | 2390.2 | 1390.9 | 3088.0 (2976.2) |  |
| vale-wolf-hunt | 2200 | 76 / 1213 | physics-engine | 219.5 | 316.7 | 454.3 | 213.5 | 6920.2 (6816.6) | 0.155 |
| vale-idle-16 | 360 | 91 / 1228 | rapier | 1130.6 | 1596.4 | 2015.3 | 1150.9 | 6693.6 (3037.9) |  |
| vale-idle-16 | 360 | 91 / 1228 | physics-engine | 191.5 | 361.6 | 534.7 | 225.2 | 14444.1 (7118.2) | 0.198 |
| vale-fights-8 | 360 | 83 / 1220 | rapier | 1440.8 | 2549.8 | 5288.2 | 1610.0 | 7815.6 (3783.7) |  |
| vale-fights-8 | 360 | 83 / 1220 | physics-engine | 301.0 | 576.0 | 1067.4 | 342.8 | 17497.9 (8343.4) | 0.199 |
| vale-fights-32 | 360 | 107 / 1244 | rapier | 1920.6 | 3649.6 | 6307.4 | 2147.7 | 9968.6 (4872.0) |  |
| vale-fights-32 | 360 | 107 / 1244 | physics-engine | 472.5 | 853.7 | 1597.0 | 509.3 | 21728.4 (10786.6) | 0.257 |
| vale-crowded-fights-64 | 360 | 139 / 1276 | rapier | 1556.8 | 2265.5 | 2682.6 | 1603.2 | 7864.2 (3875.6) |  |
| vale-crowded-fights-64 | 360 | 139 / 1276 | physics-engine | 524.2 | 743.1 | 956.7 | 504.8 | 17948.3 (9064.7) | 0.316 |
| vale-ogre-brawl | 360 | 78 / 1215 | rapier | 1267.1 | 1752.7 | 2103.9 | 1307.0 | 6655.9 (3046.5) |  |
| vale-ogre-brawl | 360 | 78 / 1215 | physics-engine | 199.0 | 365.8 | 514.3 | 238.5 | 15734.1 (7844.3) | 0.185 |
| vale-mill-hoist | 640 | 76 / 1213 | rapier | 1167.4 | 1668.8 | 2101.6 | 1182.7 | 6065.7 (2849.6) |  |
| vale-mill-hoist | 640 | 76 / 1213 | physics-engine | 228.5 | 329.4 | 494.3 | 214.0 | 13900.8 (6867.0) | 0.176 |
| vale-mill-ramps | 240 | 77 / 1214 | rapier | 985.1 | 1448.9 | 1844.7 | 1058.3 | 5915.0 (2884.7) |  |
| vale-mill-ramps | 240 | 77 / 1214 | physics-engine | 138.4 | 327.1 | 563.0 | 190.2 | 13984.5 (6867.9) | 0.177 |
| vale-charge-path | 240 | 79 / 1216 | rapier | 1019.7 | 1456.2 | 1823.7 | 1072.6 | 9561.9 (2830.9) |  |
| vale-charge-path | 240 | 79 / 1216 | physics-engine | 146.2 | 324.9 | 498.7 | 196.0 | 21785.5 (6804.7) | 0.183 |
| vale-watch-post | 420 | 77 / 1214 | rapier | 1258.4 | 2034.7 | 3427.8 | 1357.5 | 10055.6 (3029.9) |  |
| vale-watch-post | 420 | 77 / 1214 | physics-engine | 189.2 | 338.3 | 502.9 | 214.9 | 22396.1 (7275.7) | 0.164 |

On this machine the engine arm's per-tick physics path costs 0.16–0.32× Rapier's
(paired-round medians; absolute times vary with machine load; the crowded fixture, where
characters overlap most, is the most expensive). Moving the mill hoist's box in place
(physics-engine #355) brought it back down from 0.15–0.37×. Building the zone costs about 2.2× more
(heightfield preparation and per-body inserts). Note that the
Rapier arm rebuilds its world every step by design (determinism), which is part of its cost.
