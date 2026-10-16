# MMORPG roadmap

## Greyhaven Vale starter zone (tech demo)

Design contract and step plan: [docs/STARTER_ZONE.md](docs/STARTER_ZONE.md). Each step is a tracked issue and its own pull request.

- [x] Record the starter-zone design contract and the browser WASM-host decision ([ADR 0002](docs/adr/0002-browser-embeds-zone-simulation.md)).
- Refresh pinned foundations and dependencies.
- Fragment player snapshots above the datagram budget in `game-server`.
- [x] Movement v3: facing, camera-relative movement, jump.
- [x] Greyhaven Vale content, shared presentation scenery, projection budget.
- [x] Browser demo runs the shared simulation as a local WASM zone host.
- Replace placeholder characters with provenance-backed humanoid assets and start consuming the shared environment-asset pipeline ([#39](https://github.com/moritzbrantner/mmorpg/issues/39); upstream [asset-tooling#104](https://github.com/moritzbrantner/asset-tooling/issues/104), [#105](https://github.com/moritzbrantner/asset-tooling/issues/105), [#106](https://github.com/moritzbrantner/asset-tooling/issues/106)).
- Adopt shared `input-bindings` and `settings` foundations for gameplay controls, binding UI and precise player/device preferences ([#41](https://github.com/moritzbrantner/mmorpg/issues/41); upstream [input-bindings#53](https://github.com/moritzbrantner/input-bindings/issues/53), [settings#20](https://github.com/moritzbrantner/settings/issues/20)).
- [x] Units, combat and creature AI: core, protocol and minimal presentation (step 7a).
- Combat presentation in the browser: target frame, nameplates, combat text (step 7b).
- Classes and abilities.
- Progression, loot, inventory, equipment and vendors.
- [x] Quests and the Redbrand Hollow chain ([#25](https://github.com/moritzbrantner/mmorpg/issues/25)): core rules, the nine-quest Greyhaven chain, snapshot v14/command v9 and the browser dialog, log, tracker and markers.
- [x] Starter-zone combat workload evidence ([#26](https://github.com/moritzbrantner/mmorpg/issues/26)): fixed idle/distributed/crowded scripts, replay/recovery parity and deterministic work/byte ceilings.
- World presentation: relief, vegetation, water, sky, animated models, effects.
  - [x] Part A (browser): biome terrain and far ranges, procedural props and vegetation in static batches, lake, CSS sky, animated humanoids with class gear, camera polish, minimap, F3 statistics.
  - Part B: creature and NPC models, spell effects, selection circles, and fog, instancing, vertex colours and lighting once 3d-lab #82 and #84 are pinned.
- Native client parity.
- [x] Browser online mode against a local zone host.
- Durable character record behind core command/query APIs ([#30](https://github.com/moritzbrantner/mmorpg/issues/30)), consumed by the composed save/load flow below.
- Persist character progression together with versioned zone/world checkpoints so the demo can save, close, load and continue ([#40](https://github.com/moritzbrantner/mmorpg/issues/40)).
- Zone chat and emotes.

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

- Turn the in-memory reference model into a networked service boundary.
- [x] Add deterministic lease TTL/renewal and fail-closed expiry semantics to the reference model.
- [x] Add host registration/heartbeats and require live host registration for placement.
- [x] Put fencing-epoch advancement behind a provider-neutral compare-and-advance store contract; storage failure or concurrent allocation cannot acknowledge a lease.
- Back that contract with durable linearizable storage and persist active ownership/deadlines before enabling restart-driven failover.
- Keep the backing store/provider replaceable behind the control-plane contract.
- [x] Fence reference runtime operations against current directory authority and independent lease time.
- [x] Prove reassignment rejects old-owner commands, ticks, admission and publication.
- Add locally expiring permits and lease-aware WebTransport serving; the current binary remains standalone.
- Persist active deadlines as well as epoch floors and prove partition/crash behavior before failover automation.

## Cross-zone handoff

- Define the authoritative transferable player/character payload separately from connection identity.
- Prepare at a source tick boundary, admit idempotently at the destination, then retire the source.
- Route reconnects and retries through the same transfer ID.
- [x] Prove stale source/destination epochs cannot complete metadata transitions.
- [x] Keep transfer identity stable across lease renewal and reserve one active transfer per entity.
- Separate staged destination acceptance from active simulation and prove source retirement.
- Add explicit fenced cancellation/reconciliation and durable terminal-record retention.
- Add failure tests for source crash, destination crash and duplicate delivery at each phase.

## Interest management and crowded zones

- [x] Add a spatial index owned by the zone simulation.
- [x] Keep player-scoped projection authoritative in core.
- [x] Measure visibility work and snapshot bytes with deterministic sparse, dense and vale-spawn workloads.
- [x] Cap player projections by deterministic relevance priority within a measured single-datagram byte budget.
- Feed representative Greyhaven sparse/crowded workloads back into `physics-engine`, ratchet retained/delta-only maintenance there, and consume the proven revision back here ([physics-engine#190](https://github.com/moritzbrantner/physics-engine/issues/190)).
- Benchmark physics work independently in collision-heavy and crowded workloads.
- Add dynamic zone subdivision or instancing only when workload evidence shows the static-zone model is insufficient.

## Durable world state

- Separate account/character identity from zone-local connection/player IDs.
- Add command/query interfaces for durable character state.
- Add versioned checkpoint persistence for zone/world state and a durable save-slot bundle that composes character and world records ([#40](https://github.com/moritzbrantner/mmorpg/issues/40)).
- Keep persistence asynchronous to the hot simulation loop.
- Require each gameplay slice to classify new authoritative state as ephemeral, character-durable, world-durable, or static content so recovery completeness is continuously exercised.
- Do not introduce event sourcing unless replay/audit requirements justify it beyond existing game-server replay/recovery.

## Physics and gameplay

- [x] Install validated immutable static collision content through the shared physics engine.
- [x] Preserve velocity, gravity and collision content through canonical recovery.
- [x] Verify grounded/wall contact and airborne recovery continuation.
- [x] Load a shared revisioned outpost into the host and native renderer.
- [x] Replace it with the Greyhaven Vale content revision: colliders, clear spawn plaza, road corridors and named areas.
- Replace block/placeholder character presentation with provenance-backed humanoid mesh/material/animation assets through `asset-tooling`, while physics bodies remain authoritative ([#39](https://github.com/moritzbrantner/mmorpg/issues/39)).
- Introduce durable character identity before live cross-zone gameplay.
- Add server-owned interaction/ability rules with replay-complete cooldown, resource and target state.
- Measure collision-heavy and crowded-zone workloads before changing capacity.

## Client foundations

- [x] Reuse pinned `3d-lab` rendering primitives.
- [x] Decode player-visible snapshots with cross-language golden compatibility evidence.
- [x] Separate fixed demo ticks from interpolated render frames and bound presentation history.
- [x] Connect a native wgpu/winit client to the shared WebTransport session protocol.
- [x] Verify two clients share authoritative movement and key-release acknowledgements.
- [x] Resume native sessions with retained player identity, increasing sequences and presentation reset.
- Add account authentication and live zone routing; current sessions are anonymous.
- [x] Share immutable collision geometry and player dimensions between the server and native client.
- Extend this contract to authored visual assets and their provenance through `asset-tooling` ([#39](https://github.com/moritzbrantner/mmorpg/issues/39)).
- Consume pinned `3d-lab` procedural skeletal animation for two-bone IK, foot placement/locking, pelvis correction and surface-normal alignment against client-visible shared collision geometry; it remains presentation over server-owned movement and physics truth.
- Add bounded motion warping for interactions and attacks only from server-owned target/cue data; warped presentation must not change authoritative transforms, hits, cooldowns, recovery, zone handoff or replay state.
- Add optional shared-physics prediction and acknowledgement-based reconciliation.
- Reuse `input-bindings` for semantic actions/profiles and `settings` for the unified settings surface ([#41](https://github.com/moritzbrantner/mmorpg/issues/41)).
- Keep server-authoritative interest/projection limits out of ordinary client preferences; expose only safe client-side camera/presentation tuning or explicit diagnostics.
- Integrate social functionality with `social-service` only at the boundary that service actually owns.

## Desktop delivery

- [x] Native wgpu game window with interpolated authoritative snapshots.
- [x] Bounded network state, TLS trust, timeout and shutdown ownership.
- [x] One bounded automatic resume attempt on transport interruption, with cancellable shutdown.
- [x] Reproducible local host/client/GPU smoke harness.
- Add Tauri only when launcher, login, patching and settings workflows need a webview shell.
- Add platform packaging and Windows/macOS verification before distributing installers.
- Extend shared transport for snapshots larger than the negotiated datagram budget.
