# Physics backend A/B (physics-engine #330)

Production native and WASM builds compile only Physics Engine, including Greyhaven's
convex rock hulls, Redbrand's static triangle meshes, free debris and upright movable props.
Rapier is an optional independent behaviour/performance oracle behind the non-default
`physics-ab` feature (Atlas 2.1.i647). That feature alone compiles Rapier and the explicit
`World::with_backend`/`physics::using_backend` diagnostic selectors; production exposes no
backend selector or fallback. Hosts and clients continue to use the same core simulation.

Zone construction and recovery always use Physics Engine outside diagnostics. Greyhaven uses its
retained hulls and cave meshes for characters, support, ray/projectile and shape queries.
Free-rotating debris and upright movable props also contact those exact shapes. Their two
isolated response worlds retain static geometry and engine scratch, synchronize changed
static bodies, and transfer canonical pieces in and out each tick. Units enter only the prop
lane as temporary fixed capsules; the character world is separate. All temporary bodies are
removed after stepping, so no hidden dynamic continuation state survives canonical recovery.
`World::step_debris` and `World::step_props` take `&mut self`, making retained response
state ownership explicit while keeping the zone `Send + Sync`. The game does not consume Engine contact events; this is a physical/game-state continuation
claim, not a checkpoint event-stream claim (physics-engine #505).

Authored response uses two substeps, four velocity iterations for debris and one for upright
props, with sleeping disabled. Primitive-only debris preserves its existing eight-pass setup
(the four-pass primitive fixture froze a panel on an edge, #643). These paths do not enable
CCD. Unsupported future content still needs a demonstrated consumer requirement before its
capabilities can be adopted; authored geometry is never reduced to bounds.

The native and WASM A/B runners execute the same Rust assertions and embedded fixtures:
all 19 workloads, both backends, same-backend exact replay, exact canonical midpoint recovery,
behaviour invariants and identical-state gameplay-query parity. The WASM runner builds a
separate diagnostic `cdylib` example, then executes the actual `wasm32-unknown-unknown` module
through wasm-bindgen in Bun. It also executes the same authored Redbrand/Greyhaven geometry
checks as native. The production browser adapter exposes no backend selection API.
Supported characters settle their current support gap before horizontal movement, matching
the independent oracle. The support cast inflates the capsule by the contact offset so slopes
measure the margin along the surface normal. The authored towpath departure regression checks
both the grazing edge and the following landing in native and real WASM.
Native/browser GPU acceptance remains separate from these physics checks.

```sh
cargo test -p mmorpg-core --features physics-ab --test physics_ab --locked
cargo test -p mmorpg-core --features physics-ab --test authored_backend --test authored_collision --locked
bun scripts/physics-ab-wasm.ts
cargo run --release -p mmorpg-core --example physics_ab --features physics-ab --locked
```

The adoption pins Physics Engine `721a9298beca5096552215e14657056176e59a35`
(kernels `e6067536`). The focused 360-tick authored fall cases compare independent Parry
contact depth, require contact with the actual mesh/hull and settling on both backends, and
preserve exact Engine replay and canonical midpoint continuation. Free rotations produce
maximum pose differences of 26.60 units on the cave floor and 137.16 on the rock; upright
pieces differ by at most 1.80 units. The nonembedding contract is the upstream authored oracle's
0.5-unit allowance beyond Rapier's worst penetration, capped at five units. Existing 19-workload
gameplay bounds remain unchanged.

## Experimental persistent projectile CCD (shadow world, diagnostic only)

The opt-in `projectile_ccd_ab` example uses the production Physics Engine pin above, which
contains the CCD scheduler work first measured at `0d0a757e5122f2a25eccffcd6c7e55e8ef878507`:
chronological multi-impact fixes (#464/#468), spatially indexed moving sweeps and
heap-ordered events (#472), near-margin remaining-travel repair (#473), and
conservative non-closing pair pruning (#480). The prior experiment pinned
`1db80f40c3244cd1b62d951ed8b54cfdb283a876`; its [owner-provided 1–32-bomb
measurements](https://github.com/moenarch-games/mmorpg/pull/638#issuecomment-6642942809)
are retained as an **older-engine baseline**. Rerun on the same host before
claiming a performance improvement. No production or gameplay projectile path changes:

```sh
git rev-parse HEAD
rustc -Vv
lscpu
cargo test --release --locked -p mmorpg-core --features physics-ab --test thrown_weapons --test trick_shots
cargo run --release --locked -p mmorpg-core --features physics-ab --example projectile_ccd_ab
cargo run --release --locked -p mmorpg-core --features physics-ab --example projectile_ccd_ab -- --trace-staggered
```

The probe launches MMORPG's real level-six Rogue bomb with the wall and boars from
`tests/thrown_weapons.rs`, then runs an isolated persistent CCD sphere in **shadow**
at the same position and velocity. It checks the real fuse, splash/cover result,
first 60 Hz multi-impact ricochet and bit-exact replay on each arm. It reports
position divergence at the fuse, because the game uses a point sweep with one
bounce per tick and fixed per-bounce velocity scaling, while physical CCD uses a
finite radius, impulse restitution and Coulomb friction. A position difference is
not automatically an engine defect.

The example also measures 160 complete game-zone ticks with the same number of
game projectiles on each arm, **plus** optional shadow dynamic bodies. Separate
shadow-step times and distinct candidate-pair, narrow-phase, exact sweep-query,
admitted-impact and fallback counts distinguish CCD
work from ordinary dynamic integration. Its 1/8/16/24/32-body matrix includes
a 32-body staggered case; cloned simultaneous throws are an intentionally
synthetic worst case, not proof of gameplay-legal rapid casts. Timings are
informational on a shared machine, not CI gates; setup is excluded. The shadow
world reproduces the arena wall and floor, not the full zone geometry, unit
contacts or live movement. Thus measured additional CPU is **not** a prediction
of the finished adapter's total cost.

`--trace-staggered` is an untimed diagnostic for the 32-bomb case. It steps the
full staggered shadow world alongside 32 isolated single-bomb worlds at identical
lane offsets, printing admitted impacts and exact sweeps by tick, isolated
impact/sweep totals, contact-start counts, and each lane's first bitwise
shared-versus-isolated pose/velocity divergence **and maximum absolute drift**.
This helps distinguish genuine
cross-bomb interactions from unrelated scheduler work before attributing changes
in aggregate impact counts to a CCD regression. Contact starts are **pair-ordered
transitions**, not chronological impacts, and shared admitted-impact counts do
not identify the exact projectile without a chronological impact-report API.

The missing integration contracts are stable projectile-ID ↔ engine-body mapping,
resynchronizing world geometry and moving units, exact projectile pose/velocity
and any rotational/contact state needed on restore, applying gameplay collision
filters, and chronological impact data (TOI/position/normal) for game-owned
damage, shields, bounce limits, fuse and lifetime. The current
`Report::contact_events` contains pair-ordered contact transitions, **not** a
chronological impact stream. No CCD collision determines an MMORPG gameplay
outcome in this experiment; the game's swept-point projectile rules remain authoritative.

## Consumer contact workload diagnostic (physics-engine #443)

The existing manual A/B example now reports **complete 60 Hz zone ticks**, the portion spent
in the physics wrapper, and successful public body removals/replacements, scheduled platform move
requests (including the subset that actually changes position), platform placements and explicit
moving-body placements. It also samples Rapier's narrow-phase
body-pair graph *after* each tick: retained candidate pairs (including non-touching ones),
active pairs, maximum incident degree, and collision callbacks delivered to the kinematic
character controller. These are separate metrics. The latter are swept collisions, not cached
contact pairs; body pairs deduplicate Rapier's multiple collider/terrain-tile contacts.

```sh
cargo run --release --locked -p mmorpg-core --example physics_ab --features physics-ab -- --only vale-idle-16 --rounds 3 --warmup 1
cargo run --release --locked -p mmorpg-core --example physics_ab --features physics-ab -- --only vale-mill-hoist --rounds 3 --warmup 1
cargo run --release --locked -p mmorpg-core --example physics_ab --features physics-ab -- --only vale-watch-post --rounds 3 --warmup 1
```

`--only` filters the existing deterministic named workloads (comma-separated substrings select
several); no new fixture or production
instrumentation is involved. Each timed sample includes the driver's commands and the complete
`ZoneSimulation::advance_tick`, with setup and post-tick contact inspection excluded. Post-tick
inspection can still affect later CPU cache state, and timing on a shared runner is diagnostic,
not a regression gate. Per-run operation call counts are checked to be identical across rounds.
The `physics-ab` Cargo feature is absent from normal production and WASM builds; the engine
itself is compiled into both.

The Physics Engine adapter uses `World::move_characters` for characters, not its persistent
`World::step`, and uses isolated response worlds for debris/props. The pinned engine
dependency does not expose per-body persistent cache occupancy. Consequently the table marks
Rapier contact topology as unavailable for that backend: **Rapier retained pairs are not a
measurement of Physics Engine cached contact pairs**, and a zero must not be invented for a
future adapter that changes its stepping pattern. Static door replacements are reported
separately from explicit removals. Scheduled platform moves that change a collider's position
typically become backend `set_static_position` calls during its step; those internal costs remain
included in step time, not in the separate platform-scheduling timer.

### Observed consumer workload (2026-11-02)

On MMORPG `c45a546564835b94e7998bc8e4ed82ada1c4a9d0` plus this diagnostic change,
locked Physics Engine `7af1ac43` and Rapier 0.36.1, Rust 1.99.1, native Linux x86_64
(Intel Xeon Platinum 8573C), `--release`, **one warmup and three measured rounds** for
each backend, alternating first backend per round:

| Named Greyhaven workload | 60 Hz ticks | Max moving / all bodies | Rapier pairs (max) | Controller collisions/tick (mean) | Actual platform changes / scheduled calls | Explicit removals / replacements | Complete tick median, Rapier / Engine (µs) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `vale-idle-16` | 360 | 97 / 2,498 | 0 | 0.50 | 60 / 360 | 0 / 0 | 3,043 / 1,081 |
| `vale-crowded-fights-64` | 360 | 145 / 2,546 | 0 | 0.80 | 60 / 360 | 2 / 0 | 5,419 / 1,657 |
| `vale-mill-hoist` | 1,280 | 82 / 2,483 | 0 | 4.88 | 640 / 1,280 | 0 / 0 | 3,144 / 911 |
| `vale-watch-post` | 840 | 83 / 2,484 | 0 | 3.45 | 320 / 840 | 0 / 0 | 2,903 / 870 |
| `vale-crate-smash` | 720 | 83 / 2,484 | 0 | 2.47 | 320 / 720 | 2 / 0 | 2,966 / 1,106 |

The **Rapier pair count, active pair count, and per-body degree were all zero on every
observed tick**, even though kinematic sweeps found collisions and the games scheduled
static movement. There is no retained *Rapier* contact graph on these real game adapters;
the Physics Engine A/B adapter cannot supply a separate persistent-cache occupancy metric
without changing its pinned dependency, and its character workload never invokes the
cache-populating `World::step`. These measurements therefore do **not** demonstrate a
game-consumer need for Physics Engine's proposed adjacency index (#443).

The Rapier physics-call share of complete tick time ranged from 90.5% to 94.1% on these
five release fixtures. Physics Engine's share ranged from 68.0% to 96.4%; the
`vale-crate-smash` Engine p95 complete tick was 41.95 ms versus Rapier's 7.71 ms, because the
test backend has separate short-lived dynamic debris stepping. The speed difference is not
an index A/B comparison. This is a shared-CPU exploratory run, not a controlled performance
contract or a runtime server trace. No static door replacements occurred in these selected
fixtures; the existing door path remains instrumented for a future replay that exercises it.

**Decision:** leave Physics Engine's scanning production path and its unmerged experimental
[contact-adjacency PR #454](https://github.com/moritzbrantner/physics-engine/pull/454)
unchanged. Only revisit index maintenance when a real adapter starts retaining persistent
contacts and uses the neighbor-local operations often enough to offset maintenance cost.

## Seam

- `physics::World` is an enum over the backends (`crates/mmorpg-core/src/physics.rs`). Rapier's
  world moved unchanged to `physics/rapier.rs`; `physics/engine.rs` is the thin engine adapter.
  No call site changed.
- Both backend arms compile normally. The non-default `physics-ab` feature adds diagnostic
  probes and enables `physics::using_backend(backend, ..)` to override the content selection
  for an A/B run and its recoveries. `World::with_backend` selects an explicit standalone world.
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
overlaps (`overlap_shape`) and segment checks, immutable triangle meshes and convex
rock hulls. Debris (#288) uses Engine in primitive-only worlds (two substeps, eight
velocity-solver passes) and Rapier's existing dynamic path in authored mesh/hull worlds.
Movable props (#58) use engine dynamic boxes in primitive-only worlds and Rapier
when authored meshes or hulls are present. A resting crate is fixed on both arms;
the existing friction, restitution and averaging contracts are retained.


## What the test asserts

`crates/mmorpg-core/tests/physics_ab.rs` replays the bot scenarios `two-bots-move`,
`vale-hub-walk` and `vale-wolf-hunt` (command by command, as the bot runner submits them; the
production Engine arm reproduces each scenario's expected positions exactly) and the four Greyhaven combat
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
same climb and mantle queries) and `vale-roll` (#516: one player in Greyhaven Outpost rolls
back, forward with its keys released mid-roll and diagonally; another rolls into a highland crag
and back out; a third rolls back off the mill's loading dock and falls to the yard mid-roll)
and `vale-swim` (#537: one player runs and jumps off Stillwater Lake's west bank, swims out,
dives to the basin's floor and rises; another presses down onto the floor in the deep water and
rises; both arms open the floor half-space over the basin, the engine's through
`CharacterMove::exclude`) through the real zone simulation, from the same initial state with the
same inputs and tick count.

No workload places a unit inside geometry (#534, physics-engine #404): squat creatures' capsules
fit their boxes (`physics::capsule_dimensions`), creatures spawn with their capsule's round
bottom on the hillside (`ZoneSimulation::stand_y`), the fighting fixtures put each player on the
first free slot around its spawn (a capsule probe clear of every box, prop, door leaf, creature
and earlier player) and spirits released together stand apart around the graveyard.

Both arms carry the mill hoist's riders in the adapter the same way: a downward support cast
filtered to the platform finds them, its displacement is added to their move, and a sinking
platform moves before the units, a rising one after them. `vale-mill-hoist` is held to the
scripted-player tolerances below.

On **each** backend, every run must:

- keep every player and creature state finite;
- keep unit capsules out of content boxes, door leaves and standing props, and above the floor
  and terrain, beyond 0.01 units (both backends keep a 0.25-unit gap; `f32` resolves ~10⁻³ units
  at the zone's 10⁴-unit magnitudes), from the first tick: nothing is authored inside geometry;
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
- in `vale-roll`, let every player roll, never let the crag stop its roller into a climb, and
  land the dock's roller on the ground beyond the deck's edge;
- in `vale-mill-ramps`, carry the walker up the cart ramp onto the dock (feet 1.5 m up within 3
  units) and back to the ground, and never let the chute's player onto the dock.

Across backends:

- Scripted workloads (`two-bots-move`, `vale-hub-walk`, `vale-idle-16`, the fighting fixtures
  `vale-fights-8`, `vale-fights-32` and `vale-crowded-fights-64`, `vale-swim`, `vale-mill-hoist`,
  `vale-crate-shove`, `vale-crate-smash`, `vale-missiles`, `vale-traversal`, `vale-climb` and
  `vale-roll`): players within **1 unit** (the rounding step gameplay sees; in
  `vale-crate-shove` plus the crates' gap, since a player walks into its resting crate),
  identical command outcomes and door states, and the players' grounded state identical on
  every tick.
- Every query decision on **identical states** (Rapier's state every 2 s, recovered onto
  both backends): line of sight, shots, charge paths (a capsule `cast_shape`, #57) and creature routes
  (#307: shape casts and overlap queries on the navigation grid) for player–creature pairs
  within 40 m, NPC sight,
  open and closed door-leaf occupancy, ground height and grounded must agree exactly; shot
  fractions within 10⁻⁴. Grounded is compared for every unit, over the lake basin's open floor
  too.
- Restored from its canonical snapshot halfway, each arm continues exactly (Atlas 2.1.5).

Creatures (AI and a shared RNG react to every position) and `vale-ogre-brawl`'s players are
compared but not bounded; the test prints their divergences with `--nocapture`.

## Divergences

Measured at the commit the timing table names:

| Workload | Players (max Δxz / Δy) | Creatures at the end | Engine restored halfway |
|---|---|---|---|
| two-bots-move | 0 / 0 | 0 of 75 apart (max 0.03) | continues |
| vale-hub-walk | 0.01 / 0 | 1 apart (max 3.2) | continues |
| vale-wolf-hunt | 9.2 / 1.5 (climb allowance, 8.97 of 13.93 to tick 1,434) | 80 apart (max 798) | continues |
| vale-idle-16 | 0 / 0 | 0 apart (max 0.15) | continues |
| vale-fights-8 | 0 / 0.03 | 2 apart (max 3.6) | continues |
| vale-fights-32 | 0.60 / 0.89 | 10 apart (max 10.9) | continues |
| vale-crowded-fights-64 | 0.01 / 0.05 | 0 apart (max 0.09) | continues |
| vale-ogre-brawl | 73.9 / 1.2 (from tick 53) | 3 apart (max 31.7) | continues |
| vale-mill-hoist | 0.001 / 0.07 | 59 apart (max 810) | continues |
| vale-mill-ramps | 0.67 / 1.1 | 0 apart (max 0.72) | continues |
| vale-charge-path | 0 / 0.03 | 0 apart (max 0.72) | continues |
| vale-watch-post | 0 / 0.01 | 44 apart (max 907) | continues |
| vale-crate-shove | 3.0 / 0.22 (player 2 stops at its crate) | 2 apart (max 2.5); crates 2.5 short of Rapier's slide | continues |
| vale-crate-smash | 0 / 0 | 4 apart (max 4.0); settled debris within 37.1 | continues |
| vale-missiles | 0 / 0.03 | 4 apart (max 6.1); same missiles strike, world hits within 0.001 | continues |
| vale-traversal | 0.02 / 0.01 | 0 apart (max 0.72) | continues |
| vale-climb | 0.41 / 0.85 | 37 apart (max 810) | continues |
| vale-roll | 0.14 / 0.63 | 0 apart (max 0.57) | continues |
| vale-swim | 0.12 / 0.27 | 2 apart (max 3.9) | continues |

Every query decision on identical states agreed (up to 14,306 probes per workload).

Intentional or explained differences:

1. **Touching characters.** Both arms turn a move that presses into a touching moving capsule
   with the Rapier arm's `slide_around` (within 60° of head-on both step aside at full speed,
   plus a 1-unit outward nudge), the engine arm before its batch (#534). Rapier moves bodies one
   by one, so a later mover sees an earlier one already moved; the engine arm turns every move
   from the positions before the step. Characters do not push each other on either: a large
   creature's shove (#297) is a gameplay forced move decided before the step, the same on both.
   In `vale-ogre-brawl` the ogre and the players around it move in the same ticks, so the player
   walking into its side slides around it differently and players part by up to 74 units;
   every shove and stomp still happens on both.
2. **Grounded.** Both report static ground within the contact offset plus 5 cm (Rapier's ground
   prediction; physics-engine #348; the engine also requires it walkable). A body placed since
   its last move (a recovered state, a spawn) is answered on the engine arm by the Rapier arm's
   own support cast: walkable ground within offset + 5 cm beyond the contact offset (#534; it
   had stopped the offset short, so a fight fixture's player placed 5.3 units over a hillside
   was grounded on Rapier only). On identical states they agree for every unit.
3. **Creature drift.** Creatures wander and chase on a zone RNG they share. Over 5,124 ticks
   (`vale-wolf-hunt`) the small differences above move AI decisions and the RNG stream, so most
   creatures end elsewhere; the scripted hunter's path, fights and command outcomes stay
   identical.
4. **Terrain.** Both arms remove flat heightfield cells and leave them to the floor
   half-space (parry heightfield contacts let resting bodies sink; the engine arm passes the
   same cells as `HeightfieldDesc::removed_cells`); flat cells next to Stillwater Lake's basin
   stay, since the floor opens under a body straddling its edge. Engine overlap queries also
   see the terrain (Rapier skips box–heightfield pairs); door probes only look for units, so
   occupancy agrees.
5. **Contact gap.** Rapier units came as close as 0.22 units to a box (`vale-climb`), engine
   units 0.2496, never inside.
6. **Rapier on the heightfield.** Rapier's controller catches on heightfield cell seams: a
   charging player in `vale-fights-32` rises 0.9 units for a tick on flat terrain, and a diver
   gliding along the lake's floor stalls for a tick now and then. Its ground snap also snaps a
   walker stepping off a too-steep bank 2 units down onto the bank's face. `vale-swim`'s wader
   therefore leaves the bank in a jump and nobody glides along the floor.
7. **Exact ties.** Content is authored on round coordinates; a sight line run exactly along a
   box face is a tie each arm breaks its own way (Rapier in `f32` metres). The fighting fixtures
   stand their players 7 units off their spawns' rows.

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
6. **Embedded starts** (closed by content, physics-engine #404 / #407). Rapier's controller
   leaves a capsule that starts inside geometry where it is; physics-engine pushes it out by
   default (`Embedded::PushOut`). The owner chose to stop authoring units inside geometry
   rather than use `Embedded::Stay` (#534), and the A/B now bounds the fighting fixtures.
7. **Open floor over the lake basin** (fixed, physics-engine #405 / mmorpg#537). The engine arm
   kept the floor half-space under Stillwater Lake, so its swimmers floated on y = 0 while
   Rapier's dove. `CharacterMove::exclude` now opens it for a body over the basin, in its move
   and its support cast, with the Rapier arm's footprint test, and `vale-swim` covers it.

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
the dock's edge 2.5 units high for a tick before settling, so `vale-mill-ramps` allows 3 units. Free-aim swings (#517) moved `vale-wolf-hunt`'s zone RNG: with the hunter swinging from tick 1,514, a young boar stopped exactly on an integer ground-probe seam (564 or 565 at (−18 105, 8 190)), where the arms' heights round one unit apart; the hunter starts swinging at 1,544 instead, and the exact ground comparison is unchanged.

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

See the table below, produced by the example in release on the commit it names (this branch,
#534/#537, before its last commit). A sample is one tick's whole physics path (the step, every
gameplay query and body update the tick makes, as `World::spent_nanos` counts them, debris and
props included); setup is building the zone. Timing is evidence for physics-engine #330, not a
gate.

- mmorpg: 2c4604f1903a3b3ba986a779937e4194dbad231e plus the #534/#537 changes
- physics-engine: version = "0.1.0" source = "git+https://github.com/moritzbrantner/physics-engine?rev=7af1ac43cf63437ba61e543aa4188b36673d5b9f#7af1ac43cf63437ba61e543aa4188b36673d5b9f"
- rapier3d: version = "0.36.1" source = "registry+https://github.com/rust-lang/crates.io-index"
- rustc: rustc 1.99.0 (b940084d7 2026-09-28)
- profile: release; 2 warmup + 7 measured rounds per backend, alternating the first backend each round
- sample: one tick's whole physics path (step, gameplay queries, body updates, debris and props); microseconds; setup is building the zone, its physics share in parentheses

| workload | ticks | moving / bodies | backend | median | p95 | p99 | mean | setup (physics) | engine / rapier, median of paired rounds |
|---|---:|---:|---|---:|---:|---:|---:|---:|---:|
| two-bots-move | 20 | 83 / 2119 | rapier | 2052.9 | 2865.4 | 3274.0 | 2171.3 | 8505.4 (8211.0) |  |
| two-bots-move | 20 | 83 / 2119 | physics-engine | 306.4 | 943.9 | 1289.7 | 369.1 | 23903.1 (23609.5) | 0.167 |
| vale-hub-walk | 560 | 83 / 2119 | rapier | 3684.2 | 8943.3 | 11986.5 | 4141.1 | 12580.3 (12202.9) |  |
| vale-hub-walk | 560 | 83 / 2119 | physics-engine | 588.6 | 1222.8 | 3811.4 | 784.2 | 34339.0 (33872.8) | 0.209 |
| vale-wolf-hunt | 5124 | 82 / 2118 | rapier | 2945.8 | 5293.4 | 9996.4 | 3413.9 | 7555.4 (7280.5) |  |
| vale-wolf-hunt | 5124 | 82 / 2118 | physics-engine | 545.6 | 959.1 | 1101.3 | 526.6 | 22289.0 (22006.9) | 0.164 |
| vale-idle-16 | 360 | 97 / 2133 | rapier | 3633.4 | 12160.0 | 20875.4 | 4789.8 | 24464.0 (10151.8) |  |
| vale-idle-16 | 360 | 97 / 2133 | physics-engine | 528.4 | 1130.7 | 1497.6 | 624.4 | 68144.2 (30979.5) | 0.172 |
| vale-fights-8 | 360 | 89 / 2125 | rapier | 3189.2 | 5039.6 | 7322.8 | 3448.4 | 18460.2 (7652.5) |  |
| vale-fights-8 | 360 | 89 / 2125 | physics-engine | 524.8 | 998.8 | 1091.0 | 559.2 | 55405.2 (24981.5) | 0.158 |
| vale-fights-32 | 360 | 113 / 2149 | rapier | 3312.2 | 5202.2 | 6255.9 | 3582.5 | 17583.3 (6221.1) |  |
| vale-fights-32 | 360 | 113 / 2149 | physics-engine | 640.6 | 1131.8 | 1405.2 | 679.8 | 52236.0 (23901.9) | 0.190 |
| vale-crowded-fights-64 | 360 | 145 / 2181 | rapier | 3205.7 | 4504.5 | 5501.8 | 3364.0 | 57205.9 (5430.2) |  |
| vale-crowded-fights-64 | 360 | 145 / 2181 | physics-engine | 516.1 | 1153.9 | 1781.2 | 674.3 | 96806.5 (21587.4) | 0.211 |
| vale-ogre-brawl | 720 | 84 / 2120 | rapier | 2312.0 | 2777.3 | 3473.9 | 2324.3 | 12427.8 (4766.2) |  |
| vale-ogre-brawl | 720 | 84 / 2120 | physics-engine | 344.2 | 627.5 | 881.6 | 440.2 | 44001.1 (21017.7) | 0.189 |
| vale-mill-hoist | 1280 | 82 / 2118 | rapier | 2472.3 | 2978.5 | 3691.6 | 2452.7 | 12393.5 (4716.1) |  |
| vale-mill-hoist | 1280 | 82 / 2118 | physics-engine | 477.2 | 657.6 | 920.1 | 457.0 | 45354.9 (21026.9) | 0.186 |
| vale-mill-ramps | 480 | 83 / 2119 | rapier | 2043.0 | 2624.1 | 2818.2 | 2189.9 | 12071.0 (4593.3) |  |
| vale-mill-ramps | 480 | 83 / 2119 | physics-engine | 292.9 | 599.9 | 616.5 | 399.2 | 42600.8 (20524.2) | 0.184 |
| vale-charge-path | 480 | 85 / 2121 | rapier | 1918.3 | 2621.6 | 2687.8 | 2122.3 | 16572.2 (4493.5) |  |
| vale-charge-path | 480 | 85 / 2121 | physics-engine | 282.0 | 615.1 | 632.4 | 397.6 | 61477.7 (19068.3) | 0.187 |
| vale-watch-post | 840 | 83 / 2119 | rapier | 2367.6 | 2722.5 | 3145.6 | 2316.1 | 17964.1 (4733.2) |  |
| vale-watch-post | 840 | 83 / 2119 | physics-engine | 349.4 | 595.1 | 621.6 | 415.3 | 62745.9 (19314.3) | 0.180 |
| vale-crate-shove | 360 | 83 / 2119 | rapier | 1959.0 | 2640.1 | 3017.4 | 2094.2 | 22154.3 (4545.6) |  |
| vale-crate-shove | 360 | 83 / 2119 | physics-engine | 283.1 | 10984.0 | 11410.6 | 1931.4 | 83696.7 (19111.6) | 0.922 |
| vale-crate-smash | 720 | 83 / 2119 | rapier | 2306.0 | 2628.8 | 2990.1 | 2278.3 | 17479.0 (4678.9) |  |
| vale-crate-smash | 720 | 83 / 2119 | physics-engine | 558.9 | 10779.1 | 11299.5 | 2626.9 | 63309.8 (19528.4) | 1.152 |
| vale-missiles | 600 | 85 / 2121 | rapier | 2320.3 | 2870.9 | 3618.4 | 2321.0 | 17780.4 (4659.4) |  |
| vale-missiles | 600 | 85 / 2121 | physics-engine | 564.6 | 669.4 | 890.8 | 463.4 | 63681.3 (19325.3) | 0.200 |
| vale-traversal | 480 | 83 / 2119 | rapier | 2017.1 | 2742.8 | 3434.2 | 2193.1 | 13786.1 (5138.9) |  |
| vale-traversal | 480 | 83 / 2119 | physics-engine | 300.8 | 616.6 | 893.4 | 408.3 | 44436.3 (20993.7) | 0.182 |
| vale-climb | 1040 | 83 / 2119 | rapier | 2426.8 | 2899.3 | 3570.2 | 2387.9 | 12409.5 (4680.8) |  |
| vale-climb | 1040 | 83 / 2119 | physics-engine | 387.7 | 622.0 | 795.0 | 446.3 | 43779.7 (20914.0) | 0.188 |
| vale-roll | 400 | 84 / 2120 | rapier | 1928.6 | 2574.3 | 2632.3 | 2043.5 | 11924.5 (4544.2) |  |
| vale-roll | 400 | 84 / 2120 | physics-engine | 280.6 | 599.8 | 617.1 | 355.6 | 43045.2 (20658.3) | 0.174 |
| vale-swim | 720 | 83 / 2119 | rapier | 2341.0 | 2727.9 | 2957.2 | 2326.2 | 12018.6 (4561.0) |  |
| vale-swim | 720 | 83 / 2119 | physics-engine | 366.8 | 650.6 | 909.5 | 464.1 | 43454.9 (20758.8) | 0.197 |

On this machine the engine arm's per-tick physics path costs 0.16–0.21× Rapier's (paired-round
medians; absolute times vary with machine load, and another build shared the machine during the
first rows). `vale-crate-shove` and `vale-crate-smash` are the exceptions at 0.92× and 1.15×:
their p95 of about 11 ms is the engine's dynamic-body world for the sliding crates and flying
panels, rebuilt each tick, which the Rapier arm does in about a tenth of the time. Building the
zone costs about 3× more (heightfield preparation and per-body inserts). Note that the Rapier
arm rebuilds its world every step by design (determinism), which is part of its cost. Since
mmorpg#547 (after this table) a step copies a cached world of the fixed bodies and the ground and
inserts only the units and platforms, which roughly halves the Rapier per-tick medians above.

### Tangential terrain seam recovery (#644)

The combined authored scene exposed Redbrand bandit 401 ending slightly embedded at a
flat-to-rising heightfield seam. A nearly tangential earliest hit hid the next blocking
triangle; reconstruction alone then depenetrated it at tick 2,563. Physics Engine #501
recasts that tangential contact with its existing normal nudge. The exact single-move
regression requires no endpoint penetration and identical reconstructed continuation.

With engine `943d0841a6b4abb5048c042965620b35a7225f0a` (kernels `291ae802`), the wolf A/B
passes exact recovery/replay, all 9,563 physical-query comparisons, and the unchanged
scripted climb allowance (12.98 units of 17.04). This is correctness evidence, not timing.
The upstream debugging checkpoint algorithm changes to 12; MMORPG's canonical/wire
layout and protocol ID remain unchanged.

### Authored-scene integration checks (#644)

Terrain probes now span the queried cell's four sample heights rather than 20,200 units.
This avoids cancellation at the charge/missile fixture's half-unit rounding tie without
changing query parity bounds. Rapier's adapter checks the actual hit triangle before
interpreting a tiny vertical-mesh side scrape as a ceiling; real undersides and slanted
Redbrand faces still stop ascent. The raw controller's returned translation is unchanged.

The eastern cottages now intersect the old flat-ground air-control fixture. That fixture
runs the same commands and assertions on the clear spawn plaza; actual authored side,
underside, slanted-wall and cave traversal/recovery checks remain separate. The former
cottage trajectory differed by 1.94 units horizontally and 0.27 vertically after the adapter
repair; this is not a claim of universal one-unit trajectory parity on meshes.

The watch-post chase retains its original commands and unbounded AI-position contract.
Its first blast centres are [817, 1222, -7538] on Rapier and [819, 1219, -7545] on Engine:
an eight-unit difference changes piece 3's exact distance falloff by one health point.
Each arm now checks exact expected damage from its own impact centre and the same damaged
piece IDs. A separate fixed world impact requires identical piece health across backends.

The brook mill replaces the old western gate with a raised towpath and a western
stone apron. The wolf route now follows that apron instead of stepping straight
off the deck into the lower bank. The detour adds 58 ticks; the scripted climb
comparison covers the corresponding 1,492 ticks with its original tolerances.
The same wolf kill, death and spirit-return expectations remain in place.
