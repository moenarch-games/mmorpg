# Browser demo embeds the shared zone simulation

Status: accepted for the Greyhaven Vale starter-zone slice; implemented by step 6 (#21) for the currently hosted content.

Implementation notes (step 6):

- `mmorpg-wasm` hosts zone 1 with the same content as `mmorpg-zone-host` and plays the session role that `game-server`'s `MatchRuntime` plays for network hosts: never-reused player IDs from 1, sequence 0 rejected, stale sequences ignored. Its host-target tests check the same content and session outcomes against `MatchRuntime`, and byte-identical projections every tick while joined for a run-and-jump sequence and for the `browser-local-session` scenario's steps. Leaving removes the unit at once instead of after reconnect grace, so the away ticks are not compared.
- The workspace `unsafe_code = "forbid"` lint applies unchanged; the code `wasm-bindgen` 0.2.129 generates compiles under it.
- Step 6 landed first, with a scenery export that was a blockout of the hosted core colliders. Step 5 (#20) replaced it behind the same versioned format with a mapping of `mmorpg-scenery`'s Greyhaven Vale; the browser render loop did not change. Named areas are core content (`ZoneAreas`, `greyhaven_vale::areas()`).
- Local demo saves of world state are removed until the composed character/world save bundle and restore flow (#40) exists; #30 owns its durable character record prerequisite. Roster and appearance saves remain.

Implementation notes (step 14, #29):

- The online mode is the transport swap this decision kept open. `OnlineZoneSource` implements the same `WorldSource` contract over WebTransport against `mmorpg-zone-host`, with the native client's session rules; presentation is unchanged. Because a network admission needs a round trip, `WorldSource.join` became asynchronous for both sources, and a source reports whether its link is reconnecting.
- The browser implements the pinned `game-server` session frames and fragment reassembly in TypeScript. A shared fixture rendered from the Rust encoders (`fixtures/protocol/session-frames-v3.hex`) holds both to the same bytes, so a pin bump that changes a frame fails in both languages.
- The page still loads the WASM module in online mode: the scenery it draws comes from it, and every online projection must carry that content revision. The local zone stays the default, and the GitHub Pages build never joins a host.

## Context

The GitHub Pages demo moves a character with its own TypeScript rules in an illustrative scene. That was acceptable while the demo showed only presentation. A starter zone with combat, creature AI, quests, loot and progression cannot keep two rule owners (Rust core for hosts, TypeScript for Pages) without diverging (DESIGN-006). Pages cannot host a zone server, so the public demo cannot rely on a network host.

## Decision

Compile `mmorpg-core` and `mmorpg-protocol` to WebAssembly through a thin `mmorpg-wasm` adapter crate. In the browser it acts as a **local single-player zone host**:

- it constructs the same `ZoneSimulation` with the same content revision as `mmorpg-zone-host`;
- the browser sends encoded commands with increasing sequences and advances fixed 30 Hz ticks from a bounded accumulator;
- the browser reads only encoded **player-scoped** projections and decodes them with the same TypeScript decoder an online WebTransport source would use. Canonical snapshots never cross into rendering;
- presentation-only scenery (`mmorpg-scenery`) is exported through the same module so both clients draw identical worlds.

This is not MMO authority in the browser. It has no fencing, no leases, no handoff, no persistence authority and no shared world. It is the same single-writer simulation running in-process for one local player, like a listen server. The presentation layer cannot tell a local source from a network source, which keeps a future browser online mode a transport swap.

## Consequences

- There is one implementation of gameplay rules, and the browser demo shows the real simulation.
- The Pages build needs a Rust toolchain, the `wasm32-unknown-unknown` target and a pinned `wasm-bindgen` CLI. Generated WASM and bindings are build outputs, not committed (REP-003).
- `mmorpg-core` and `mmorpg-protocol` must keep compiling for `wasm32-unknown-unknown`: no threads, filesystem or wall clock in core paths. CI builds the WASM target.
- Browser tests that need the simulation build the WASM module first; pure decoder and UI tests stay independent.
- Local demo saves (#40) compose the durable character record (#30), accessed through core query/command APIs, with matching canonical zone/world checkpoints. They restore through those shared APIs, not by reaching into simulation internals.
- Online play from the browser needs a zone host the page can reach over WebTransport: a publicly trusted certificate, or for development a short-lived ECDSA certificate whose SHA-256 hash the page is given. It adds no MMO authority to the browser: the host alone runs the zone, and a browser session is as anonymous as a native one.
