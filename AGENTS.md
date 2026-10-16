# Agent Instructions

Server-authoritative MMORPG foundation that scales by distributing zone ownership, with deterministic physics and a separate client presentation layer.
Rust workspace + native wgpu client + non-authoritative browser demo (`web/`).

## Read first

- `README.md` — authority map and current state.
- `docs/ARCHITECTURE.md`, `docs/PROTOCOL.md`, `docs/INTEREST_WORKLOADS.md`, `docs/NATIVE_CLIENT.md`, `docs/SCENARIOS.md`, `docs/adr/`.

## Layout

| Crate | Role |
| --- | --- |
| `mmorpg-core` | Deterministic zone-local gameplay and interest/visibility policy |
| `mmorpg-protocol` | Command/snapshot wire encoding |
| `mmorpg-game-server` | Adapter from a zone simulation into `game-server` |
| `mmorpg-control-plane` | Placement, leases, fencing, handoff metadata (in-memory reference model) |
| `mmorpg-scenery` | Presentation-only scenery derived from core content (props, roads, water, relief, terrain grid); clients only (native client, `mmorpg-wasm`'s scenery export), never network hosts |
| `mmorpg-client` | Native wgpu/winit client |
| `mmorpg-scenarios` | Headless deterministic bot and control-plane scenario runners (tooling; owns no rules) |
| `mmorpg-wasm` | wasm-bindgen adapter: the shared zone simulation as a local single-player host for `web/` (owns no rules) |
| `web/` | GitHub Pages single-player tech demo over the WASM local host; no MMO authority |

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
cargo build -p mmorpg-core -p mmorpg-protocol -p mmorpg-wasm --target wasm32-unknown-unknown --locked
(cd web && bun install --frozen-lockfile && bun test && bun run build)
cargo run -p mmorpg-scenarios --locked -- bots crates/mmorpg-scenarios/scenarios/bots/*.toml                    # headless multiplayer bots, no GPU
cargo run -p mmorpg-scenarios --locked -- control-plane crates/mmorpg-scenarios/scenarios/control-plane/*.toml  # lease/handoff invariants
MMORPG_SCENARIOS_UPDATE=1 cargo test -p mmorpg-scenarios --test scenarios --locked                              # regenerate expected scenario outputs
python3 scripts/smoke-native.py      # offscreen end-to-end: host + client + GPU frame
./scripts/dev-native.sh              # full local native dev environment
python3 scripts/smoke-browser.py     # real Chromium against the built web/dist (needs Playwright)
```

`bun run build` and `bun run dev` first sync browser dependencies with `bun install --frozen-lockfile`. They and `bun test` compile `mmorpg-wasm` for `wasm32-unknown-unknown` (listed in `rust-toolchain.toml`) and run the `wasm-bindgen` CLI, whose version must equal the crate's exact `wasm-bindgen` pin: `cargo install wasm-bindgen-cli --version =0.2.129 --locked`. Generated bindings land in the ignored `web/src/generated/`.

## Authority

- `mmorpg-core` owns deterministic zone-local gameplay and interest/visibility policy.
- `physics-engine` owns collision, movement integration and physics semantics. Do not reimplement them here.
- `game-server` owns reusable transport/session/tick/reconnect/replay/recovery behavior. Keep MMO adapters thin.
- `mmorpg-control-plane` owns placement, leases, fencing and handoff metadata. It must not mutate zone gameplay state.
- `mmorpg-scenery` owns presentation scenery and relief derived from core colliders. Network hosts never depend on it (`mmorpg-game-server/tests/dependency_boundary.rs`), `mmorpg-wasm`'s local zone host never reads it, and it never feeds back into gameplay.
- Rendering comes from pinned `3d-lab`; input, settings, assets and social systems belong to `input-bindings`, `settings`, `asset-tooling` and `social-service`. Compose them; do not grow substitutes here.

## Distributed-world invariants

- A zone has exactly one active writer for a lease epoch.
- Never use eventual agreement as permission for two zone hosts to simulate the same authoritative zone.
- Every ownership-changing operation is fenced by the expected zone epoch.
- Cross-zone handoff is idempotent and identified independently from connection/session identity.
- Source authority is not retired until the destination has accepted the handoff.
- Do not distribute one physics step across machines. Partition the world at explicit authority boundaries.

## Determinism

- Stable IDs and ordered collections are preferred inside authoritative code.
- Commands with stale or duplicate sequence numbers fail closed.
- Canonical snapshots remain suitable for replay/recovery even when clients receive player-scoped projections.
- Performance changes need deterministic workload evidence; do not gate correctness on wall-clock timings.

## Architecture

- Prefer lightweight CQRS/CQS at service boundaries. Event sourcing is optional and requires concrete justification.
- Keep durable persistence outside the simulation hot loop.
- Do not pick a cloud/provider-specific scheduler, database or service mesh until the contract that needs it exists.
- External reusable foundations are pinned by git rev. Update pins intentionally with compatibility evidence.

## Direction

- Balance playable-game progress with the distributed foundation. Gameplay slices are welcome but must not break the ownership, fencing, handoff or determinism contracts above.

## Shared conventions

General engineering rules (git and merging, commits, testing, ADRs, docs, dependencies, Rust style, …) come from `coding-agent-conventions`, installed in `.conventions/`. Read the rule briefing in `.conventions/index.md` before implementing and open the linked source when a rule applies. Do not edit `.conventions/`; refresh it with `coding-tooling conventions update`. Rules below are repository-specific additions or exceptions.

## Shared foundations

- Shared foundations (`game-server`, `physics-engine`, `3d-lab`, …) are checked out beside this repo under `~/privat/`. Fix defects there and bump the pin here (DEP-003).

## Done means

- Format, Clippy, tests and build pass for the complete workspace (and `web/` if touched).
- Changes to server multiplayer, session or control-plane behavior add or update a scenario in `crates/mmorpg-scenarios/scenarios/` (see `docs/SCENARIOS.md`).
- Changes to client/server interaction pass `scripts/smoke-native.py`.
- Protocol changes keep the Rust/browser snapshot and command compatibility tests passing and update `docs/PROTOCOL.md`.
- `mmorpg-core` and `mmorpg-protocol` keep compiling for `wasm32-unknown-unknown`: no threads, filesystem or wall clock in core paths.
