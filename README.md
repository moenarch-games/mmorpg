# mmorpg

Server-authoritative MMORPG foundation that scales by distributing **zone ownership**, with deterministic physics and a separate client presentation layer.

The native Rust client now connects to the standalone zone host and renders server-authoritative multiplayer movement through wgpu. The GitHub Pages demo runs the same Rust zone simulation in the page as a local single-player WASM host. The in-memory control plane and fenced runtime establish tested contracts; they are not yet a production distributed fleet. See the [architecture capability map](docs/ARCHITECTURE.md#scope-and-current-state) for implemented behavior and deployment prerequisites.

## Authority map

| Concern | Authority |
| --- | --- |
| Zone-local gameplay rules and interest policy | `mmorpg-core` |
| Collision and physical movement | `physics-engine` |
| Command/snapshot wire encoding | `mmorpg-protocol` |
| Session ticks, reconnects, replay/recovery, WebTransport | `game-server` |
| Zone placement, lease fencing, host routing, handoff metadata | `mmorpg-control-plane` |
| Adapter from a zone simulation into `game-server` | `mmorpg-game-server` |
| Presentation scenery: props, roads, water, relief, terrain grid (clients only) | `mmorpg-scenery` |
| Native graphics | `mmorpg-client`: wgpu/winit adapter over pinned `3d-lab` mesh/camera models |
| Browser graphics | pinned `3d-lab` browser renderer |
| Browser zone host (single-player demo) | `mmorpg-wasm`: thin wasm-bindgen adapter over `mmorpg-core` and `mmorpg-protocol` |
| Runtime input semantics | `input-bindings` (future client slice) |
| User-facing settings | `settings` (future client slice) |
| Asset normalization/provenance | `asset-tooling` (future content slice) |
| Social systems | `social-service` where its existing authority fits |
| Runtime/performance evidence | `runtime-profiler` integration, without moving gameplay truth |

The MMO repository composes those foundations. It should not grow substitutes for them.

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
- [deterministic visibility workloads](docs/INTEREST_WORKLOADS.md) with wire parity and snapshot-size evidence;
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
- the Greyhaven Vale starter-zone content revision (hub, woods, farm, lake and hollow colliders, a validated spawn plaza, open road corridors, named areas, 59 creature spawns of seven templates, nine NPCs and a graveyard), hosted by the zone host and the browser's WASM local host, and rendered by both clients from presentation-only `mmorpg-scenery` (props, relief terrain, water) plus placeholder creature and NPC bodies;
- strict Rust/browser snapshot v5 and command v2 compatibility tests (self, target and event sections, compact priority-ordered unit records, facing, viewer identity, golden command bytes) and bounded client interpolation;
- player projections capped by deterministic relevance priority and packed by section priority into a measured single-datagram byte budget;
- native session resume with preserved player identity, command sequencing and connection-epoch resets;
- [headless deterministic scenario runners](docs/SCENARIOS.md) for scripted bots against the real zone host path, including a Wolfrun Woods hunt that fights, dies and releases its spirit, and for control-plane lease/handoff sequences with invariant checks after every step;
- architecture and roadmap documents that keep future persistence and orchestration choices replaceable.

The control-plane implementation in this slice is a **reference model**, not yet a production distributed consensus system. Host registration is placement eligibility only: a missed heartbeat blocks new assignment to that host but does not revoke an already-issued zone lease. Existing lease deadlines and fencing epochs remain the authority boundary. The model exists to make ownership, liveness-sensitive placement, epoch fencing and handoff idempotence executable before choosing storage or orchestration infrastructure.

## Native multiplayer client

The native client uses **wgpu 30.0.1 + winit 0.30.13**, shared Rust world geometry, and the existing WebTransport protocol. W/S run and backpedal, A/D or Q/E strafe relative to a third-person orbit camera (drag to orbit, wheel to zoom), Space jumps, Tab selects the nearest creature, F toggles auto-attack and R releases a dead spirit; the client only sends intent and the GPU renders interpolated player-visible snapshots of players, creatures and NPCs with their facing. The window title shows health and target. Multiple client processes can join the same zone.

```sh
python3 scripts/smoke-native.py --window
```

This builds the client and host, creates disposable development TLS credentials, checks server readiness, verifies an actual GPU frame, opens a window for 120 frames, and shuts everything down. Requires Python 3, OpenSSL and a supported graphics backend. Omit `--window` for the offscreen GPU check.

For persistent interactive play, see [native setup and controls](docs/NATIVE_CLIENT.md). Tauri is a possible future launcher shell; the gameplay window uses a native GPU surface. See [the decision](docs/adr/0001-native-client.md).

Start the complete local native development environment with:

```sh
./scripts/dev-native.sh
```

## Browser tech demo

`web/` is the GitHub Pages client. It runs the shared Rust zone simulation in the page as a **local single-player zone host** ([ADR 0002](docs/adr/0002-browser-embeds-zone-simulation.md)): the `mmorpg-wasm` crate compiles `mmorpg-core` and `mmorpg-protocol` to WebAssembly and builds the same `ZoneSimulation` and content revision as `mmorpg-zone-host` (zone 1, Greyhaven Vale). There is one implementation of the movement and combat rules; the browser owns none.

- **Enter World** joins the local zone and spawns the selected character; **Characters** (or Escape) leaves it and removes the unit. The next entry is a new player at the spawn. A projection the strict decoder rejects fails closed: entry is refused, or the page leaves the world, and the reason appears under **Enter World**.
- W/S run and backpedal, A/D (or Q/E) strafe relative to the orbit camera, Space jumps. A left drag looks around without turning the character, a right drag turns the character with the view (mouse-look), and the wheel zooms smoothly; the camera stays above the ground but collides with nothing else. F3 toggles frame rate, node counts and the renderer's work observations. The page encodes command wire v2 with strictly increasing sequences and resends the current intent like the native client.
- A `WorldSource` seam (`web/src/world/`) advances fixed 30 Hz ticks from a bounded accumulator and hands presentation only encoded player-scoped projections, decoded by the same `web/src/replication.ts` an online source would use. Canonical state never reaches rendering. An online WebTransport source (#29) plugs into the same seam.
- The world is drawn from a versioned `scenery()` export of the WASM module (format v2), which maps the same `mmorpg-scenery` Greyhaven Vale the native client draws. The browser builds it as a stylised low-poly valley: relief terrain in biome tones with smooth roads, plaza and shore, snow-capped distant ranges hazed toward the sky, procedural models for every prop (keep, timber-framed houses, palisade, windmill, camp, cliffs, oak, pine and birch trees, grass, flowers, reeds …) merged into a few hundred static batches, a translucent lake, and a CSS sky behind a transparent canvas. Structures' walls are their exact core colliders. Players are animated humanoids with class gear; other players share a neutral look because projections carry no appearance yet. A circular minimap shows the subzone name, roads, water, buildings and units.
- It is procedural presentation, not authored art: there are no textures, vertex colours, fog, dynamic lighting or day/night lighting yet, and haze and glows are baked into colours until the pinned renderer gains them (3d-lab #82, #84). Creature and NPC models and combat effects are not in this build yet. Units stand at their physics position plus the shared relief.
- Creatures and NPCs are placeholder bodies coloured by disposition (hostile, neutral, friendly), which their minimap dots share; corpses lie flat. Tab selects the nearest living creature, F or a right click (a right drag still turns) toggles auto-attack and R releases a dead spirit. The HUD text shows health, target and the latest combat feedback, with names from the module's versioned `catalog()` export. Target frames, nameplates and combat text follow in step 7b of #22.
- It has no fencing, leases, handoff, shared world or persistence authority. World position and progress are **not saved**: a durable character record is issue #30. Character roster and appearance stay in browser storage.

Building the page requires the Rust toolchain from `rust-toolchain.toml` (with the `wasm32-unknown-unknown` target) and the `wasm-bindgen` CLI at the crate's exact version: `cargo install wasm-bindgen-cli --version =0.2.129 --locked`. `bun run build` and `bun run dev` first sync browser dependencies from the committed lockfile, then compile the module. `bun test` also compiles the module before its WASM tests. The generated bindings are ignored build output. Update the Git checkout explicitly to build newer source; these scripts do not change source revisions or dependency pins.

The standalone host, the native client and the browser's WASM local host share the Rust Greyhaven Vale content (`mmorpg_core::greyhaven_vale`). Snapshot schema/wire version 5 carries the viewer's own state, its target's target, bounded feedback events and compact, priority-ordered unit records within a one-datagram byte budget, and command wire version 2 carries `Move`, `Jump`, `SelectTarget`, `StartAttack`, `StopAttack` and `ReleaseSpirit`; v4 snapshots, v1 commands and old recovery bundles require an explicit migration decision. See [the wire specification](docs/PROTOCOL.md).

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

Use lightweight CQRS/CQS at service boundaries: commands mutate authoritative durable state; queries read it. Do not introduce event sourcing by default. Zone hot loops must not synchronously depend on a distributed database. Durable character/world persistence and zone checkpoint storage are separate upcoming boundaries.

## Pinned foundations

- `physics-engine`: `1b98f84d409796b2a15b84f3fa4ed7f03a11f8bd`
- `game-server`: `a3851dab9c1fb25dd31b465fb554ca475769caab`
- `3d-lab` (`three-d-core`, `three-d-camera` and the browser `@moritzbrantner/three-d-renderer`, kept on one commit): `f484db8a3d2a7a555fa463eddf9c28790b240ce0`
- reusable validation workflow: `45042e56be120b438096e774027637cac0280075`

## Validation

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

Committed Rust and Bun lockfiles make local and CI resolution reproduce the same dependency graphs. Rust and browser tests both consume `fixtures/protocol/player-snapshot-v5.hex` and `fixtures/protocol/commands-v2.hex`.

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

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [ROADMAP.md](ROADMAP.md).
