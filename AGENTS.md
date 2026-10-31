# Agent Instructions

Server-authoritative MMORPG foundation that scales by distributing zone ownership, with deterministic physics and a separate client presentation layer.
Rust workspace + native wgpu client + non-authoritative browser demo (`web/`).

## Read first

Read this entire file, `README.md` and `.conventions/index.md` for every task. Authority, distributed-world invariants, determinism and completion requirements below apply to every scope.

Decision Atlas: `docs/decisions/atlas.md` is the canonical product-intent map. Its semantic leaf entries are only Decisions and Questions. Retrieve relevant entries before making consequential product choices; follow Decisions, and ask the owner one unanswered Question at a time only when it materially affects the task. Preserve answered/superseded entries through linked append-only Decisions rather than rewriting history. New entries take an issue-derived ID (parent ID + `i` + issue number, e.g. `5.9.i415`; `-2`, `-3` for further entries of the same issue under that parent) instead of the next sequential number, so parallel PRs never collide; existing numeric IDs never change.

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
| `mmorpg-control-plane` | Placement, leases, fencing, handoff metadata and logical-zone partition maps (in-memory reference model) |
| `mmorpg-characters` | Durable character records under exclusive fenced character leases; storage-backend neutral, WASM-compatible |
| `mmorpg-scenery` | Presentation-only scenery derived from core content (props, roads, water, relief, terrain grid); clients only (native client, `mmorpg-wasm`'s scenery export), never network hosts |
| `mmorpg-presentation` | Shared procedural presentation drawn by both clients: static world (terrain, roads, water, props) and units (models, poses, effects, imported-model clip choice); renderer-neutral, WASM-compatible, clients only |
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
cargo test -p mmorpg-core --features physics-ab --test physics_ab --locked                                     # Rapier vs physics-engine A/B invariants (docs/PHYSICS_AB.md)
cargo run --release -p mmorpg-core --example physics_ab --features physics-ab --locked                           # manual A/B physics timing
cargo run -p mmorpg-scenarios --locked -- bots crates/mmorpg-scenarios/scenarios/bots/*.toml                    # headless multiplayer bots, no GPU
cargo run -p mmorpg-scenarios --locked -- control-plane crates/mmorpg-scenarios/scenarios/control-plane/*.toml  # lease/handoff invariants
MMORPG_SCENARIOS_UPDATE=1 cargo test -p mmorpg-scenarios --locked                                              # regenerate expected scenario outputs
MMORPG_UPDATE_FIXTURES=1 cargo nextest run --workspace --locked                                                 # regenerate every golden fixture (protocol hex, traces, scenarios); review the diff
python3 scripts/smoke-native.py      # offscreen end-to-end: host + client + GPU frame
python3 scripts/smoke-handoff.py     # two handoff hosts on PostgreSQL 17 (Docker): the native autopilot walks and teleports between them (#145)
./scripts/dev-native.sh              # full local native dev environment
python3 scripts/smoke-browser.py     # real Chromium against the built web/dist (needs Playwright)
python3 scripts/smoke-browser-online.py  # two Chromium pages join one real zone host over WebTransport
./scripts/dev-browser-online.sh      # zone host + browser client in online mode
```

`bun run build` and `bun run dev` first sync browser dependencies with `bun install --frozen-lockfile`. They and `bun test` compile `mmorpg-wasm` for `wasm32-unknown-unknown` (listed in `rust-toolchain.toml`) and run the `wasm-bindgen` CLI, whose version must equal the crate's exact `wasm-bindgen` pin: `cargo install wasm-bindgen-cli --version =0.2.129 --locked`. Generated bindings land in the ignored `web/src/generated/`.

## Authority

- `mmorpg-core` owns deterministic zone-local gameplay and interest/visibility policy.
- Rapier (`rapier3d`, embedded in `mmorpg-core/src/physics.rs`, Atlas 2.1.6, #271) resolves collision and character motion: units are capsules moved by its `KinematicCharacterController`, and f32 positions/velocities are authoritative (rounded only on the wire). `mmorpg-core` keeps movement intent, gravity/jumps and every gameplay outcome. Do not grow a second solver; record each Rapier gap or workaround for physics-engine issues. physics-engine is compiled only behind the test/bench `physics-ab` feature for the A/B comparison (Atlas 2.1.7); never enable it in a production or WASM build.
- `game-server` owns reusable transport/session/tick/reconnect/replay/recovery behavior. Keep MMO adapters thin.
- `mmorpg-control-plane` owns placement, leases, fencing and handoff metadata. It must not mutate zone gameplay state.
- `mmorpg-scenery` owns presentation scenery and relief derived from core colliders. Network hosts never depend on it (`mmorpg-game-server/tests/dependency_boundary.rs`), `mmorpg-wasm`'s local zone host never reads it, and it never feeds back into gameplay.
- Use mature external rendering and asset-import providers until internal tools demonstrate equivalent capability on the game’s actual acceptance scenarios. Existing working `3d-lab` mesh/camera contracts may remain, but missing internal capabilities must not automatically become prerequisites for playable-game fixes. Compare correctness, visual quality and resource use before adopting or expanding an internal provider. Keep one implementation owner per capability; do not grow a competing generic engine/parser here.
- Input, settings, asset authoring/provenance and social systems retain their `input-bindings`, `settings`, `asset-tooling` and `social-service` boundaries. A declared internal ownership map is not proof of runtime adoption or capability.

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

These rules govern task scope and verification effort. The owner’s happy-path policy below controls validation frequency; Authority, distributed-world invariants and Determinism remain product requirements.

- **One task = one branch = one PR.** A task is one GitHub issue, typically a plan step (for example in `docs/STARTER_ZONE.md`) or an explicitly specified core or presentation half of one. Deliver the task's complete declared scope on one branch, including the core rules, protocol, server integration, presentations, scenarios and docs it requires, in small commits (GIT-007). Do not split a task into new issues or follow-up PRs on your own; if it cannot land as one PR, stop and propose the split on the issue instead of creating it.
- **Stay inside the task.** Do not start foundation, tooling, CI, pin-refresh, maintenance or budget work unless the task cannot be completed without it (DEP-003). Note unrelated findings as a TODO (REPO-010) or one line in the PR description; do not open issues for them.
- **No new ratchets unless the task asks for one.** Do not add size/performance budgets, baselines, evidence collectors or gates on your own initiative. Existing ratchets stay; when a task legitimately moves one, update its baseline in the same PR.
- **No hand-bumped version numbers** ([ADR 0006](docs/adr/0006-merge-safe-format-identities.md)). The content revision is a hash of the content, so content changes need no edit. A change to the command, projection, observation or canonical layout runs `scripts/new-protocol-id.sh` once (a fresh random protocol ID in Rust and the browser), then re-records fixtures with `MMORPG_UPDATE_FIXTURES=1`. Never add a counter for compatibility; if a merge conflicts on the protocol ID line, rebase and run the script again.
- **Happy-path development is the default (owner decision, Atlas 7.1.5).** Implement the requested user-visible outcome and run one inexpensive relevant check. A successful existing build or direct happy-path run can be sufficient; do not manufacture an acceptance project, independent worker handoff, exhaustive edge-case matrix, paired image gallery, benchmark or upstream prerequisite for routine work. Stop checking once the requested path works.
- **Escalate only for a serious observed breakage or an explicit owner request.** A crash, inability to start/use the requested path, data corruption, authority/security failure or comparable actual failure can justify targeted diagnosis. A speculative concern, unrelated test failure, cosmetic difference or flaky fixture does not authorize widening the task. Fix the evidenced cause with the smallest relevant check, then return to delivery; do not turn diagnosis into repeated full-suite runs.
- **Expensive checks are opt-in, locally and in CI.** Full workspace/all-target/all-feature suites, Clippy, scenario matrices, native/browser acceptance, repeated screenshots, asset reproduction and size/performance audits are not routine prerequisites. Existing tests and assertions remain available; do not delete or weaken them. For Rust-related changes, normal CI uses format, `cargo check --workspace --locked` and `cargo test --workspace --locked` (GPU tests stay `#[ignore]`d; owner decision 2026-10-18). Clippy stays opt-in for PRs but is **enforced at merge time**: the merge queue runs `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` beside the tests for Rust changes, and it feeds the required `merge-ready` check (owner decision 2026-10-25); documentation/workflow-only edits skip Rust compilation and its cache restore; Pages builds remain available for preview/publication without comprehensive tests. Select `full-validation` on a PR or the `full_validation` workflow-dispatch input to run comprehensive validation. `browser-evidence` explicitly selects browser acceptance only. Do not add these labels automatically for ordinary changes. Do not duplicate CI locally or repeat a completed check without a concrete reason.
- **This owner policy overrides older verification mandates.** It supersedes conflicting instructions in shared conventions, skills, old issue handoffs and acceptance documents, including mandatory tests-first/independent acceptance for every behavioral change and the earlier all-target/all-feature pre-push requirement. Keep honest reports of what ran, failures and untested scope; satisfy configured branch protection without claiming an unrun check passed.
- **Codex reviews are optional and risk-targeted.** Do not assume automatic PR review or request one for documentation, formatting, Renovate, or mechanical dependency changes. For substantive security, server-authority, storage, protocol, or architecture risk, the implementation agent may request one focused `@codex review` after validation. Missing, running, or stale review is not a merge gate; triage any actual findings by demonstrated impact, and resolve or explicitly answer high-priority findings. A new commit or ordinary fix alone never demands another review.
- **Decide and continue.** When a task leaves a design choice open, pick the simplest option consistent with this file, record it in the PR description (or an ADR when consequential, REPO-020) and keep going.
- **Short PR descriptions.** At most about 15 lines: what changed, format/compatibility changes, one line naming the gate steps that ran, and anything not verified (REPO-017). Leave detailed evidence to CI and the tests.

## Shared conventions

General engineering rules (git and merging, commits, testing, ADRs, docs, dependencies, Rust style, …) come from `coding-agent-conventions`, installed in `.conventions/`. Read the rule briefing in `.conventions/index.md` before implementing and open the linked source when a rule applies. Do not edit `.conventions/`; refresh it with `coding-tooling conventions update`. Rules below are repository-specific additions or exceptions.

## Shared foundations

- Shared foundations (`game-server`, `3d-lab`, …) are checked out beside this repo under `~/privat/`. Fix defects there and bump the pin here (DEP-003). For rendering/asset-import gaps, apply the owner’s external-provider adoption rule above before starting upstream work; extending an unproven internal tool is not the default way to unblock the game.

## Done means

- Deliver the requested behavior and report the one focused check used to confirm it, plus any known remaining problem.
- Do not make unrelated findings or comprehensive verification a delivery prerequisite. Apply the happy-path policy above instead of older blanket full-suite requirements.
- Keep documentation and any directly affected protocol/scenario examples consistent with the change. Authoritative gameplay, fencing, determinism and core WASM portability remain requirements, without automatically running every available suite for every task.
