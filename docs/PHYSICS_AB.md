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
  (debris and props included), and `mmorpg_core::physics_ab` exposes read-only probes that ask the
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
overlaps (`overlap_shape`) and segment checks, and dynamic bodies (mmorpg#442): debris (#288)
and the heavy crates' slides (#58) are engine dynamic boxes (rotation-locked for crates) in a
world rebuilt each tick from the nearby game bodies, units as fixed capsules, as the Rapier arm
rebuilds its own; two substeps of four passes for debris and of one for crates, as
physics-engine's `debris` and `props` oracle scenarios calibrate them. A crate at rest is a fixed
box on both arms. Both arms give the pieces friction 0.8 and restitution 0.15 and leave fixed
geometry and the heightfield at the defaults (0.5, 0); both average a pair's coefficients
(physics-engine #391).

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
watch post, whose pieces the blasts break and whose colliders leave both worlds) and
`vale-crate-shove` (#58: two level-6 Wardens Shield Bash Millbrook's heavy crates across the
yard, then one walks into its crate where it rests) and `vale-crate-smash` (#288: two players
smash Greyhaven Outpost's crates 900 and 901, whose panels fly, settle and despawn) and
`vale-missiles` (#289: four level-6
Arcanists Firebolt wolves among the Wolfrun trunks, then cast free Firebolts into the woods) and
`vale-traversal` (#413: a Warden and a Ranger in Greyhaven Outpost make running jumps under an
experimental action graph whose airborne phase keeps the realized momentum and steers at most
6 m/s², letting go or turning back mid-air) and `vale-climb` (#490: one player jumps at a
highland crag, climbs it, lets go, grabs it again and climbs to its top; another mantles the
mill's loading dock; the zone decides both on rays and capsule sweeps, so the two arms answer the
same climb and mantle queries)
through the real zone simulation, from the same initial state with the same inputs and tick count.

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
- in `vale-crate-shove`, slide both crates north and bring them to rest, ending within 13
  units (5% of the 265-unit slide, the `props` oracle bound) of each other across the arms;
- in `vale-crate-smash`, break both crates into twelve panels at the same tick on both arms,
  keep every piece finite and above the ground, settle every piece before it despawns, and
  rest each within 150 units (the `debris` oracle bound) of Rapier's;
- in `vale-traversal`, keep the Warden flying south on its momentum two (30 Hz) ticks after it
  let go of the keys mid-air;
- in `vale-missiles`, strike some wolf with a missile, with every missile spent the same way
  (in a unit or in the world) on both arms;
- in `vale-climb`, end with the climber standing on the crag's top and the mantler on the
  dock's deck, each within a unit;
- in `vale-mill-ramps`, carry the walker up the cart ramp onto the dock (feet 1.5 m up within 3
  units) and back to the ground, and never let the chute's player onto the dock.

Across backends:

- Scripted workloads (`two-bots-move`, `vale-hub-walk`, `vale-idle-16`, `vale-mill-hoist`,
  `vale-crate-shove`, `vale-crate-smash`, `vale-missiles`, `vale-traversal` and `vale-climb`): players within **1 unit** (the
  rounding step gameplay sees; measured ≤ 0.15; in `vale-crate-shove` plus the crates' gap,
  since a player walks into its resting crate), identical command outcomes and door states,
  and the players' grounded state identical on every tick.
- Every query decision on **identical states** (Rapier's state every 2 s, recovered onto
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
| vale-wolf-hunt | 0 / 0 | 73 apart (max 1,098) | continues |
| vale-idle-16 | 0 / 0 | 0 apart (max 0.55) | continues |
| vale-fights-8 | 56.5 / 0.01 (from tick 181) | 1 apart | continues |
| vale-fights-32 | 14,084 / 82.4 (from tick 1) | 34 apart | continues |
| vale-crowded-fights-64 | 241 / 120 (from tick 1) | 51 apart | continues |
| vale-ogre-brawl | 31.3 / 1.9 (from tick 27) | 3 apart (max 21.5) | continues |
| vale-mill-hoist | 0.01 / 0.15 | 3 apart (max 3.4) | continues |
| vale-mill-ramps | 1.8 / 2.7 (from tick 19) | 0 apart | continues |
| vale-charge-path | 0 / 0 | 0 apart (max 0.34) | continues |
| vale-watch-post | 0 / 0 | 14 apart (max 671) | continues |
| vale-crate-shove | 5.0 / 0 (player 2 stops at its crate) | 3 apart (max 5.0); crates 5.0 short of Rapier's 265-unit slide | continues |
| vale-crate-smash | 0 / 0 | 22 apart (max 576); settled debris within 58.3 | continues |
| vale-missiles | 0 / 0 | 1 apart (max 6.4); same missiles strike, world hits within 0.001 | continues |
| vale-traversal | 0.02 / 0.01 | 0 apart (max 0.875) | continues |
| vale-climb | 0.41 / 0.85 | 26 apart (max 942) | continues |

Every query decision on identical states agreed (up to 20,468 probes per workload).

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

## 60 Hz (#379)

The zone runs at 60 Hz; the scripted workloads keep their 30 Hz timetables at the same moments
(commands on the matching even ticks, lengths, parity and penetration sampling in seconds).
`vale-wolf-hunt` is no longer bounded: its creatures part from tick 1 and reach the scripted
hunter's fights after 38 s (tick 2276), where the players part by up to 81 units. Both arms'
support tests skip a too-steep surface met first and look for walkable ground below it. 60 Hz
exposed two engine gaps (gaps 3 and 4 below); both are fixed, and the check holds for every unit
again.

## physics-engine gaps found

1. **Contact material combine rule** (fixed, physics-engine #391 / #392). Rapier averages two
   colliders' friction and restitution by default; physics-engine took the larger friction
   and the smaller restitution. With the pieces' 0.8 / 0.15 against Rapier's default 0.5 / 0 on
   fixed geometry, Rapier slides a crate with μ = 0.65 and the engine with μ = 0.8: Shield-Bashed
   crates slid 215 units on the engine against Rapier's 265 (0.81 = 0.65 / 0.8). physics-engine
   now has Rapier's per-body combine rules (default `Average`, default friction 0.5), and the
   adapter passes the game's own materials: the crates end 5 units short of Rapier's (260 vs
   265) and settled debris within 58 units of Rapier's.
2. **Restore on a heightfield slope** (open, physics-engine #394). On the engine arm, a character
   resting on a sloped heightfield cell moves 0.4–2 units on the first tick after a restore, while
   the uninterrupted run holds it still (the lake lurkers in `vale-fights-8`/`-32` once Stillwater
   Lake rose onto the #423 terrace). The grounded cache is ruled out; contact state carried between
   moves is suspected. Workaround: the lake's level rim is wider and the lurkers stand on it.
3. **Grounded at a too-steep slope's foot** (fixed, physics-engine #395 / #398). A player pressing
   into the mill chute's foot was not grounded on the engine arm. The support cast met the 60°
   face first and stopped there, while Rapier's arm finds the floor below. The engine's support
   test and the adapter's grounded fallback now pass over too-steep faces.
4. **Walking into a heightfield cliff** (fixed, physics-engine #396 / #400, rust-kernels#177).
   A terrain is one body, so a cast reports one contact with it. A capsule touching two cliff
   faces, or a face and the floor, could be told about the face it was leaving, and it then
   sank into the other. Terrain casts now report what the motion runs into.
5. **Walking up and down a heightfield slope** (open; owner decision 2026-10-25, no engine
   change). On the Wolfrun Climb's ledge and cutting (#423 slice 3) the two controllers walk a
   slope at slightly different paces: the engine's character travels about 0.25 % farther per
   tick than Rapier's, and Rapier drifts a little sideways while the engine holds its line.
   `vale-wolf-hunt`'s hunter parts by up to 9 units by the top. The owner chose a bounded climb
   tolerance over an engine change: on every tick of its scripted walk (to tick 1,434, before
   it turns toward the den wolf) the players agree within 1 unit plus 0.2 % of the distance
   each has walked on slopes so far (`SLOPE_DIVERGENCE_PER_UNIT`, about 14 units by the end of
   the climb). Flat walking adds nothing, so every comparison before the first slope stays
   exact. The Stillwater lurkers wander only 2 m to keep to the level rim (gap 2), and wolves
   and bandits stand clear of the new climbs.

Dynamic bodies themselves (debris #288, movable crates #58) need no engine feature: the engine
arm runs them since mmorpg#442 (physics-engine C18).

Not engine gaps after all: climbing and mantling (#490) need no engine feature: a climber or
mantler is weightless like a swimmer and moves along the zone's velocity, and the zone decides
both on rays and capsule sweeps both arms answer alike. A mantle lifts the body 10 units above
the ledge before moving across, clear of both controllers' 5-unit ground snap: a body exactly at
that distance is snapped by one arm and not the other. Moving platforms (#56) need no engine
feature either, since both arms
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
as `World::spent_nanos` counts them; since mmorpg#442 debris and props too, which the table below predates); setup is building the zone. Timing is
evidence for physics-engine #330, not a gate.

- mmorpg: aacd5638e9ed0b2abc0d81b60073d918f3ff5d5b plus the #289 change (magic missiles)
- physics-engine: version = "0.1.0" source = "git+https://github.com/moritzbrantner/physics-engine?rev=ae64adbdb2d1af74f89f5befe287a8029eeaa072#ae64adbdb2d1af74f89f5befe287a8029eeaa072"
- rapier3d: version = "0.36.1" source = "registry+https://github.com/rust-lang/crates.io-index"
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)
- profile: release; 2 warmup + 7 measured rounds per backend, alternating the first backend each round
- sample: one tick's whole physics path (step, gameplay queries, body updates), debris excluded; microseconds; setup is building the zone, its physics share in parentheses

| workload | ticks | moving / bodies | backend | median | p95 | p99 | mean | setup (physics) | engine / rapier, median of paired rounds |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
| two-bots-move | 10 | 77 / 1216 | rapier | 882.8 | 1299.0 | 1639.9 | 935.3 | 2590.7 (2502.5) |  |
| two-bots-move | 10 | 77 / 1216 | physics-engine | 114.5 | 397.5 | 475.5 | 145.0 | 6091.7 (6003.7) | 0.155 |
| vale-hub-walk | 280 | 77 / 1216 | rapier | 966.2 | 1330.5 | 1757.3 | 984.3 | 2742.4 (2631.7) |  |
| vale-hub-walk | 280 | 77 / 1216 | physics-engine | 142.4 | 290.1 | 475.9 | 174.7 | 6116.0 (6032.6) | 0.173 |
| vale-wolf-hunt | 2200 | 76 / 1215 | rapier | 1133.9 | 1765.8 | 2369.7 | 1216.3 | 3008.8 (2890.7) |  |
| vale-wolf-hunt | 2200 | 76 / 1215 | physics-engine | 196.2 | 283.9 | 425.3 | 190.3 | 6037.9 (5952.2) | 0.161 |
| vale-idle-16 | 360 | 91 / 1230 | rapier | 918.7 | 1180.3 | 1431.4 | 913.2 | 5620.3 (2580.8) |  |
| vale-idle-16 | 360 | 91 / 1230 | physics-engine | 152.9 | 276.2 | 385.2 | 182.9 | 12308.7 (6057.0) | 0.199 |
| vale-fights-8 | 360 | 83 / 1222 | rapier | 928.6 | 1247.3 | 1470.9 | 966.3 | 5436.4 (2457.8) |  |
| vale-fights-8 | 360 | 83 / 1222 | physics-engine | 174.3 | 280.1 | 378.3 | 190.8 | 11745.0 (5990.2) | 0.196 |
| vale-fights-32 | 360 | 107 / 1246 | rapier | 1016.4 | 1326.0 | 1574.6 | 1023.4 | 5383.4 (2566.7) |  |
| vale-fights-32 | 360 | 107 / 1246 | physics-engine | 275.9 | 400.0 | 536.1 | 271.1 | 11751.9 (5967.6) | 0.263 |
| vale-crowded-fights-64 | 360 | 139 / 1278 | rapier | 1050.5 | 1402.2 | 1648.2 | 1053.8 | 5227.3 (2590.1) |  |
| vale-crowded-fights-64 | 360 | 139 / 1278 | physics-engine | 336.9 | 448.6 | 552.7 | 317.8 | 11972.3 (5917.8) | 0.303 |
| vale-ogre-brawl | 360 | 78 / 1217 | rapier | 954.6 | 1276.6 | 1481.3 | 978.2 | 5362.8 (2433.1) |  |
| vale-ogre-brawl | 360 | 78 / 1217 | physics-engine | 146.7 | 275.3 | 404.6 | 177.4 | 11638.9 (5951.4) | 0.184 |
| vale-mill-hoist | 640 | 76 / 1215 | rapier | 992.7 | 1328.6 | 1711.9 | 999.0 | 5227.2 (2434.5) |  |
| vale-mill-hoist | 640 | 76 / 1215 | physics-engine | 190.6 | 265.8 | 394.1 | 177.9 | 12299.2 (5942.4) | 0.181 |
| vale-mill-ramps | 240 | 77 / 1216 | rapier | 839.5 | 1152.0 | 1585.3 | 887.6 | 5018.1 (2414.6) |  |
| vale-mill-ramps | 240 | 77 / 1216 | physics-engine | 115.7 | 236.3 | 402.0 | 151.9 | 11244.0 (5758.9) | 0.172 |
| vale-charge-path | 240 | 79 / 1218 | rapier | 775.4 | 1173.3 | 1601.7 | 834.2 | 7412.4 (2223.8) |  |
| vale-charge-path | 240 | 79 / 1218 | physics-engine | 112.3 | 237.3 | 372.1 | 149.1 | 16874.2 (5285.9) | 0.179 |
| vale-watch-post | 420 | 77 / 1216 | rapier | 939.2 | 1258.6 | 1727.5 | 947.9 | 7703.2 (2237.0) |  |
| vale-watch-post | 420 | 77 / 1216 | physics-engine | 136.3 | 235.4 | 359.2 | 157.5 | 16832.6 (5259.2) | 0.168 |
| vale-crate-shove | 180 | 77 / 1216 | rapier | 843.4 | 1128.6 | 1405.4 | 891.6 | 10428.0 (2443.2) |  |
| vale-crate-shove | 180 | 77 / 1216 | physics-engine | 112.5 | 238.2 | 407.4 | 152.6 | 24195.1 (5710.3) | 0.159 |
| vale-missiles | 300 | 79 / 1218 | rapier | 882.7 | 1233.8 | 1793.5 | 899.9 | 7266.3 (2160.4) |  |
| vale-missiles | 300 | 79 / 1218 | physics-engine | 190.2 | 257.1 | 408.1 | 170.3 | 17107.1 (5375.4) | 0.189 |

On this machine the engine arm's per-tick physics path costs 0.16–0.30× Rapier's
(paired-round medians; absolute times vary with machine load; the crowded fixture, where
characters overlap most, is the most expensive). Moving the mill hoist's box in place
(physics-engine #355) brought it back down from 0.15–0.37×; creature navigation (#307) adds a
shape cast per chasing creature and tick, plus A* overlap queries while one is blocked. The heavy crates' slides (#58) run on Rapier in both
arms and are excluded like debris; a resting crate is one more fixed box. Building the zone costs about 2.2× more
(heightfield preparation and per-body inserts). Note that the
Rapier arm rebuilds its world every step by design (determinism), which is part of its cost.
