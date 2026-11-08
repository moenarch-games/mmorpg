# Party membership lives in a fenced party directory

Status: accepted; implements the party-system part of #404 (Atlas 5.4.7, 5.3.6, 5.4.5, 5.4.i404, 5.4.i404-2).

## Decision

Party membership is cross-zone: a party survives a handoff, a member in another zone stays a member. Membership therefore cannot live in a zone. It has one owner, the **party directory** (`mmorpg_control_plane::party`), an in-memory reference model behind a `PartyAuthority` store:

- `LocalParties` for single-process hosts (standalone and development hosts, the journaled PostgreSQL host): zones are one process's, so there is no lease to fence and a restart starts it empty.
- `postgres::PostgresParties` for the trusted-LAN handoff hosts: one row in `mmorpg_coordination.party_directory` read and rewritten under its row lock, in the same database as their zone grants, so every host's operations are linearized.

A zone never holds or writes membership. It holds, as canonical state, only each of its players' **versioned view** (`PartyState`) set by a trusted host input `[1, 9, view]`, in order with commands, so replay and recovery reproduce it. Sharing a kill is a zone decision made from those views; the directory never touches player or zone gameplay (Atlas 5.4.7).

The party service (`mmorpg_game_server::party`, one per host) is the bridge. It answers clients' reliable-control party requests against the directory, and every 100 ms reconciles each zone: it reports the characters bound to the zone, sweeps expired invites and long-offline members, and submits any view that changed as a host input. Each directory transaction first checks the issuing zone's lease (`ensure_current_in` for PostgreSQL), so a fenced host changes nothing. Reconciliation is level-triggered from the zone's reported state and the directory, so handoffs, reconnects and host restarts need no party-specific hook: a character simply reports from its new zone.

Members are `CharacterId`s, independent of sessions and zones; hosts without character claims alias a session as its character (`session-<zone>-<player>`). The party keeps join order, so `members[0]` is the leader and the longest-standing member succeeds a leader who leaves. Refusals (stale roster versions, repeated or expired invites, not-the-leader actions) leave the directory untouched. A view's revision increases per character and is always above the revision its zone holds, so a stale view or one crossing a restart changes nothing (`ZoneSimulation::apply_party_state`).

Alternatives considered: carrying membership through the fenced handoff record (a party spanning two zones would need two copies and an arbiter between them, a second live writer); or zone-owned parties with cross-zone routing of membership changes (same problem, and it puts durable coordination in the simulation).

## Behaviour decided here (the Atlas left these open)

- **Eligibility:** membership is evaluated at the creature's death. A tapper's party member shares the kill if alive and within the zone's existing interest radius of the corpse (`kill_credit_eligible`), so "nearby" reuses the rule the tapper already passes. Each eligible member earns the same kill XP as a tapper would (no splitting) and the kill-objective credit `credit_kill` already deduplicates per life per quest log.
- **Loot eligibility** is recorded on the corpse at death (`CanonicalCreatureSnapshot::loot_party`). Any eligible member may make the existing one-time corpse claim; a quest item stays the tapper's alone, so collect objectives remain individual (5.4.5). Need/Greed remains #404 part 3.
- **Display names** are not server-owned yet (characters have no stored name). A client announces its roster name with `[2, 1, name]` once per connection; it travels only in party frames and `/invite`. `/invite` matches a unique online name case-insensitively, or `Player <n>`/`#<n>` (what other players' labels show); a selected player needs no name. Authenticated, unique character names remain future work.
- **Lifetimes:** an unanswered invite lapses after 60 s. A member offline for 5 minutes is removed as if they left; a party left with one member dissolves. A pending invite blocks a second one.

## Not covered

No cross-host party directory exists for the file development host (it is single-process anyway) and no durable directory for `LocalParties`. The single-row PostgreSQL store is sized for trusted-LAN use; a per-party schema can replace it behind `PartyAuthority` without touching zones or clients. Looking-for-group listings (5.4.11) and the party-private instance are later #404 work.
