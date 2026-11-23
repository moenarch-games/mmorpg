# mmorpg

Server-authoritative MMORPG foundation that scales by distributing **zone ownership**, with deterministic physics and a separate client presentation layer.

## Current state

- **Playable:** the Greyhaven Vale starter zone with classes, abilities, creatures, quests, loot, vendors, chat and durable characters, played through the native wgpu client against a zone host, or through the GitHub Pages browser demo, which runs the same Rust zone simulation in the page as a local single-player host ([ADR 0002](docs/adr/0002-browser-embeds-zone-simulation.md)).
- **Development and LAN hosting:** a standalone zone host, the native client's **Host World** (a local host with invitations for native guests), browser online mode over WebTransport, and two PostgreSQL-backed host modes: crash-durable zone persistence and trusted-LAN cross-zone handoff. The two PostgreSQL modes cannot be combined in one host today.
- **Not production:** there is no account authentication, no public fleet routing and no production deployment acceptance. The control plane's lease, fencing and handoff rules are tested contracts, not a deployed distributed fleet.

The [architecture capability map](docs/ARCHITECTURE.md#scope-and-current-state) lists what is implemented and what remains. Product intent and decision history live in the [Decision Atlas](docs/decisions/atlas.md); open work is in the [roadmap](ROADMAP.md).

## Authority map

| Concern | Authority |
| --- | --- |
| Zone-local gameplay rules and interest policy | `mmorpg-core` (over `sim-foundation`, `sim-movement`, `sim-combat`) |
| Collision and physical movement | Physics Engine (`physics-engine`); Rapier only as the optional `physics-ab` A/B oracle ([physics backends](docs/PHYSICS_AB.md)) |
| Command/snapshot wire encoding | `mmorpg-protocol` |
| Session ticks, reconnects, replay/recovery, WebTransport | `game-server` |
| Zone placement, lease fencing, handoff metadata, party and instance directories | `mmorpg-control-plane` |
| Durable character records and exclusive character leases | `mmorpg-characters` (file entries, browser `localStorage`, optional PostgreSQL) |
| Adapter from a zone simulation into `game-server`; host modes | `mmorpg-game-server` |
| Presentation scenery and shared client presentation (clients only) | `mmorpg-scenery`, `mmorpg-presentation`, `mmorpg-client-model` |
| Native graphics | `mmorpg-client`: wgpu/winit with `render-engine` and `3d-lab` geometry/camera |
| Browser graphics | `3d-lab`'s Three.js renderer adapter |
| Browser zone host (single-player demo) | `mmorpg-wasm`: thin wasm-bindgen adapter, owns no rules |
| Runtime input semantics | `input-bindings` (both clients) |
| User-facing settings | `settings` (both clients) |
| Asset normalization and provenance | `asset-tooling`: the bundles both clients build from, and the saved authoring packages |
| Social systems | `social-service` where its existing authority fits |
| Runtime/performance evidence | `runtime-profiler` integration, without moving gameplay truth |

The repository composes those foundations and does not grow substitutes for them. The foundations and Battle Royale are followed at their `main` branch, not pinned to a revision (Atlas 7.2.i1126); `Cargo.lock` and `web/bun.lock` record the commit a build uses. See [ownership and dependency direction](docs/ARCHITECTURE.md#ownership-and-dependency-direction) and the [rendering and asset adoption record](docs/ARCHITECTURE.md#rendering-and-asset-provider-adoption).

## Build

[`mise.toml`](mise.toml) lists the tools (Bun, Python with uv, cargo-nextest, gh, jq, ripgrep); Rust comes from rustup through `rust-toolchain.toml`, including the `wasm32-unknown-unknown` target.

```sh
mise install && mise run setup   # tools, Rust toolchain, Playwright, asset bundles
scripts/fetch-bundles.sh         # the asset-tooling bundles both clients build from; run before any client build
```

The routine checks:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo build -p mmorpg-core -p mmorpg-protocol -p mmorpg-wasm --target wasm32-unknown-unknown --locked
(cd web && bun install --frozen-lockfile && bun test && bun run build)
```

[AGENTS.md](AGENTS.md#commands) lists every command, including scenarios, fixtures, smoke tests and profiling.

## Run the native client

```sh
./scripts/dev-native.sh                  # local zone host + client, rebuilt on every source edit
./scripts/run-native.sh                  # optimised standalone game in dist/mmorpg-native; Host World starts its own host
python3 scripts/smoke-native.py --window # offscreen host + client + GPU frame check, then a short window
```

Setup, controls, Host World, LAN play and the native limits are in [the native client guide](docs/NATIVE_CLIENT.md); camera and aiming rules are in [controls](docs/CONTROLS.md).

## Run the browser client

```sh
(cd web && bun run dev)              # local single-player page on the WASM zone host
./scripts/dev-browser-online.sh      # zone host + page; browser tabs join one real zone over WebTransport
```

The browser walkthrough, online mode and LAN play are in [the browser client guide](docs/BROWSER_CLIENT.md).

## Run a zone host

```sh
MMORPG_CERT_PEM=cert.pem MMORPG_KEY_PEM=key.pem \
  cargo run --locked -p mmorpg-game-server --bin mmorpg-zone-host
```

[Zone host modes](docs/NATIVE_CLIENT.md#zone-host-modes) explains the configuration, the status endpoints and which modes acquire control-plane leases.

## Documentation

- Architecture and contracts: [architecture](docs/ARCHITECTURE.md), [protocol](docs/PROTOCOL.md), [zone lifecycle](docs/ZONE_LIFECYCLE.md), [ADRs](docs/adr/), [physics backends](docs/PHYSICS_AB.md), [static collision](docs/STATIC_COLLISION.md).
- Gameplay: [Greyhaven Vale starter zone](docs/STARTER_ZONE.md), [progression](docs/PROGRESSION.md), [inventory](docs/INVENTORY.md), [loot](docs/LOOT.md), [PvP combat](docs/PVP_COMBAT.md), [Battle Royale](docs/BATTLE_ROYALE.md), [ephemeral characters](docs/EPHEMERAL_CHARACTERS.md), [save and load](docs/SAVE_LOAD.md).
- Evidence: [scenarios](docs/SCENARIOS.md), [interest](docs/INTEREST_WORKLOADS.md), [combat](docs/COMBAT_WORKLOADS.md) and [presentation](docs/PRESENTATION_WORKLOADS.md) workloads.
- World authoring: [World Studio](docs/WORLD_STUDIO.md), [Blender layout](docs/WORLD_LAYOUT_EDITOR.md), [continent atlas](docs/WORLD_ATLAS.md), [relief](docs/RELIEF_AUTHORING.md), [scatter](docs/SCATTER_AUTHORING.md), [terrain patches](docs/TERRAIN_PATCHES.md), [walk-in buildings](docs/WALK_IN_BUILDINGS.md).
- Assets: [avatars](docs/AVATAR_ASSETS.md), [Ranger](docs/RANGER_ASSETS.md), [Warden ability art](docs/WARDEN_ABILITY_ART.md).
