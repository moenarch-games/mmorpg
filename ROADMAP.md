# MMORPG roadmap

## Greyhaven Vale starter zone (tech demo)

Design contract and step plan: [docs/STARTER_ZONE.md](docs/STARTER_ZONE.md). Each step is a tracked issue and its own pull request.

- [x] Record the starter-zone design contract and the browser WASM-host decision ([ADR 0002](docs/adr/0002-browser-embeds-zone-simulation.md)).
- [x] Refresh foundations and dependencies: the foundations now follow their `main` branch, recorded by the lockfiles (Atlas 7.2.i1126).
- [x] Fragment player snapshots above the datagram budget in `game-server`.
- [x] Movement v3: facing, camera-relative movement, jump.
- [x] Greyhaven Vale content, shared presentation scenery, projection budget.
- [x] Browser demo runs the shared simulation as a local WASM zone host.
- [x] Replace placeholder characters with provenance-backed humanoid assets and start consuming the shared environment-asset pipeline ([#39](https://github.com/moenarch-games/mmorpg/issues/39)). Every class and sex still uses the one Ranger figure.
- [x] Adopt shared `input-bindings` and `settings` foundations for gameplay controls, binding UI and precise player/device preferences ([#41](https://github.com/moenarch-games/mmorpg/issues/41)).
- [x] Units, combat and creature AI: core, protocol and minimal presentation (step 7a).
- Combat presentation in the browser (step 7b): the target frame and combat text are done; nameplates remain.
- [x] Classes and abilities.
- [x] Progression, loot, inventory, equipment and vendors.
- [x] Quests and the Redbrand Hollow chain ([#25](https://github.com/moenarch-games/mmorpg/issues/25)): core rules, the Greyhaven chain and the browser dialog, log, tracker and markers.
- [x] Starter-zone combat workload evidence ([#26](https://github.com/moenarch-games/mmorpg/issues/26)): fixed idle/distributed/crowded scripts, replay/recovery parity and deterministic work/byte ceilings.
- [x] World presentation: relief, vegetation, water, sky, animated models, effects.
  - [x] Part A (browser): biome terrain and far ranges, props and vegetation in static batches, lake, CSS sky, animated humanoids, camera polish, minimap, F3 statistics.
  - [x] Part B: creature and NPC models, spell effects, selection circles, fog, instancing, vertex colours and lighting.
- [x] Native client parity ([#183](https://github.com/moenarch-games/mmorpg/issues/183)).
- [x] Browser online mode against a local zone host.
- [x] Durable character record behind core command/query APIs ([#30](https://github.com/moenarch-games/mmorpg/issues/30)), consumed by the composed save/load flow below.
- [x] Persist character progression together with versioned zone/world checkpoints so the demo can save, close, load and continue ([#40](https://github.com/moenarch-games/mmorpg/issues/40)).
- [x] Zone chat and emotes.

## Foundation — distributed world authority

- [x] Define zone-local deterministic authority around `physics-engine`.
- [x] Adapt zone authority to `game-server::GameSimulation`.
- [x] Use player-scoped snapshots so interest policy stays outside transport.
- [x] Define fenced zone leases and deterministic host routing.
- [x] Define idempotent cross-zone handoff metadata.
- [x] Commit the generated workspace `Cargo.lock` after bootstrap dependency resolution.

## Runnable zone host

- [x] Add a zone-host binary around `game-server::MatchHost`.
- [x] Prove multiple zones advance independently in one process.
- [x] Expose health/readiness/status without making status endpoints gameplay authorities.
- [x] Keep one listener/routing surface per host rather than one process per player or one port per zone.
- [x] Recover the complete hosted zone set atomically across graceful restarts.

## Distributed control plane

- Turn the in-memory reference model into a networked service boundary. Hosts now share a PostgreSQL directory; there is no placement service that assigns zones across hosts.
- [x] Add deterministic lease TTL/renewal and fail-closed expiry semantics to the reference model.
- [x] Add host registration/heartbeats and require live host registration for placement.
- [x] Put fencing-epoch advancement behind a provider-neutral compare-and-advance store contract; storage failure or concurrent allocation cannot acknowledge a lease.
- [x] Back that contract with durable linearizable storage and persist active ownership/deadlines before enabling restart-driven failover (PostgreSQL directory, TECH-001).
- Keep the backing store/provider replaceable behind the control-plane contract.
- [x] Fence reference runtime operations against current directory authority and independent lease time.
- [x] Prove reassignment rejects old-owner commands, ticks, admission and publication.
- [x] Add locally expiring permits and lease-aware WebTransport serving: the PostgreSQL persistence and handoff host modes; the open and development hosts still take no lease.
- Persist active deadlines as well as epoch floors and prove partition/crash behavior before failover automation. Deadlines and epoch floors are persisted; failover automation does not exist.

## Cross-zone handoff

- [x] Define the authoritative transferable player/character payload separately from connection identity.
- [x] Prepare at a source tick boundary, admit idempotently at the destination, then retire the source.
- [x] Route reconnects and retries through the same transfer ID.
- [x] Prove stale source/destination epochs cannot complete metadata transitions.
- [x] Keep transfer identity stable across lease renewal and reserve one active transfer per entity.
- [x] Separate staged destination acceptance from active simulation and prove source retirement.
- Add explicit fenced cancellation/reconciliation and durable terminal-record retention. Fenced abort, reconciliation and PostgreSQL handoff records are done; terminal-record retention remains.
- [x] Add failure tests for source crash, destination crash and duplicate delivery at each phase.
- Run handoff and crash-durable zone persistence in the same host; today they are separate host modes.

## Interest management and crowded zones

- [x] Add a spatial index owned by the zone simulation.
- [x] Keep player-scoped projection authoritative in core.
- [x] Measure visibility work and snapshot bytes with deterministic sparse, dense and vale-spawn workloads.
- [x] Cap player projections by deterministic relevance priority within a measured single-datagram byte budget.
- [x] Feed representative Greyhaven sparse/crowded workloads back into `physics-engine`, ratchet retained/delta-only maintenance there, and consume the proven revision back here ([physics-engine#190](https://github.com/moritzbrantner/physics-engine/issues/190)).
- Benchmark physics work independently in collision-heavy and crowded workloads.
- Add dynamic zone subdivision or instancing only when workload evidence shows the static-zone model is insufficient.

## Durable world state

- Separate account/character identity from zone-local connection/player IDs. Durable `CharacterId`s are separate from session player IDs; accounts do not exist.
- [x] Add command/query interfaces for durable character state.
- [x] Add versioned checkpoint persistence for zone/world state and a durable save-slot bundle that composes character and world records ([#40](https://github.com/moenarch-games/mmorpg/issues/40)).
- [x] Keep persistence asynchronous to the hot simulation loop.
- Require each gameplay slice to classify new authoritative state as ephemeral, character-durable, world-durable, or static content so recovery completeness is continuously exercised.
- Do not introduce event sourcing unless replay/audit requirements justify it beyond existing game-server replay/recovery.

## Physics and gameplay

- [x] Install validated immutable static collision content through the shared physics engine.
- [x] Preserve velocity, gravity and collision content through canonical recovery.
- [x] Verify grounded/wall contact and airborne recovery continuation.
- [x] Load a shared revisioned outpost into the host and native renderer.
- [x] Replace it with the Greyhaven Vale content revision: colliders, clear spawn plaza, road corridors and named areas.
- [x] Replace block/placeholder character presentation with provenance-backed humanoid mesh/material/animation assets through `asset-tooling`, while physics bodies remain authoritative ([#39](https://github.com/moenarch-games/mmorpg/issues/39)).
- [x] Introduce durable character identity before live cross-zone gameplay.
- [x] Add server-owned interaction/ability rules with replay-complete cooldown, resource and target state.
- Measure collision-heavy and crowded-zone workloads before changing capacity.

## Client foundations

- [x] Reuse `3d-lab` rendering primitives.
- [x] Decode player-visible snapshots with cross-language golden compatibility evidence.
- [x] Separate fixed demo ticks from interpolated render frames and bound presentation history.
- [x] Connect a native wgpu/winit client to the shared WebTransport session protocol.
- [x] Verify two clients share authoritative movement and key-release acknowledgements.
- [x] Resume native sessions with retained player identity, increasing sequences and presentation reset.
- Add account authentication and live zone routing; sessions are anonymous or claim characters without authentication.
- [x] Share immutable collision geometry and player dimensions between the server and native client.
- [x] Extend this contract to authored visual assets and their provenance through `asset-tooling` ([#39](https://github.com/moenarch-games/mmorpg/issues/39)).
- Consume `3d-lab` procedural skeletal animation for two-bone IK, foot placement/locking, pelvis correction and surface-normal alignment against client-visible shared collision geometry; it remains presentation over server-owned movement and physics truth.
- Add bounded motion warping for interactions and attacks only from server-owned target/cue data; warped presentation must not change authoritative transforms, hits, cooldowns, recovery, zone handoff or replay state.
- Add optional shared-physics prediction and acknowledgement-based reconciliation. Own-movement prediction exists as an arena netcode option (`mmorpg_core::prediction`, #680); it is not the default.
- [x] Reuse `input-bindings` for semantic actions/profiles and `settings` for the unified settings surface ([#41](https://github.com/moenarch-games/mmorpg/issues/41)).
- Keep server-authoritative interest/projection limits out of ordinary client preferences; expose only safe client-side camera/presentation tuning or explicit diagnostics.
- Integrate social functionality with `social-service` only at the boundary that service actually owns.

## Desktop delivery

- [x] Native wgpu game window with interpolated authoritative snapshots.
- [x] Bounded network state, TLS trust, timeout and shutdown ownership.
- [x] One bounded automatic resume attempt on transport interruption, with cancellable shutdown.
- [x] Reproducible local host/client/GPU smoke harness.
- Add Tauri only when launcher, login, patching and settings workflows need a webview shell.
- Add platform packaging and Windows/macOS verification before distributing installers. `scripts/package-native.py` packages a Linux build; Windows and macOS are unverified.
- [x] Extend shared transport for snapshots larger than the negotiated datagram budget.
