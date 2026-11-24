# Derived protocol ID

Status: accepted (#1066, 2026-11-24). Amends the wire identity of [ADR 0006](0006-merge-safe-format-identities.md).

## Problem

ADR 0006 made `mmorpg_protocol::PROTOCOL_ID` a random token that `scripts/new-protocol-id.sh` rerolled on every layout change. That kept two formats from merging under one number, but at a cost. Any two open PRs that touched the wire conflicted on the ID line and on every hex fixture, because every fixture led with the ID, even when their layout changes had nothing to do with each other. Each conflict meant rerolling and re-recording after the merge. The four fixtures were the most-changed files in the repository.

## Decision

The ID is derived, not chosen.

- **The layout is described line by line.** `fixtures/protocol/layout.txt` has one line per command kind (name and sizes) and one line per field of every other message sample: the canonical snapshot, the three player projections and the redirect notice (`message @offset width`). Bytes no field read covers, such as strings, appear as `blob` lines, so the lines cover every byte. `mmorpg-protocol`'s layout test generates the description by tracing the decoders' reads (`wire::take`) over the same samples the byte fixtures encode. It fails until the file matches, and `MMORPG_UPDATE_FIXTURES=1` rewrites it. The description cannot drift from the bytes.
- **The fixtures carry no ID.** `commands.hex`, `player-snapshot.hex`, `player-loot.hex`, `presentation-observation.hex` and the new `canonical-snapshot.hex` spell each leading ID byte `PP`. Readers put the current ID back (`mmorpg_protocol::fixture`). A layout change elsewhere no longer rewrites them.
- **The ID is a hash of both.** At compile time `PROTOCOL_ID` is FNV-1a over `layout.txt` and the placeholder fixtures (carriage returns skipped), folded to 24 bits. A low byte of 1–13, a retired command wire version, is rehashed until it is not. The fixtures are hashed as well because they pin what widths cannot: codes, flags and field meaning. A change that keeps every width still moves the ID.
- `scripts/new-protocol-id.sh` is retired. Builds of the same layout and fixtures agree on the ID. Any change to them moves the ID with no manual step. The browser still reads the ID through `mmorpg-wasm`, and decoders still accept only the current ID.

## Consequences

- Two branches that change different messages edit different lines of `layout.txt` and different fixtures, so they merge cleanly and the merged build derives its own ID. They conflict only when both edit the same message, and that is a real conflict.
- Changing a sample in a test changes the ID even though the layout stays the same. Like any rotation, this only invalidates checkpoints and saved browser worlds, which content changes already do.
- The layout covers what the samples exercise. A new message, record kind or optional section needs a sample in its fixture, as the byte fixtures already did.
