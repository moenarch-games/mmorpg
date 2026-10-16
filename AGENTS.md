# Agent Instructions

Server-authoritative MMORPG foundation that scales by distributing zone ownership, with deterministic physics and a separate client presentation layer.
Rust workspace + native wgpu client + non-authoritative browser demo (`web/`).

## Read first

Read this entire file, `README.md` and `.conventions/index.md` for every task. Authority, distributed-world invariants, determinism and completion requirements below apply to every scope.

Decision Atlas: `docs/decisions/atlas.md` is the canonical product-intent map. Its semantic leaf entries are only Decisions and Questions. Retrieve relevant entries before making consequential product choices; follow Decisions, and ask the owner one unanswered Question at a time only when it materially affects the task. Preserve answered/superseded entries through linked append-only Decisions rather than rewriting history.

Use the existing `coding-tooling inspect` task lookup (coding-tooling #272) to select declared links and focused commands from `.coding-tooling.json`:

```sh
coding-tooling inspect --target web/src/world/world-view.ts --task-kind presentation --json
coding-tooling inspect --target crates/mmorpg-control-plane/src/lib.rs --json
coding-tooling inspect --target web/src/command-wire.ts --json
```

Presentation lookup retains the browser's local-host authority decision and relevant avatar/character documentation; protocol adapters also retain wire compatibility and producer checks. Native gameplay and control-plane scopes retain their architecture/workload/scenario links. Related owners are declared references, not evidence that their validation ran.

Lookup is read-only and does not replace the full completion gate or required native/WASM/browser acceptance below. If lookup is unavailable, partial or the relationship is undeclared, read `docs/ARCHITECTURE.md`, `docs/PROTOCOL.md`, `docs/INTEREST_WORKLOADS.md`, `docs/NATIVE_CLIENT.md`, `docs/SCENARIOS.md` and `docs/adr/`, then use the full workspace commands. Native crate lookup retains the existing root workspace commands because the capability catalog models the Cargo workspace as one component. Scope is a navigation aid, never permission to weaken authority or completion.

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
python3 scripts/smoke-browser-online.py  # two Chromium pages join one real zone host over WebTransport
./scripts/dev-browser-online.sh      # zone host + browser client in online mode
```

`bun test` and `bun run build` first compile `mmorpg-wasm` for `wasm32-unknown-unknown` (listed in `rust-toolchain.toml`) and run the `wasm-bindgen` CLI, whose version must equal the crate's exact `wasm-bindgen` pin: `cargo install wasm-bindgen-cli --version =0.2.129 --locked`. Generated bindings land in the ignored `web/src/generated/`.

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

## Execution scope

These rules govern how work is sliced and when expensive checks run. They never relax Authority, the distributed-world invariants, Determinism or Done means.

- **One task = one branch = one PR.** A task is one GitHub issue, typically a plan step (for example in `docs/STARTER_ZONE.md`) or an explicitly specified core or presentation half of one. Deliver the task's complete declared scope on one branch, including the core rules, protocol, server integration, presentations, scenarios and docs it requires, in small commits (GIT-007). Do not split a task into new issues or follow-up PRs on your own; if it cannot land as one PR, stop and propose the split on the issue instead of creating it.
- **Stay inside the task.** Do not start foundation, tooling, CI, pin-refresh, maintenance or budget work unless the task cannot be completed without it (DEP-003). Note unrelated findings as a TODO (REPO-010) or one line in the PR description; do not open issues for them.
- **No new ratchets unless the task asks for one.** Do not add size/performance budgets, baselines, evidence collectors or gates on your own initiative. Existing ratchets stay; when a task legitimately moves one, update its baseline in the same PR.
- **One format bump per task.** Settle command, snapshot and canonical format changes before implementing; a task bumps each version at most once.
- **Validate in tiers (TEST-015, GIT-005).** While iterating, run the focused commands that `coding-tooling inspect` selects for the touched scope. For verified Markdown-only PRs, `rust / validate` remains present but skips expensive commands; an empty/unknown diff, non-Markdown changes, and pushes to main retain full validation. GitHub Actions is the full gate: `validate.yml` runs format, Clippy, workspace tests (including scenario goldens), builds, the wasm32 build and the size budget; `pages.yml` runs Bun tests and the web build, and Chromium smoke when the PR carries the `browser-evidence` label. Before pushing, run locally only what CI does not cover: `scripts/smoke-native.py` for client/server interaction changes, and add the `browser-evidence` label for browser-visible changes. A red CI check blocks merge; fix it rather than re-proving it locally.
- **Compile Rust API updates before pushing.** For a Rust dependency update or API change, run `cargo check --workspace --all-targets --all-features --locked` locally before pushing, so feature-gated code and test-host adapters compile against the changed API. This is a narrow exception to the CI-first local-validation rule above; full Clippy, tests and builds remain required in CI.
- **Codex reviews are optional and risk-targeted.** Do not assume automatic PR review or request one for documentation, formatting, Renovate, or mechanical dependency changes. For substantive security, server-authority, storage, protocol, or architecture risk, the implementation agent may request one focused `@codex review` after validation. Missing, running, or stale review is not a merge gate; triage any actual findings by demonstrated impact, and resolve or explicitly answer high-priority findings. A new commit or ordinary fix alone never demands another review.
- **Decide and continue.** When a task leaves a design choice open, pick the simplest option consistent with this file, record it in the PR description (or an ADR when consequential, REPO-020) and keep going.
- **Short PR descriptions.** At most about 15 lines: what changed, format/compatibility changes, one line naming the gate steps that ran, and anything not verified (REPO-017). Leave detailed evidence to CI and the tests.

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
