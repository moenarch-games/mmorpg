# Seam corridors and fenced character transfer

Status: accepted; parts 1, 2 and 3a of #145 (owner decision Q5 = A: three sequential PRs; part 3 split into 3a, 3b and 3c with owner approval). Parts 3b (shared control plane, live host wiring) and 3c (client cutover, two-host journey) extend this record.

## Boundary

A seam between two logical zones is explicit content. Each side binds a `HandoffCorridor` (`ZoneContent::with_handoff_corridors`): the neighbouring `ZoneId`, an interaction-light XZ rectangle, a crossing strip inside it and the coordinate offset into the neighbour. Corridors are part of the content identity; content without corridors keeps its revision.

Inside a corridor, movement, facing and presentation continue, but nothing irreversible or economically meaningful begins: loot claims, vendor trades and quest accept/turn-in/abandon are refused with error code 35 (`HandoffCorridor`), and creatures evade the player's attacks, so no tap, kill credit, loot or boss reward starts there. Creatures never aggro a player in a corridor, and an engaged creature leashes home when its target stands in one or its body comes within `CORRIDOR_TURN_BACK_UNITS` (2 m) of one. Content validation keeps every creature's wander area, NPC, routine path, the graveyard and every player spawn slot clear of corridors (creatures and NPCs also of the margin), so no unit crosses a seam and no combat spans two zones.

## Transfer contract

Walking and teleporting share one contract, `CharacterTransfer`, identified by a `TransferId` independent of sessions and player IDs. `ZoneSimulation::seam_crossings` reports players in a crossing strip and where they continue; a teleport supplies its own destination.

1. **Freeze** (source, the cutover): the player leaves the live simulation and the source exports the durable `CharacterRecord` located in the destination's coordinates, the held movement and body velocity, and the last applied command sequence. Refused while climbing, rolling, or with this tick's reward grants not yet committed (TECH-005).
2. **Stage** (destination): validated against the destination content and held, not live. Idempotent per transfer ID; a different transfer under the same ID, or a second transfer of a staged character, is refused.
3. **Activate** (destination): the character becomes live and only commands after the cutover sequence apply, so stale and duplicate sequences fail closed. Repeated activation as the same player is a no-op.
4. **Retire** (source) once the destination accepted, or **abort** (source): the frozen player returns live and unchanged.

The zone holds at most one live copy of a character and never decides which side wins. The control plane's fenced handoff record does (part 2): a host activates only after the destination's acceptance is recorded and retires the source only after that. Frozen and staged transfers are host-phase state and are not canonical; recovery reconciles them from the handoff record and the character store. Session and tick-local state (target, queued intents, casts, auras, guard, combo, swing and roll timers) ends at the cutover, as at logout. No general action replay is introduced.

## Host-to-host transfer (part 2)

Three parties take part and none decides alone: each zone holds the live copy, the character store (`mmorpg-characters`) holds the lease and the in-flight transfer, and the control plane's fenced `HandoffRegistry` record decides the outcome. `mmorpg_game_server::handoff` composes them; every step is idempotent per transfer ID.

1. **Begin** (source): freeze, then `CharacterStore::begin_handoff` stores the frozen transfer under the source's lease, then the registry prepares the ticket. While a handoff is pending nobody commits, releases or acquires the character, not even a newer epoch of the source zone. A refused step undoes the earlier ones.
2. **Accept** (destination): stage, then record acceptance. **`Accepted` is the decision point**: before it the source may `abort` (a new `Aborted` phase) and stays authoritative; after it the transfer only completes. Then `complete_handoff` commits the transfer's record and grants the destination the lease in one atomic store step; the old lease is fenced at the store.
3. **Retire** (source): `commit` in the registry, then drop the frozen copy.
4. **Activate** (destination): only once the source committed, or the transfer is accepted and the source's zone lease is no longer current (a fenced host cannot restore an accepted transfer).

Recovery, at every phase:

- Not yet accepted (no record, or `Prepared`): the source aborts in the registry, cancels the store's pending handoff and unfreezes its player. Abort and acceptance race in the registry and exactly one wins; a late offer to the destination is refused and leaves nothing staged.
- Accepted or committed: the source retires; the destination completes the lease move and activates.
- Both hosts' host-phase state lost: the store still holds the pending transfer. Whoever next acquires the character runs `resolve_pending_handoff`: an accepted transfer completes to its destination; an unaccepted one is cancelled only by a newer epoch of the source zone, which proves the host that began it is fenced and can no longer prepare or accept it. The character then returns from the last committed record (source) or the transfer's record (destination), and a newer epoch supersedes the lease as for any crashed zone.

The registry's `commit` and `abort` accept the ticket's lease or a newer epoch of the same zone, which speaks for its fenced predecessor. `commit` no longer requires the destination's lease to be current: the source confirming its own retirement does not depend on the destination host.

Hosts reach each other over the `transfer_link` TCP link: length-prefixed offer and activate requests, each answered ok or refused. It is unauthenticated and unencrypted, for development and trusted LAN only (Q1 = A, ADR 0003); production transfer credentials are not claimed.

Formats: `mmorpg-protocol` encodes `CharacterTransfer` (`CHARACTER_TRANSFER_WIRE_VERSION`, a random token). The character entry gains the pending handoff and the last completed handoff with its lease generation; `ENTRY_FORMAT_VERSION` became a random token, so older entries are refused (no migration, pre-1.0).

## Test zones and waystones (part 3a)

Owner decision Q2 (revised): the live handoff is proven between two small dedicated logical test zones, Seam Test West (zone 901) and Seam Test East (902) (`mmorpg_core::seam_test`). They share one flat layout; East coordinates are West's shifted 100 m toward −X, and each crossing strip lands a walker inside the other side's corridor but short of its crossing, so nobody is handed straight back. Greyhaven Vale keeps its geometry and has no seam into them; its outpost waystone reaches them by teleport only. Hosts pick content by zone ID (`content_for_zone`): 901 and 902 are the test zones, every other ID Greyhaven.

Waystones (Q4 = A) form one global network (`mmorpg_core::WAYSTONES`): a stable ID, name, logical zone, stone and arrival point per stone; content binds the stones that stand in it. A living player within 8 m of a bound stone unlocks it for good. The unlocks are a durable character field, canonical player state and six spare bits of the projection's self flags, which caps the network at six stones until a wire change grows it. From within 8 m of a stone, travel to an unlocked stone is instant, with no cast or cooldown, and is refused in combat (error 36), in a corridor (35) and toward a locked, unknown or current stone (37). Travel within a zone moves the player there. Travel to a stone of another zone uses the same fenced transfer as walking. Until part 3b wires hosts to drive that transfer, the zone refuses it with error 37.

Formats: command tag 30 (`UseWaystone`), canonical intent 23, error codes 36 and 37, a new protocol ID, and a new random `CHARACTER_RECORD_SCHEMA_VERSION` token: stored characters of the old schema are refused, with no migration.

## Not covered here

Client reconnect, live-host wiring of these steps into `game-server` sessions (walking seams and cross-zone waystone travel) and the two-host journey are parts 3b and 3c. The test zones bind no quests, so a character carrying Greyhaven quest state fails the destination's record validation; 3b has to settle how quest logs cross zones. The handoff registry remains the in-memory reference model; persisting its records in PostgreSQL and a retention policy for terminal records are open.
