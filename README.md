# mmorpg

Server-authoritative MMORPG foundation that scales by distributing **zone ownership**, with deterministic physics and a separate client presentation layer.

The native Rust client now connects to the standalone zone host and renders server-authoritative multiplayer movement through wgpu. The GitHub Pages demo runs the same Rust zone simulation in the page as a local single-player WASM host; in a development setup, browser tabs can instead join a zone host together over WebTransport. The in-memory control plane and fenced runtime establish tested contracts; they are not yet a production distributed fleet. See the [architecture capability map](docs/ARCHITECTURE.md#scope-and-current-state) for implemented behavior and deployment prerequisites.

Decision history and stable cross-references: [MMORPG Decision Atlas](docs/decisions/atlas.md). The atlas distinguishes implemented behavior from agreed but unimplemented design.

## Authority map

| Concern | Authority |
| --- | --- |
| Zone-local gameplay rules and interest policy | `mmorpg-core` |
| Collision and physical movement | `physics-engine` |
| Command/snapshot wire encoding | `mmorpg-protocol` |
| Session ticks, reconnects, replay/recovery, WebTransport | `game-server` |
| Zone placement, lease fencing, host routing, handoff metadata | `mmorpg-control-plane` |
| Durable character records and exclusive character leases | `mmorpg-characters` (file entries for development/LAN, browser `localStorage` entries; PostgreSQL pending) |
| Adapter from a zone simulation into `game-server` | `mmorpg-game-server` |
| Presentation scenery: props, roads, water, relief, terrain grid (clients only) | `mmorpg-scenery` |
| Native graphics | `mmorpg-client`: wgpu/winit adapter over pinned `3d-lab` mesh/camera models |
| Browser graphics | pinned `3d-lab` browser renderer |
| Browser zone host (single-player demo) | `mmorpg-wasm`: thin wasm-bindgen adapter over `mmorpg-core` and `mmorpg-protocol` |
| Runtime input semantics | `input-bindings` (browser and native action runtime; the browser Settings modal embeds its binding workbench with per-character profiles, native editor pending, [#41](https://github.com/moritzbrantner/mmorpg/issues/41)) |
| User-facing settings | `settings` (browser Settings modal over `@moritzbrantner/settings-browser`; native pending, [#41](https://github.com/moritzbrantner/mmorpg/issues/41)) |
| Asset normalization/provenance | `asset-tooling` (future content slice) |
| Social systems | `social-service` where its existing authority fits |
| Runtime/performance evidence | `runtime-profiler` integration, without moving gameplay truth |

The MMO repository composes those foundations. It should not grow substitutes for them.

The [rendering and asset adoption record](docs/ARCHITECTURE.md#rendering-and-asset-provider-adoption) distinguishes active external components, in-house foundations, and unintegrated character capabilities with source-backed evidence.

## Distributed shape

```text
                         control plane
                  zone directory / placement
                    leases + fencing epochs
                           /      \
                          /        \
                         v          v
                 zone host A     zone host B
                ┌────────────┐   ┌────────────┐
client ────────▶│ game-server│   │ game-server│◀──────── client
                │ MatchHost  │   │ MatchHost  │
                │ zone 10    │   │ zone 11    │
                └─────┬──────┘   └─────┬──────┘
                      v                v
                 mmorpg-core      mmorpg-core
                      |                |
                      v                v
                physics-engine   physics-engine
```

A zone is a single-writer authoritative simulation. A host may own many zones and the fleet may contain many hosts. Cross-zone movement transfers authority at a deterministic boundary using a unique transfer ID plus source/destination lease epochs. Two hosts must never concurrently own the same zone epoch.

This deliberately avoids shared mutable physics across machines. Horizontal scale comes from more zones, more zone hosts, bounded per-zone populations, player-scoped interest projections, and later evidence-driven zone splitting.

## Implemented foundation

The workspace provides:

- a Rust 1.98 workspace and reusable validation workflow;
- a deterministic zone simulation that uses the pinned `physics-engine`;
- validated immutable collision content, gravity, and velocity-preserving physical recovery;
- stale-command rejection and bounded per-zone player capacity;
- canonical full-zone snapshots for replay/recovery that reference their immutable content by revision and fingerprint and fail closed on any content mismatch;
- player-scoped interest snapshots with a spatial index for network publication;
- [actual-zone physics adoption workloads](docs/PHYSICS_WORKLOADS.md), including unchanged canonical/projection bytes across the engine update;
- [deterministic visibility workloads](docs/INTEREST_WORKLOADS.md) with wire parity and snapshot-size evidence;
- [deterministic Greyhaven combat workloads](docs/COMBAT_WORKLOADS.md) with AI/physics/publication counters, replay/recovery parity and ratcheted work/byte budgets;
- an explicit `game-server::GameSimulation` adapter;
- deterministic mapping from `ZoneId` to `game-server::MatchId`;
- a provider-neutral in-memory control-plane reference model with heartbeat-gated host placement and fenced, expiring zone leases;
- deterministic host registration/heartbeat expiry, lease renewal/expiry, and a restartable fencing-epoch floor contract;
- idempotent prepare/accept/commit handoff metadata that survives lease renewal and reserves one transfer per entity;
- a fenced runtime interface that rejects expired/stale owners before commands, admission, ticks, or publication in the reference model;
- a runnable multi-zone host with one WebTransport routing surface and separate operational status;
- a deliberately single-player GitHub Pages tech demo that runs the shared zone simulation as a local WASM zone host and renders its player-scoped projections with the pinned `3d-lab` renderer;
- local browser-demo character creation with Warden, Ranger, and Arcanist starter classes, male/female presentation variants, stable per-character local identities, and per-character appearance saves;
- facing-relative movement (run, strafe, backpedal) and grounded jumps driven by a const-generated integer trigonometry table;
- units, targeting, auto-attack with an integer hit table, death, corpses, respawn, release spirit, out-of-combat regeneration and tapping, driven by queued intents whose refusals are feedback events, never session errors;
- deterministic creature AI (wander, aggro through the interest index, family assist, chase, leash and evade) with a canonical zone RNG, and recovery continuation proven tick by tick mid-chase, mid-swing, after a death and during an evade;
- the Greyhaven Vale starter-zone content revision (hub, woods, farm, lake and hollow colliders, a validated spawn plaza, open road corridors, named areas, 59 creature spawns of seven templates, nine NPCs and a graveyard), hosted by the zone host and the browser's WASM local host, and rendered by both clients from presentation-only `mmorpg-scenery` (props, relief terrain, water) plus creature and NPC bodies (procedural models in the browser, low-poly box-built models in the native client);
- quests: the nine-quest Greyhaven chain from the hub's quest givers to Garrick Redbrand and back, with kill, collect (quest-only Wolf Pelt drops), talk and explore objectives, prerequisites, a ten-quest log, XP/copper rewards with one item choice and per-player `!`/`?` NPC markers, all in core and recovered exactly; a scripted Warden completes the first three quests on the hosted content;
- strict Rust/browser snapshot v18 and command v11 compatibility tests (self, target, event and chat sections, compact priority-ordered unit records, facing, viewer identity, golden command bytes) and bounded client interpolation;
- player projections capped by deterministic relevance priority and packed by section priority into a measured single-datagram byte budget;
- native session resume with preserved player identity, command sequencing and connection-epoch resets;
- a development [browser online mode](#browser-online-mode) in which browser tabs join a zone host over WebTransport with the native client's session semantics (the Pages build stays offline);
- [headless deterministic scenario runners](docs/SCENARIOS.md) for scripted bots against the real zone host path, including a Wolfrun Woods hunt that fights, dies and releases its spirit, and for control-plane lease/handoff sequences with invariant checks after every step;
- architecture and roadmap documents that keep future persistence and orchestration choices replaceable.

The control-plane implementation in this slice is a **reference model**, not yet a production distributed consensus system. Host registration is placement eligibility only: a missed heartbeat blocks new assignment to that host but does not revoke an already-issued zone lease. Existing lease deadlines and fencing epochs remain the authority boundary. The model makes ownership, liveness-sensitive placement, epoch fencing and handoff idempotence executable ahead of the [selected PostgreSQL production directory](docs/ARCHITECTURE.md#tech-001--postgresql-for-production-zone-ownership) and any future orchestration infrastructure.

## Native multiplayer client

The native client uses **wgpu 30.0.1 + winit 0.30.13**, shared Rust world geometry, and the existing WebTransport protocol. W/S run and backpedal, A/D or Q/E strafe relative to a third-person orbit camera (drag to orbit, wheel to zoom), Space jumps, left click selects the specific visible creature or NPC without attacking, Tab cycles attackable creatures and N cycles NPCs, F toggles auto-attack, 1–4 use the class abilities (without `--class warden|ranger|arcanist` the window opens a local character roster with a turntable preview, kept under the platform config directory), B/C/L toggle bags, character and quest log, Escape cancels a cast or closes panels and R releases a dead spirit; the client only sends intent and the GPU renders interpolated player-visible snapshots of players, creatures and NPCs with their facing. An egui interface shows unit/target frames, the action bar with cooldowns, minimap, quest tracker/log, quest and vendor windows, bags, character pane, loot and chat, plus world-space nameplates, NPC quest markers and floating combat text; every control sends an existing command. Enter opens the selected NPC’s existing quest/vendor context (otherwise chat): Up/Down browses every quest, stock offer and bag slot, 1 accepts/turns in or buys, 2 sells, Left/Right chooses a quest reward, H pages authored dialogue/objectives, and Escape closes the context or cancels an armed click. The zone validates range, quest state and transactions. Multiple client processes can join the same zone.

```sh
python3 scripts/smoke-native.py --window
```

This builds the client and host, creates disposable development TLS credentials, checks server readiness, verifies an actual GPU frame, opens a window for 120 frames, and shuts everything down. Requires Python 3, OpenSSL and a supported graphics backend. Omit `--window` for the offscreen GPU check.

For persistent interactive play, see [native setup and controls](docs/NATIVE_CLIENT.md). Tauri is a possible future launcher shell; the gameplay window uses a native GPU surface. See [the decision](docs/adr/0001-native-client.md).

Start the complete local native development environment with:

```sh
./scripts/dev-native.sh
```

The launcher fetches the latest merged source and fast-forwards safely before building, then watches local Rust, shader, scenery and build-input edits and restarts the host and client. Each restart resets the disposable zone. Local edits and commits are preserved; an update that needs manual integration stops with an explanation. Use `--no-update` to work offline or intentionally run your current checkout, and `--no-watch` for a single interactive launch. Requires Python 3 in addition to Cargo, Git, curl and OpenSSL; see [native development](docs/NATIVE_CLIENT.md#interactive-play).

## Browser tech demo

`web/` is the GitHub Pages client. It runs the shared Rust zone simulation in the page as a **local single-player zone host** ([ADR 0002](docs/adr/0002-browser-embeds-zone-simulation.md)): the `mmorpg-wasm` crate compiles `mmorpg-core` and `mmorpg-protocol` to WebAssembly and builds the same `ZoneSimulation` and content revision as `mmorpg-zone-host` (zone 1, Greyhaven Vale). There is one implementation of the movement and combat rules; the browser owns none.

- **Enter World** joins the local zone and spawns the selected character; **Characters** (or Escape) leaves it and removes the unit. The next entry is a new player at the spawn. A projection the strict decoder rejects fails closed: entry is refused, or the page leaves the world, and the reason appears under **Enter World**.
- **Bags** (or B) shows 16 projected slots. Select a stack, set its quantity, then choose a destination; only the zone changes the items. Escape closes the panel before leaving the world. Each fresh world entry gets the starter bag; inventory is not saved across entries.
- **Character** (or C) shows the six equipment slots (main hand, off hand, head, chest, legs, feet) with item names and stats, the Stamina/Strength/Agility/Intellect totals, health and damage range from the received projection. Select an equippable stack in Bags and choose **Equip**, or choose **Unequip** in the pane; the zone performs every swap and refusal. Bags and the character pane can be open together.
- **Vendor** (or V) trades with Innkeeper Bram Tolliver at the inn, east of the spawn plaza: within 5 m it lists his priced stock with **Buy** and your bag with each stack's sale value and **Sell**. Only the zone moves copper and items; opening it closes Bags, Loot and the character pane.
- **Click a visible creature or NPC** to select that specific unit. Selection sends the existing zone-validated target intent and never starts an attack. The closest drawn body under the pointer wins; solid scenery blocks picking hidden units. A camera drag, canceled pointer or click on empty ground does not select. A confirmed quest-NPC or vendor selection opens its existing dialogue or shop, showing reach feedback when needed. Guards and other NPCs remain selectable without invented dialogue.
- **Talk** (T, or the button in the quest log) opens the dialog of the selected quest NPC, or the nearest quest giver when no NPC is selected: the quests it offers (**Accept**), quests ready to turn in with their reward choice (**Complete quest**) and quests still in progress. Talk and Vendor never substitute another NPC for an explicit NPC selection. **Quests** (or L) shows the log with each objective's progress and **Abandon**; the HUD tracks every active objective, and `!` (available), `?` (ready) and grey `?` (in progress) float over quest givers. Givers need you within 5 m; only the zone accepts, credits and rewards quests. Start with Marshal Elden Greywatch west of the spawn plaza.
- **Chat**: Enter focuses the chat field (held movement stops while you type); Enter sends the line to players within 20 m, `/y` yells it to 60 m, Escape returns to the world. `/wave`, `/bow`, `/cheer`, `/laugh` and `/point` emote to players within 20 m. You may speak or emote once per second; the zone delivers every line and emote, including your own, and they are not saved.
- **Loot** selects your current owned corpse, or the nearest visible owned corpse. Move within reach to see its projected copper/items, then choose **Claim rewards**. The HUD shows received copper; full bags retain corpse rewards. Bags and Loot open separately, and Escape closes the current panel before leaving.
- W/S run and backpedal, A/D (or Q/E) strafe relative to the orbit camera, Space jumps. A left drag looks around without turning the character, a right drag turns the character with the view (mouse-look), and the wheel zooms smoothly; the camera stays above the ground but collides with nothing else. F3 toggles frame rate, node counts and the renderer's work observations. Keyboard and standard-mapping gamepad input resolve through `input-bindings` to an MMORPG-owned action catalog (`web/src/input/game-controls.ts`): the left stick moves, A jumps, B backs out (or cancels a cast), X toggles auto-attack, Y releases the spirit, RB targets next and the d-pad uses ability slots 1–4; open bags, the character pane, loot or the vendor form a non-blocking overlay that claims Escape while movement falls through, and leaving the world, focusing a control, opening Settings, blur or a hidden tab release held movement. Mapped game keys (Space, Tab, arrows, F3, …) suppress cancelable browser defaults only while the game owns input; text fields, the Settings dialog, Shift+Tab and Ctrl/Cmd shortcuts keep their browser behavior, and a mouse click on a HUD or panel button hands focus back to the world so Space still jumps. **Settings** (on character selection and in the world toolbar) is a modal over the shared `settings` session with field of view, look sensitivity and reduce motion (exact numeric entry, per-setting and confirmed global reset, saved locally), plus the shared `input-bindings` workbench for the selected character's own key bindings, which apply and save on each confirmed change. Settings pauses the browser-local zone; an online zone keeps running with the player's input blocked. The page enters with the character's class and sex and encodes command wire v11 with strictly increasing sequences and resends the current intent like the native client.
- A `WorldSource` seam (`web/src/world/`) advances fixed 30 Hz ticks from a bounded accumulator and hands presentation only encoded player-scoped projections, decoded by the same `web/src/replication.ts` the online source uses. Canonical state never reaches rendering. The [online mode](#browser-online-mode) swaps in a WebTransport source behind the same seam.
- The world is drawn from a versioned `scenery()` export of the WASM module (format v3), which maps the same `mmorpg-scenery` Greyhaven Vale the native client draws. The browser builds it as a stylised low-poly valley: relief terrain in biome tones with smooth roads, plaza and shore, snow-capped distant ranges softened by renderer fog, procedural models for every prop (keep, timber-framed houses, palisade, windmill, camp, cliffs, oak, pine and birch trees, grass, flowers, reeds …) with shared instance geometry for repeated trees, grass and rocks and static batches for structures, a translucent lake, and a renderer sky with a matching CSS backdrop. Structures' walls are their exact core colliders. Players are animated humanoids with class gear; other players share a neutral look: projections carry each player's class and sex as its appearance code, which the browser does not draw yet. A circular minimap shows the subzone name, roads, water, buildings and units.
- It is procedural presentation, not authored art: the pinned renderer draws vertex-coloured relief, distance fog, emissive lamps/runes/staff orbs and unlit target rings. Ambient and directional lighting follow a cosmetic twenty-minute noon/dusk/night/dawn cycle; reduced motion holds noon. This clock never changes simulation or NPC behavior. Repeated scenery shares immutable geometry and stable instance payloads, and picking tests its actual rendered transforms. Textures and authored replacement art remain separate work. Spell visuals (a Firebolt projectile, Frost Nova ring, Arcane Barrier bubble, Blizzard shards, Ranger arrows and Warden swing arcs) are short-lived renderer nodes driven by projection events, and per-actor procedural poses follow ordinary swing occurrences and accepted abilities, including creature casts. Confirmed positive damage produces bounded target-local impact sparks. Players and creatures fall on observed death, while newly hydrated corpses start settled; death overrides actions. Reduced motion snaps corpses and hides action/effect motion. Units stand at their physics position plus the shared relief.
- Players, creatures and NPCs are animated Quaternius CC0 models (Atlas 6.3.6; the same class/sex, family and role mapping as the native client), with the procedural low-poly models (quadruped animals with a trot, compact humanoids; unknown templates fall back to a box) shown while a model loads, with the disposition colour (hostile, neutral, friendly) as an accent that their minimap dots share; corpses lie down. Tab selects the nearest living creature, F or a right click (a right drag still turns) toggles auto-attack, 1–4 (or a click on the action bar) use the chosen class's abilities at the selection, Escape cancels a cast and R releases a dead spirit. A Warden's **directed Heroic Strike** is a separate bindable action (default G): hold it, move the pointer left/right and up (high) or down (low), and release to swing; the zone resolves which creature the sweep physically reaches (or a whiff) and the client draws the arc at the aimed height. Blur, Settings, an open panel or a click cancel the held aim without a strike; the Heroic Strike slot shows the working binding as a compact hint and in its tooltip, or says it is unbound. The Warden's **shield guard** is another bindable held action (default Z): hold it and move the pointer up (high) or down (low) to pick the lane; the zone blocks matching creature blows from the front and shield side, slows movement while the guard is up, suspends it in the air, and a heavy charging blow breaks it for 1.5 s before the held lane returns. A lane indicator beside the screen centre shows the zone's guard (steel while protecting, dim while suspended, red while broken), blocks and breaks flash at the lane and float Block / Guard broken text, and the most recent of guard and directed strike owns the pointer (pressing one cancels the other without a strike). Warden melee is **movement-dependent** (Atlas 5.2.69–5.2.70): charging at the target deals +20% melee damage, an airborne (overhead) blow +35% while the airborne Warden takes +25% creature melee damage, a planted Warden takes −10% and one backing or strafing away deals −15%; melee pressed during the short landing recovery waits and fires when it ends. The status line names the current stance. A directed Heroic Strike that damages a creature opens the Warden's first **combo**: Shield Bash pressed during the 0.2 s impact or the following 0.6 s window hits exactly that creature (whatever is selected) and may ignore only the slash's global cooldown; the status line announces the open combo. The **action bar** (bottom centre) shows the class's four abilities with keybinds and glyph icons; hover, right-click or long-press a slot for its tooltip (cost, range, cast time, cooldown, description). Slots tint blue without enough resource, turn the keybind red when the target is out of range, sweep radially over a cooldown (seconds from 2 s) or the global cooldown, highlight while casting and grey out with their unlock level until learned. The **player frame** shows health, the class resource (rage red, focus amber, mana blue) and buff and debuff icons with time left; the **target frame** shows the target's health, auras and cast; a **cast bar** sits above the action bar (channels drain backwards, with an Interrupted flash). Floating **combat text** shows damage, heals (green), Absorb, smaller periodic damage and interrupts, and the status lines give readable messages for every error code. Names and numbers come from the module's versioned `catalog()` export and the decoded projection; range and descriptions are presentation facts held in `web/src/world/units/ability-presentation.ts`. Nameplates follow in a later step.
- Each roster character is **durable** (Atlas 3.1.6): entering leases it from browser storage through the same `mmorpg-characters` record and lease rules zone hosts use, and its level, XP, copper, bags, equipment, quests, health, cooldowns and position are committed every five seconds, when the page is hidden and on leaving, then resumed on the next visit; the roster shows its committed level. One tab plays a character at a time (Web Locks); a reloaded page's newer local zone epoch fences the page it replaces. The world itself (creatures, corpses) is not saved: #40 owns the composed character/world save bundle. There is no handoff, shared world or multiplayer authority, and browser storage is a development adapter.

Building the page requires the Rust toolchain from `rust-toolchain.toml` (with the `wasm32-unknown-unknown` target) and the `wasm-bindgen` CLI at the crate's exact version: `cargo install wasm-bindgen-cli --version =0.2.129 --locked`. `bun run build` and `bun run dev` first sync browser dependencies from the committed lockfile, then compile the module. `bun test` also compiles the module before its WASM tests. The generated bindings are ignored build output. Update the Git checkout explicitly to build newer source; these scripts do not change source revisions or dependency pins.

The standalone host, the native client and the browser's WASM local host share the Rust Greyhaven Vale content (`mmorpg_core::greyhaven_vale`). Snapshot schema/wire version 15 carries the viewer's own state (with class, resource, cast, cooldowns, auras and melee range), its target's target, cast and auras, the self sheet (bag, equipment, stat totals and the quest log with completed quests and quest-NPC markers), bounded feedback events, heard chat lines and emotes and compact, priority-ordered unit records within a one-datagram byte budget, and command wire version 9 carries `Move`, `Jump`, `SelectTarget`, `StartAttack`, `StopAttack`, `ReleaseSpirit`, `MoveItem`, death-fenced `Loot`, `UseAbility`, `CancelCast`, `ChooseClass`, `EquipItem`, `UnequipItem`, `BuyItem`, `SellItem`, `Chat`, `Emote`, `AcceptQuest`, `CompleteQuest` and `AbandonQuest`; v14 and earlier snapshots, v8 and earlier commands and old recovery bundles require an explicit migration decision. See [the wire specification](docs/PROTOCOL.md).

## Browser online mode

The same page can join a real zone host instead of its local zone, over WebTransport with the `game-server` session protocol: the browser counterpart of the native client, and the transport swap [ADR 0002](docs/adr/0002-browser-embeds-zone-simulation.md) kept open. It is a development and self-hosting mode; the GitHub Pages build stays offline.

```sh
./scripts/dev-browser-online.sh
```

The launcher reuses or creates the ECDSA P-256 development certificate under `target/dev-tls/` (valid for 10 days; WebTransport certificate hashes allow at most 14), starts `mmorpg-zone-host` for zone 1 on UDP port 4433 (status on TCP port 8080) and Vite on `127.0.0.1:5173`, and prints a URL to open in two tabs of a Chromium-based browser. Ctrl-C stops both processes, `--check` verifies the setup and exits, and `MMORPG_DEV_TRANSPORT_PORT`, `MMORPG_DEV_STATUS_PORT` and `MMORPG_DEV_WEB_PORT` override the ports. It shares the certificate and ports with `./scripts/dev-native.sh`, so run one of them at a time; a native client can join the launcher's zone with the [second-player command](docs/NATIVE_CLIENT.md#interactive-play).

- **Selecting a host.** `?server=https://<host>:<port>/game/matches/zone-<id>&certHash=<base64 SHA-256 of the DER certificate>` joins that host; without `certHash` the host needs a publicly trusted certificate. A build with `VITE_ZONE_SERVER` and `VITE_ZONE_CERT_HASH` offers the same through a **Play online** checkbox on the character screen, unchecked by default; the default remains the local WASM zone. The address must be a canonical admission route: credentials, queries, fragments and resume routes are refused, and an invalid address disables entry with the reason instead of falling back to the local zone.
- **Session.** `OnlineZoneSource` (`web/src/world/online-zone-source.ts`) reads the welcome (player, tick rate, connection epoch, reconnect token), sends command frames with strictly increasing sequences, reassembles and verifies snapshot frames and fragments per connection (`web/src/session/`, a port of the pinned `SnapshotReassembler`), and decodes projections with the same decoder as the local zone. Movement is held intent the page resends every 50 ms, like the native client; a jump or any other discrete intent is resent byte for byte until a projection acknowledges it, and no later command overtakes it meanwhile, so the host's stale-sequence rule cannot drop it unseen.
- **Fail closed.** Every projection must carry the page's zone, player and the content revision of the scenery the page loaded. A mismatch or a malformed frame leaves the world with the reason under **Enter World** rather than drawing a different world.
- **Interruptions.** A lost connection or five seconds without projections triggers one bounded resume with the in-memory token while the last scene stays visible and the top bar shows **Reconnecting…**; Characters or Escape cancels it. The resume keeps the player and command sequencing, starts stopped until acknowledged, drops intents made meanwhile and resets interpolation for the new connection epoch. A refused or expired resume ends the session; a new player is never substituted. Tokens stay in memory: they never appear in the page URL or the page's messages (a browser's network console may still show a failed resume request).
- **Leaving.** Characters, Escape or closing the tab ends the session; the host keeps the character for its reconnect grace (five seconds in the launcher), then removes it. Sessions are anonymous. The page joins with the character's class and sex (its first command is `ChooseClass`, like the native client); the look stays presentation-only, and nothing is saved.

`python3 scripts/smoke-browser-online.py` proves it end to end with the production bundle: it starts a real host with a disposable certificate on OS-selected ports, opens two Chromium pages in online mode, runs one character and checks the other page sees it move in its own decoded projections, then closes the running page and checks its unit disappears after the host's one-second grace.

### Play together on a local network

On the **host computer**, use its actual LAN IPv4 address (for example `192.168.1.20`):

```sh
./scripts/dev-browser-online.sh --lan 192.168.1.20
```

The launcher runs **one** authoritative Greyhaven zone host (UDP 4433) and serves the browser game over HTTPS on that LAN interface (TCP 5173). The printed link names the correct LAN WebTransport endpoint and its pinned development certificate hash. Other computers on the same network open **that link** in a Chromium-based browser; they do not need the Rust toolchain or a copy of the game source. Each player selects **Enter World** to join the same zone. `--check` also works with `--lan` to verify server readiness and the HTTPS page before playing.

The page uses a self-signed **development** HTTPS certificate. On each device, trust the host's certificate `target/dev-tls/cert.pem` for this locally controlled server (or explicitly accept the browser's certificate warning where supported) before using the shared link. Never distribute `key.pem`. The launcher includes the LAN IP in the short-lived certificate's subject alternative names, regenerating the pair if the IP changes. Open firewall access for **TCP 5173** (HTTPS page) and **UDP 4433** (WebTransport) only to trusted LAN peers; do **not** forward the ports to the internet. The server has anonymous sessions and no public authentication or production durability.

You can also connect a **native client from another LAN computer** to this host (or to one running `./scripts/dev-native.sh`) by copying **only** the public certificate `cert.pem` to that computer and supplying its LAN endpoint:

```sh
cargo run --locked -p mmorpg-client -- \
  --url https://192.168.1.20:4433/game/matches/zone-1 \
  --certificate ./cert.pem
```

For a host-only check without a browser, the native zone host is already available as `mmorpg-zone-host`; no separate LAN gameplay server is needed. The ordinary `./scripts/dev-browser-online.sh` (without `--lan`) stays bound to loopback and uses HTTP for local development. The GitHub Pages demo remains local single-player.

## Scaling model

The first scaling unit is the zone:

1. one zone has one active authority lease;
2. one `game-server::MatchRuntime` drives that zone;
3. one process can host many zones through `MatchHost`;
4. more processes add more independent zone capacity;
5. a routing/control-plane layer tells clients and operators which host owns each zone;
6. cross-zone handoff moves player authority only after the destination accepts the transfer.

The initial `MAX_PLAYERS_PER_ZONE` is a safety bound, not a performance claim. Capacity changes require deterministic workload evidence. Large cities, raids, or crowded world events should eventually be handled by measured spatial/instance partitioning rather than silently increasing one process's bound.

## Persistence

The [starter progression rules](docs/PROGRESSION.md) define the shared XP curve
and level-difference kill rewards. The zone awards and projects durable XP; the browser displays its XP bar and level-up feedback from that state.

[Starter inventory rules](docs/INVENTORY.md) define the immutable item catalog
and atomic 16-slot bags. Player-owned bags, queued moves and recoverable self sheets are implemented; the browser displays those slots and sends split/move/merge intent through its Bags panel.

[Starter loot rules](docs/LOOT.md) define validated weighted rewards and deterministic rolling. Core generates corpse rewards once, fences claims by owner/death/reach, and atomically credits money and bags with replay/recovery coverage. The browser Loot window displays the complete selected-corpse sheet, sends the fenced claim and shows refusals.

Use lightweight CQRS/CQS at service boundaries: commands mutate authoritative durable state; queries read it. Do not introduce event sourcing by default. Zone hot loops must not synchronously depend on a distributed database. `mmorpg-characters` is the durable character boundary (#30): a versioned `CharacterRecord` captured from and admitted into a zone, stored with an exclusive lease held by one zone at one control-plane epoch; every commit names its lease and fails closed once a newer epoch of the zone or a release superseded it. Its development backend is a directory of atomically replaced, OS-locked entry files; PostgreSQL (TECH-002) and zone-host admission remain pending. Zone checkpoint storage is a separate upcoming boundary.

## Pinned foundations

- `physics-engine`: `16833b766629c354a6a5925b991eeb74208a242d`
- `game-server`: `a3851dab9c1fb25dd31b465fb554ca475769caab`
- `3d-lab` (`three-d-core`, `three-d-camera`, `three-d-projective`, `three-d-animation` and the browser `@moritzbrantner/three-d-renderer`, kept on one commit): `df52c324a5692d04d2016d8a29f5bc4368e4aac0`
- reusable validation workflow: `45042e56be120b438096e774027637cac0280075`

## Validation

The maintained `mmorpg-core` release library has an opt-in [native size budget and reviewed baseline](docs/SIZE_EVIDENCE.md). Run `bun run --cwd web size:budget` on Linux x64 after installing the locked web development dependencies. Archive bytes remain separate from runtime performance evidence.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
```

`mmorpg-core`, `mmorpg-protocol` and `mmorpg-wasm` must keep compiling for WebAssembly:

```sh
cargo build -p mmorpg-core -p mmorpg-protocol -p mmorpg-wasm --target wasm32-unknown-unknown --locked
```

For the browser client, from `web/` using Bun 1.4.2 (needs the pinned `wasm-bindgen` CLI above):

```sh
bun install --frozen-lockfile
bun test
bun run build
```

Committed Rust and Bun lockfiles make local and CI resolution reproduce the same dependency graphs. Rust and browser tests both consume `fixtures/protocol/player-snapshot-v18.hex`, `fixtures/protocol/commands-v11.hex` and the `game-server` session frames in `fixtures/protocol/session-frames-v3.hex`.

The browser's online mode has its own end-to-end check against a real zone host (needs cargo, OpenSSL, a built `web/dist` and Playwright Chromium, like `scripts/smoke-browser.py`):

```sh
python3 scripts/smoke-browser-online.py
```

## Run a standalone zone host

Do not launch competing processes for the same zones: this binary does not acquire control-plane leases yet.

Provide a TLS certificate and key, then start one process hosting one or more zones:

```sh
MMORPG_ZONE_IDS=10,11 \
MMORPG_CERT_PEM=cert.pem \
MMORPG_KEY_PEM=key.pem \
MMORPG_RECOVERY_DIR=recovery/zone-host \
cargo run --locked -p mmorpg-game-server --bin mmorpg-zone-host
```

The host uses one WebTransport listener (port `4433` by default) and routes sessions under `/game/matches/zone-<id>`. Operational HTTP status is exposed separately on port `8080`:

- `/healthz` reports process liveness;
- `/readyz` reports whether at least one hosted zone can accept sessions;
- `/status` reports host capacity and per-zone readiness;
- `/matches/zone-<id>/readyz` and `/matches/zone-<id>/status` expose a single zone's operational projection.

When `MMORPG_RECOVERY_DIR` is set, graceful `SIGINT`/`SIGTERM` shutdown writes one atomic recovery bundle for the complete configured zone set. Restarting with the same zones restores simulation, command-sequence, and reconnect state, then consumes the old bundle. A missing zone, an extra zone, or corrupt recovery data fails startup closed instead of partially restoring a host. Durable crash recovery remains a separate persistence boundary.

These endpoints consume `game-server` host state and never mutate or redefine zone gameplay authority. Configuration is explicit through `MMORPG_ZONE_IDS`, `MMORPG_PORT`, `MMORPG_STATUS_PORT`, `MMORPG_CERT_PEM`, `MMORPG_KEY_PEM`, `MMORPG_RECOVERY_DIR`, `MMORPG_ROUTE_PREFIX`, `MMORPG_RECONNECT_GRACE_TICKS`, and `MMORPG_DRAIN_GRACE_MS`; recovery is opt-in, while invalid configured values fail startup instead of silently falling back.

The [saved grass authoring package](docs/SCATTER_AUTHORING.md) freezes accepted Outpost placements and reproduces their exclusion-mask filtering through pinned asset-tooling. Both clients consume its saved selection through shared scenery.

The [saved relief authoring package](docs/RELIEF_AUTHORING.md) freezes the Outpost heightfield and reproduces its saved flatten coverage through the same pinned producer. Both clients consume the saved result through shared scenery.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [ROADMAP.md](ROADMAP.md).
