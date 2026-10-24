# Merge-safe format identities

Status: accepted (owner, 2026-10-24).

## Problem

Features bumped hand-written integers: the Greyhaven content `REVISION`, five catalog revisions, `SNAPSHOT_WIRE_VERSION`, `SNAPSHOT_SCHEMA_VERSION`, `COMMAND_WIRE_VERSION` and the WASM catalog, scenery and static-scene format versions, several mirrored by hand in the browser. Two parallel PRs both claimed the next number (schema 26 / revision 26). Identical one-line edits merge without a conflict, so main combined two different formats under one number and broke after the second merge. The repository is on a personal account, so there is no merge queue to serialize them.

## Decision

No compatibility guard is a counter.

- **Content is content-addressed.** `ZoneContent::revision()` is the FNV-1a hash of the canonical encoding of every table, the former fingerprint. `ZoneDefinition` and the loot, ability, vendor, quest and item catalogs have no revision; the hash covers their contents. Snapshots, scenery, the WASM catalog and checkpoints carry the hash and refuse any other value. Content PRs edit no number, and the hand-maintained revision changelog and fingerprint fixture are gone.
- **The wire format has one random identity.** `mmorpg_protocol::PROTOCOL_ID` is a 24-bit random token covering commands, player projections, presentation observations and canonical snapshots (checkpoints and saved browser worlds). Snapshots lead with its three bytes and commands with its low byte. A layout change runs `scripts/new-protocol-id.sh`, which writes fresh random bytes into the Rust constant and its browser copy (`web/src/protocol-id.ts`; a Rust test holds them equal). Two parallel layout changes therefore edit the same line differently, and git refuses to merge the second until it is rebased and rerolled. Decoders accept only the current ID; there is no migration (pre-1.0, no compatibility).
- **Same-build JSON exports carry no version.** The WASM catalog, scenery and static-scene exports are produced and consumed by the same build. Their strict key-set decoders already reject another shape.

Fixtures are unversioned (`fixtures/protocol/commands.hex`, `player-snapshot.hex`, `player-loot.hex`, `presentation-observation.hex`) and re-recorded with `MMORPG_UPDATE_FIXTURES=1`. Old-version fixtures are deleted. Tests flip ID bytes instead.

## Kept numeric

- PostgreSQL schema versions are ordered migrations (`MIGRATIONS.len()`). Appending to the same list conflicts in git.
- `CHARACTER_RECORD_WIRE_VERSION`, `ENTRY_FORMAT_VERSION`, `CHARACTER_RECORD_SCHEMA_VERSION`, the binding registry, roster and browser save versions are all still 1 or 2 and are rarely touched. The next change to one of them replaces it with a token the same way.
- The `game-server` session frame version (3) belongs to that repository.
- `PINNED_*_REVISION` and the Battle Royale arena content revision (`2` in `web/src/world/battle-royale/protocol.ts` and `mmorpg-wasm`) are cross-repository pins. They change when the pin moves. When battle-royale re-pins this crate, its content revision becomes its content hash and the browser pin takes that value.

## Consequences

- Parallel PRs either merge cleanly or conflict textually on the protocol ID line. They never combine silently.
- Any content change invalidates checkpoints and saved browser worlds, as revision bumps already did, and it cannot be forgotten.
- Pinned values that include the content hash still move with content, as they did with every revision bump: the scenery `STABLE_HASH` and presentation fingerprint tests, and seed-dependent goldens.
- Zoo follows the same rule (`docs/decisions/merge-safe-format-identities.md` there).
