# Private Redbrand Depths instances as a pool of fenced zones

Status: accepted; implements the private-instance part of #404 (Atlas 5.1.24, 5.1.26, 5.1.27, 5.1.29, 5.1.30, 5.1.36; 5.1.i404-2 and 5.1.i404-3 settle its content and browser decisions).

## Decision

A private run is one logical zone, an **instance slot**: a depths zone from a fixed ID range (`20000..20032`) hosted like any zone. A host configures slot IDs in `MMORPG_ZONE_IDS` next to its public zone, so each slot has an ordinary fenced zone lease, party reconciliation and recovery wiring with no new placement machinery; a run's slot is unique among live runs, so one active writer per run per lease epoch is the ordinary zone-lease invariant. `content_for_zone` answers one authored dungeon, `redbrand_depths::content()`, for every slot. Slots run idle and empty between runs.

The **run directory** (`mmorpg_control_plane::instance`) is the one owner of runs, behind an `InstanceAuthority` store (`LocalInstances` single-process, `PostgresInstances` for the trusted-LAN handoff host, one row in `mmorpg_coordination.instance_directory`, like the party directory). It holds each run's frozen roster (character IDs in entry order; its length is the party-size profile), its slot, the progress the zone reports (cleared chambers and checkpoint) and when it went empty. Every write is fenced by the issuing zone's lease. A character is on at most one live run. The control plane records progress as data for a restarted host; it never touches zone gameplay.

A zone holds only the `DepthsRun` its host binds with a trusted input, canonical so replay and recovery see it. Composition, the wipe, reinforcements and rewards are the core's (`mmorpg_core::depths`); core only ever sees a frozen roster and a party size.

### Flow

1. A public zone's `DepthsThreshold` (strip and gathering area, content) reports a living, out-of-combat player standing in the strip with its gathered party members (party view, within the area) and whether each completed the qualifying quest.
2. The instance service resolves the request in one directory transaction: a roster character resumes its run; otherwise a new run takes the unbound candidates, needs one qualified candidate (5.1.36) and a free slot. Refusals (`DepthsLocked`, `DepthsClosed`) become a queued player notice.
3. The service queues the departure as a host input. The existing fenced handoff (ADR 0007) freezes the character, records the lease and offers it to the slot; a slot on the same host is accepted without the link, under the same storage and record guards, and the record decides activation and retirement. An arrival first binds the run (roster, progress) to the slot with a trusted input and then activates; the claim path admits a depths character only from an accepted transfer.
4. Exit is the content's `HandoffCorridor` at the entrance hall's west wall, an ordinary fenced transfer into the public zone's cave mouth, so return and recovery have exactly one ownership path. A character whose stored location is a depths slot (a run expired or the character signed out inside) returns to the same safe public entrance on its next admission.

### Run state and lifecycle

Composition is a table in the content (`DepthsSpawn`: party-size range and an optional reinforcement trigger), chosen once at run bind from the roster length, never rescaled. A chamber's creatures never respawn inside a run, so kill rewards are paid at most once per creature per run, across wipes, reconnects and recovery. When every present player is dead for 5 s, a wipe resolves: uncleared chambers' surviving creatures go home at full health, every player stands up at the checkpoint (after the first chamber) or the entrance with release-spirit health, and fallen creatures stay fallen (no second wipe-loop reward).

The directory resets a run's retention whenever a character (re)enters. After the last character leaves or disconnects, the run stays resumable for `MMORPG_DEPTHS_GRACE_MS` (default 15 minutes, a host setting in wall-clock time, never in the simulation), then `sweep` retires it, freeing its slot. Rebinding after a host restart replays the recorded progress; a slot whose run retired is reset by its next run's bind.

### Browser

The Pages demo's `LocalZoneHost` runs the depths solo in a second `ZoneSimulation` beside Greyhaven (one run, this host's lifetime, only that character), with the same content and the same transfer steps and no lease or second authority. Online browser clients join hosted instances like native clients: the redirect and zone presentation paths already exist.

## Behaviour decided here

- A roster is the party members standing in the gathering area (the inner chamber, whose threshold strip is against one wall) when one touches the strip. Members who are elsewhere are not on the roster; they cannot join later.
- One active run per character. A new run needs the old one to expire; there is no explicit reset command yet, so a party that wants a fresh run waits out the grace period (Atlas 5.1.27).
- Depths zones spawn creatures only for a bound run; an unbound slot is an empty room.
- Wipe recovery keeps fallen creatures fallen; reinforcements already arrived stay.
- On hosts without the instance service (open dev host, file character host, journaled PostgreSQL host) the threshold stays a closed barricade, no request is ever raised.

## Not covered

No cross-host slot placement beyond static zone routes (TECH-007's routing API); a very short window between a kill and the host's progress report can lose a chamber's clear on restart (the creature's reward is the zone's; no second grant is possible within one sim, and a fresh rebind with an older mask cannot reissue durable rewards because creature kills carry no durable receipts yet); the sealed boss chamber waits for #404 part 3.
