# Seam corridors and fenced character transfer

Status: accepted; part 1 of #145 (owner decision Q5 = A: three sequential PRs). Parts 2 (trusted-LAN host-to-host transfer, epoch fencing, recovery) and 3 (native/browser cutover, walking/teleport journey, exploration waystones) extend this record.

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

## Not covered here

No host-to-host transport, lease movement, failure injection, client reconnect, waystone or second test zone. These are parts 2 and 3. No canonical, checkpoint or SQL format changes; the wire change is the new error code.
