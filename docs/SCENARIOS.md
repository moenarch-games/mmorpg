# Headless scenarios

> **Interactive review scenarios (#371)** are a separate, manual tool: open the browser game with `?scenario=ogre-combat&seed=7` (local: `cd web && bun run dev`, then `http://localhost:5173/mmorpg/?scenario=ogre-combat&seed=7`; or the same query on GitHub Pages). It skips character selection and drops a level 5 Warden 15 m from the Southmeadow Hill Ogre in a fresh local world; **Reset scenario** or F9 restarts it from the same seed, and nothing is saved. Scenarios are defined in `crates/mmorpg-wasm/src/scenario.rs`; the badge in the top bar names scenario, seed and content revision for bug reports.
>
> **Action Lab (#381)** is the scenario `?scenario=action-lab&class=warden&level=6&gear=standard&resource=60` (optional `weapon=<main-hand item id>`, `seed=<n>`): a configured ephemeral character of any implemented class at the Wolfrun Woods pack, with a dock showing the statechart, a tick timeline and an inspector read from `mmorpg-core`'s `action_inspection`/`arbitrate_input`. F7 pauses, F8 steps one tick, the slider/tick field/timeline scrub the last 30 s of recorded ticks; presets and *Apply & reset* restart it with another validated setup (`crates/mmorpg-wasm/src/action_lab.rs`, `web/src/world/action-lab.ts`).
>
> **Experiments (#382)** in the same dock: the arena records every command since its fresh start with the tick it was applied at. *Save recording* keeps it as a named scenario (setup, seed, content revision, protocol ID, command-wire bytes, the rules' identity and the final canonical digest), whose commands can be moved, copied or removed tick by tick. Two configurations A and B are the core `ActionGraph` as JSON (`actionLabBaseline()`), edited field by field (edge windows, priority, conditions, effects and their magnitudes, precision windows, window modifiers, class rule and recovery overrides) and validated by `ActionGraph::validate`; their identity is a hash of the rules. *Replay A/B* replays the scenario in the arena; *Compare* runs it headless under both through the ordinary local host and shows outcome totals, input-to-effect delays, refusals, the first tick whose canonical snapshot differs and the differing ticks. Experiments run only in fresh, unsaved lab worlds (`ZoneSimulation::set_action_graph`); scenarios and configurations are saved in this browser or exported/imported as JSON, and never change production rules (`crates/mmorpg-wasm/src/lab_experiment.rs`, `web/src/world/lab-experiment*.ts`).
>
> **Traversal and momentum (#413)**: the movement region (airborne, then landing recovery) runs for every class, and the dock shows the running movement, where it comes from and the body's speed. A configuration can author **movement per action phase** (steering %, kept momentum %, an acceleration limit in cm/s²; Rapier still resolves every contact), add **landing edges** (`LandingToMelee`, `LandingToShieldBash`, `LandingToAbility`, anchored at touchdown, ending the landing recovery) and **impulse** effects. *Traversal sample* sets momentum through the air, slowed landings and a Warden's aerial hit landing into an early Heroic Strike or Shield Bash; compare it with production on a recorded run → jump → aerial attack → landing → follow-up. Replays and comparisons report each tick's position and speed.
>
> **Diagnostics and promotion (#383).** Every replay also reports explained findings read off the authoritative ticks: transitions never eligible in the scenario (or not exercised), buffered inputs lost before their edge opened, input-to-action lag above 100 ms, usable candidates outranked in arbitration, cast-cancel loops, and (A→B) combos that succeeded under A and fail under B. Each finding traces the active region state, pending commands, the edge's status and eligible window, its anchor tick's events, the observed arbitration and the first diverging tick; they are diagnostics, never a ranking. *Save version* keeps validated rules as the next numbered version of their name. *Promote* requires explicitly choosing a saved version, shows its rule changes against production and exports a promotion request with the current scenario as evidence. In a checkout, `cargo run -p mmorpg-wasm --locked --example action-lab -- promote <file>` refuses a request made against other production rules, replays the evidence under production and the candidate, regenerates `crates/mmorpg-core/src/action/baseline.rs`, writes a record (previous and promoted rules plus scenario) under `crates/mmorpg-wasm/promotions/` and a PR description under `target/action-lab/`, and prints the branch/commit/`gh pr create` steps. It never commits or pushes; production rules change only when that PR merges, and importing a record into the lab compares (or rolls back to) the previous rules (`crates/mmorpg-wasm/src/lab_diagnostics.rs`, `lab_promotion.rs`, `examples/action-lab.rs`).
>
> **Swimming (#413)**: a living player in deep water runs the `Swim` action in the movement region. A configuration can author its movement and rules that **refuse** an attack in water (the zone answers *You can't do that while swimming*) or **hold** it until the swimmer is ashore; the inspector shows the rule and "while swimming" instead of a release tick. *Swimming sample* refuses melee and Shield Bash, holds other abilities and makes the swim glide (full momentum, 4 m/s²); swim into Stillwater Lake, east of the lab spot, to try it. Production authors no water rule.
>
> **Authored actions (#414)**: a configuration can compose **new experimental actions** (`authored`) from existing primitives: a class, a trigger (Warden melee, Shield Bash or another class ability, optionally one ability) in a context (`Direct`, `Airborne` for jump + attack, `Moving(Any|Forward|Backward|Sideways)` for direction + action; more specific wins), timed startup/active/recovery phases with bounded movement, effects on the use (impulse, recovery, refund, waived global cooldown) and outgoing `Authored(n)` edges anchored at the use's impact. The startup holds the input; the active phase resolves it through its ordinary rules (eligibility, cost, cooldowns, hit); other offense inputs wait until the action ends unless an edge out of it is open. Validation names the unsupported primitive or conflict (`BadAuthored(slot, Input)`, `ConflictingTrigger`, `OutlastsSource`, …), and the dock lists each authored action with its trigger and why the current state accepts or refuses it. *+ Sliding slash* adds a sample: forward + Heroic Strike winds up 100 ms, slides 300 ms on its momentum with a forward launch, recovers 200 ms and may follow into Shield Bash. Authored actions ride along in saved versions and promotion requests; production loads one only after a promoted, reviewed PR regenerates `baseline.rs` (`AUTHORED_ACTIONS`).
>
> **Held, chorded and timed inputs (#414)**: a trigger has a `gesture` (absent for the press): `Charge` (hold to charge, release after `minMs` to fire with the action's effects, a full startup fires by itself, an earlier release is the plain use, a retired hold never acts) or `Chord` (pressed while `Guard` or another `Input(..)` is held). It may also be a timed follow-up (`follows`) of another authored action: it counts only `openAfterMs`…+`actionableMs` after that action's use, a press up to `bufferMs` before the opening waits (reaching back into the source's startup), an earlier press loses the follow-up, and a late input keeps the #415 grace. The browser sends each ability slot's release through `input-bindings` (`ReleaseAbility`); the zone reads holds in arrival ticks. *+ Charged bash* adds a sample: hold Shield Bash 200 ms and release to lunge, or after *+ Sliding slash*, the slash's buffered follow-up.

`mmorpg-scenarios` runs scripted multiplayer and control-plane scenarios without a GPU, network, wall clock or randomness. Use it to check server-side behavior that `scripts/smoke-native.py` can only reach through a GPU window.

```sh
cargo run -p mmorpg-scenarios --locked -- bots crates/mmorpg-scenarios/scenarios/bots/*.toml
cargo run -p mmorpg-scenarios --locked -- control-plane --json crates/mmorpg-scenarios/scenarios/control-plane/*.toml
```

Output is one digest line per event, ending with `PASS <name> …` or `FAIL <name> …` per file. `--json` prints the same lines as compact JSON objects with sorted keys, a `schema` (`mmorpg.bot-scenario/v1` or `mmorpg.control-plane-scenario/v1`) and a `type` (`scenario`, `step`, `tick`, `expect`, `error`, `result`). Exit status is 0 when every scenario passes, 1 when any fails and 2 for usage, file or format errors.

The runners own no rules. They drive the existing authorities and compare what those decide with the scenario's expectations.

| Runner | Drives | Checks |
| --- | --- | --- |
| `bots` | `build_zone_host` → `game-server` `MatchHost`/`MatchRuntime` → `ZoneGameServerAdapter` → `mmorpg-core` physics, interest, combat and creature AI | Step outcomes and decoded player-scoped snapshots |
| `control-plane` | `HostRegistry`, `ZoneDirectory`, `HandoffRegistry`, and `FencedZoneRuntime` for writes | Step outcomes and distributed-world invariants after every step |

## Bot scenarios

A bot scenario advances one zone hosted with the shared Greyhaven Vale content, including its creatures and NPCs; bots spawn on the hub plaza (slot 0 at (−1550, 1250), then 100 units apart along +X). Ticks are stepped explicitly with `MatchRuntime::advance_tick`. Each bot's snapshot bytes come from `MatchRuntime::snapshot_for` and are decoded with `mmorpg_protocol::decode_snapshot`; a snapshot for another zone, tick or viewer is an error. Reconnect tokens are deterministic per bot.

```toml
name = "two-bots-move"      # [A-Za-z0-9_-]
zone = 1
ticks = 10                  # observations exist for ticks 1..=10
reconnect_grace_ticks = 120 # optional, default 120
digest_interval = 1         # optional; eventful ticks always print

[[bots]]
name = "alice"

[[steps]]                   # applied at `tick`, before it advances to tick + 1
tick = 0
bot = "alice"
action = "join"             # join | move | jump | select_target | start_attack | stop_attack | release_spirit | move_item | loot | guard | disconnect | reconnect

[[steps]]
tick = 0
bot = "alice"
action = "move"
forward = 1                 # move requires forward, strafe (i8; the zone accepts -1..=1)
strafe = 0
facing = 16384              # and facing (u16 yaw, 65536 per turn; 0 faces +Z, 16384 faces +X)
seq = 5                     # optional (commands); default is the bot's next sequence
connection_epoch = 1        # optional (commands); default is the bot's current epoch
expect = "applied"          # optional; default is the action's success tag

[[steps]]
tick = 1
bot = "alice"
action = "select_target"
entity = "creature:108"     # select_target only: none | bot:<name> | creature:<id> | npc:<id>

[[steps]]
tick = 2
bot = "alice"
action = "guard"
lane = "middle"             # guard only: high | middle | low | none (lowers it); the zone decides what it blocks

[[expect]]                  # checked against the snapshot decoded at `tick`
kind = "sees"               # sees | not_sees | position | acknowledged | identity | visible_count | area
                            # | health | target | event | unit | inventory | copper | loot
bot = "alice"
target = "bob"
tick = 1                    # or by_tick = N (sees, event and unit), optionally with from_tick
```

Step outcome tags are `joined`, `applied`, `ignored_stale`, `disconnected`, `resumed`, and `rejected:<kind>`. Rejection kinds come from `game-server`: `invalid_sequence`, `stale_connection`, `simulation`, `unknown_token`, `already_connected`, `reconnect_expired`, and others. The runner adds `no_session` and `not_connected`. A step whose outcome differs from its `expect` fails the scenario.

| Expectation | Required fields | Passes when the bot's snapshot at the tick… |
| --- | --- | --- |
| `sees` | `target`, `tick` or `by_tick` | contains the target; with `by_tick`, at any tick in `from_tick..=by_tick` |
| `not_sees` | `target`, `tick` | does not contain the target |
| `position` | `position`, `tick`, optional `target` | shows the target (default: itself) at exactly `[x, y, z]` |
| `acknowledged` | `sequence`, `tick` | acknowledges that command sequence |
| `identity` | `tick` | shows the bot under the player ID from its first join |
| `visible_count` | `count`, `tick` | contains exactly `count` players, including itself |
| `area` | `area`, `tick`, optional `target` | shows the target (default: itself) inside the named core area (`greyhaven_vale::areas()`, the areas of the hosted vale content) |
| `health` | `health`, `tick` | shows the bot's own exact health |
| `target` | `entity`, `tick` | shows the bot's own target as `entity` (`none` for no selection) |
| `event` | `event`, `tick` or `by_tick`, optional `entity` | carries a feedback event of that kind (`damage_dealt`, `damage_taken`, `miss`, `died`, `evade`, `error:<code>`) about `entity`: whom the bot hit, who hit it, who died, who evaded, or the target an error concerned |
| `unit` | `entity` (not `none`), `state`, `tick` or `by_tick` | shows the unit `alive`, `dead` (a corpse or a dead player), `absent`, `in_combat`, `evading`, `targets_viewer` or `tapped_by_other` |

Error codes are `no_target`, `out_of_range`, `target_dead`, `not_attackable`, `you_are_dead`, `not_dead`, `invalid_target`, `too_many_intents`, `invalid_inventory_move`, `inventory_full`, `invalid_loot`, `not_loot_owner`, `empty_loot`, `money_overflow`, `no_class`, `not_learned`, `not_ready`, `not_enough_resource`, `stunned`, `already_casting`, `invalid_class`, `not_equippable`, `invalid_vendor`, `not_enough_money`, `chat_throttled`, `invalid_quest`, `quest_log_full` and `quest_incomplete`. Units are named `bot:<name>`, `creature:<id>` (the spawn ID of the vale content) or `npc:<id>`.

A `jump`, `start_attack`, `stop_attack` or `release_spirit` step submits that bare command; `select_target` submits `SelectTarget` for its `entity`. Like `move`, every command takes optional `seq` and `connection_epoch` overrides. A well-formed command the zone refuses, such as attacking without a target, is still `applied`: the refusal arrives as an `error:<code>` event in the next snapshot. `select_target` of a bot that has not joined yet is `rejected:unknown_entity` and sends nothing.

A disconnected bot receives no snapshot, so any expectation on it fails. Other bots keep seeing it until reconnect grace expires.

Digest lines have the form `t=<tick> <bot> p<player> e<connection epoch> ack<sequence> (<x>,<y>,<z>) sees[<bots>] | …`. A bot's own combat state follows while it is not unhurt and idle: `hp<health>/<max>`, `dead`, `target=<unit>`, `attacking` and `in_combat`, then `events[…]` with the tick's feedback (`dealt:<unit>:<amount>`, `taken:<unit>:<amount>`, a trailing `!` for critical hits, `miss:<source>><target>`, `died:<unit>`, `evade:<unit>`, `error:<code>`). Ticks where any bot's combat state changes or events arrive are always printed. JSON tick lines carry the same state in `health`, `max_health`, `dead`, `in_combat`, `auto_attacking`, `target` and `events`.

## Control-plane scenarios

```toml
name = "lease-lifecycle"
lease_ttl_ticks = 100
heartbeat_ttl_ticks = 50

[[steps]]
at = 0                      # control-plane time; must not go backwards
op = "assign"
zone = 10
host = "host-a"
expect = "ok"               # optional; or rejected:<kind>
```

| `op` | Fields | Model call |
| --- | --- | --- |
| `register`, `heartbeat` | `host` | `HostRegistry::register` / `heartbeat` |
| `expire_hosts`, `expire_leases` | none | `HostRegistry::expire` / `ZoneDirectory::expire` |
| `assign` | `zone`, `host` | `ZoneDirectory::assign` |
| `reassign` | `zone`, `expected_epoch`, `host` | `ZoneDirectory::reassign` |
| `renew`, `release` | `zone`, `host`, `epoch` | `ZoneDirectory::renew` / `release` |
| `write` | `zone`, `host`, `epoch` | one `advance_tick` through that grant's `FencedZoneRuntime` |
| `prepare` | `transfer`, `entity`, `source`, `destination` | `HandoffRegistry::prepare` |
| `accept` | `transfer`, destination `zone`/`host`/`epoch` | `HandoffRegistry::accept` |
| `commit` | `transfer`, source `zone`/`host`/`epoch` | `HandoffRegistry::commit` |

Leases are named by identity `(zone, host, epoch)`. `source` and `destination` are inline tables: `{ zone = 10, host = "host-a", epoch = 1 }`. Each grant gets its own fenced runtime, so a host can keep trying to write after losing its lease. Rejection kinds are the `ControlPlaneError` variants in snake case, such as `stale_lease`, `lease_expired`, `zone_already_assigned`, `lease_owner_mismatch`, `transfer_id_collision`, `entity_transfer_in_progress` and `transfer_not_accepted`.

After every step the runner reads the directory, epoch floor and handoff records, then checks:

- `single_writer`: at most one granted lease per zone is currently valid, no zone epoch was granted twice, and every directory owner came from a grant;
- `epoch_fenced`: a successful fenced operation presented the current, unexpired epoch; epoch floors never regress; a rejected operation changed no state;
- `handoff_idempotent`: a transfer keeps its ticket and never regresses, repeating a reached phase changes nothing, and each entity has at most one unfinished transfer;
- `retire_after_accept`: a transfer is committed, which retires the source, only after acceptance was observed.

Step lines show owners as `zone<id>=<host>@<epoch><<deadline>`. Violations print `INVARIANT FAIL <name>: <detail>` and fail the scenario. Unit tests in `src/control_plane.rs` feed each check a violating state.

## Expected outputs

Every `scenarios/<tool>/<name>.toml` has a `<name>.expected` text output. `cargo test --workspace --all-features --locked` requires each checked-in scenario to pass and reproduce its expected output byte for byte. After an intentional change, regenerate the files and review the diff:

```sh
MMORPG_SCENARIOS_UPDATE=1 cargo test -p mmorpg-scenarios --test scenarios --locked
```

## Limits

- The bot runner covers one zone per scenario and every command of wire version 4 (`choose_class` with `class` and `sex`, `use_ability` with `ability` and an optional `entity`, `cancel_cast`), plus `resource` expectations on the bot's exact class resource. `class-abilities` walks a Warden, a Ranger and an Arcanist in a column along the `vale-wolf-hunt` route to Timber Wolf 108 and checks class choice and refusal order, rage from swings, focus and mana regeneration with the five-second rule, the global cooldown, the Aimed Shot cooldown, Firebolt's cast, a movement interrupt, a cancelled cast and an overlapping cast. `vale-wolf-hunt` walks a bot from the hub into Wolfrun Woods along trunk-grid lines: it kills Timber Wolf 108 (aggro, out-of-range and refused attacks, damage both ways, death and corpse), then dies to Timber Wolf 106, which evades home, and releases its spirit to the graveyard with half health, where it stands although it sent a walk while dead. Its steps rely on the vale's deterministic creature levels and wander paths; content changes that move creatures need the scenario retuned. `browser-local-session` scripts the browser demo's session (enter, camera-relative run and strafe, jump arc, area, leave, re-enter) through the hosted runtime. `mmorpg-wasm`'s host tests replay the same steps against the WASM local host and `MatchRuntime` and require byte-identical projections every tick while the player is joined (ticks 1–44 and 51–54). Leaving is modelled differently: the local host removes the unit at once, the hosted runtime after reconnect grace, so the away ticks are not compared. The scenario file and that test are separate copies of the steps; changing one needs the same change in the other.
- Scenarios run in process. Transport framing, datagram size limits, TLS and real reconnect timing are not covered. `scripts/smoke-native.py` and `mmorpg-client`'s loopback test still cover those.
- A network mode against `mmorpg-zone-host` is not implemented. The reusable client session lives in `mmorpg-client`, which depends on wgpu and winit unconditionally.
- The control-plane runner checks the in-memory reference model. It does not check a distributed deployment.

### Inventory vocabulary

`move_item` steps require `source_slot`, `destination_slot` (`u8`) and `quantity`
(`u16`), and support the same sequence/connection overrides as other commands.
`inventory` expectations require `inventory_revision` and `sheet` (presence).
For a present sheet, optional `slot`, `item` and `quantity` are supplied together;
empty slots use item/quantity zero. Expectations read decoded self projections.
The `inventory-resume` scenario checks splitting, refused partial swaps, duplicate
sequences, stale connection epochs, resume, merging, periodic sheets and isolation
from another player's bag.

### Corpse loot vocabulary

`loot` steps require `creature` (`u32` spawn ID) and `died_at` (`u64` death tick),
with the usual sequence/connection overrides. `copper` expectations require
`copper` and `tick`. `loot` expectations require `sheet` (presence) and `tick`;
a present sheet also requires `creature` and `died_at`. All read decoded player
projections. No scenario command supplies money or item rewards.

`corpse-loot-resume` kills wolf 108 at tick 912, refuses a wrong death fence,
settles two copper/two Torn Fur once, refuses duplicate and stale claims, resumes
the same player across connection epochs, rejects the old connection, and sees
the periodic complete bag at tick 920. Existing hunt/scenario outputs stay
unchanged; copper/loot expectations add explicit observations to this scenario.

### Vendor vocabulary

`buy_item` steps require `npc` (`u32`), `offer` (`u8` stock index) and
`quantity`; `sell_item` steps require `npc`, `bag_slot` and `quantity`. Both pass
their values through unchanged and support the usual sequence/connection
overrides; the zone refuses unknown vendors, offers, reach, copper and slots
with feedback events. `use_door` steps require `door` (`u8`) and `open`
(boolean); the zone checks reach and blockers. `vendor-resume` is refused out of
reach on the spawn plaza (for the vendor and the inn door), walks to the inn,
opens its door, walks in to Innkeeper Bram Tolliver behind his bar, sells its
Torn Fur, is refused for
lacking copper and at a guard, sells the Worn Dagger and buys it back after a
connection resume, checking copper and the bag sheet at each step.

### Chat vocabulary

`chat` steps require `channel` (`say` or `yell`) and `text`; the runner refuses
text core would refuse. `chat` expectations check the lines a bot heard at a
tick (or within a `by_tick` window): `target` (the speaking bot) with `text`, or
`count` for the number of lines heard. `zone-chat` covers say and yell ranges
with a bot about 32 m away, the speaker hearing itself, the once-per-second
rate limit and its `chat_throttled` error.

`emote` steps require `emote` (`wave`, `bow`, `cheer`, `laugh` or `point`) and
send it as a zone-local emote. In a `chat` expectation, `target` with `emote`
instead of `text` checks that the bot heard that emote from that speaker; `count`
includes emotes. `zone-emote` covers the 20 m emote range, the speaker seeing its
own emote and the rate limit an emote shares with chat, across a reconnect.

### Quest vocabulary

`accept_quest` steps require `npc` (the giver) and `quest`; `complete_quest`
steps require `npc` (the ender), `quest` and `choice` (the reward index);
`abandon_quest` steps require `quest`. All pass their values through unchanged.
Quest sheets arrive on change ticks and every ten ticks, so quest expectations
read the latest sheet the bot received: `quest` with `quest` and `quest_state`
(`active`, `completed` or `absent`) and, for an active quest, optional
`progress` (three objective slots); `marker` with `npc` and `marker`
(`available`, `in_progress`, `complete` or `none`). The `quest_progress` and
`quest_completed` events carry no unit; digests show them as
`quest:<id>:<slot>=<count>` and `quest_done:<id>`. `quest-resume` is refused
out of reach of Marshal Elden Greywatch, walks to him, is refused the locked
second quest, accepts *Trouble in the Woods*, cannot turn it in incomplete,
abandons and re-accepts it, and keeps it across a connection resume while the
Marshal's marker follows the log. The full chain is covered by
`crates/mmorpg-core/tests/quest_chain.rs`.

The `strike_swept` event (a directed Heroic Strike, #154) concerns the struck
creature, or the striker for a whiff; digests show it as
`swept:<striker>:<ability>:<side><height>><struck or ->`. Bot steps do not send
directed strikes yet; `crates/mmorpg-core/tests/directed_strike.rs` covers the
coverage rules.

The `blocked` and `guard_broken` events (the Warden's shield guard, #156)
concern the attacker for the defender and digest as
`blocked:<attacker>><defender>:<lane>:<amount>` and
`guard_broken:<attacker>><defender>:<lane>:<ticks>`. Bots do not guard yet;
`crates/mmorpg-core/tests/guard.rs` covers lanes, coverage, breaks, re-guard,
slowed movement and replay.
