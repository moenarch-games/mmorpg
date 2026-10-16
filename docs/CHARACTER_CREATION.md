# Character creation MVP

The browser demo supports a local character roster for testing the character-selection and world-entry flow without pretending that the browser owns authoritative MMO accounts.

## Creation flow

From **Choose your character**, select **Create character** and provide:

- a 2–24 character name;
- one of exactly three starter classes: **Warden**, **Ranger**, or **Arcanist**;
- **Male** or **Female** presentation.

The 3D preview updates before creation and remains rotatable. Class changes update the starter equipment and main-hand presentation: Warden uses a sword, Ranger a bow, and Arcanist a staff. Sex changes the presentation frame only; it does not change gameplay statistics.

Created characters start at level 1 in Greyhaven Outpost. The original Aelric Stormward preview remains a built-in level-18 character. **Enter World** spawns a new level-1 unit in the local WASM zone host and chooses the character's class and sex with `ChooseClass` as its first command, so the zone grants that class's resource and abilities. The preview's level stays presentation.

## Local identity and persistence

Created characters receive stable local IDs such as `local-1`. The browser stores only the created roster in the versioned `mmorpg.offline-roster.v1` record; the built-in character is not duplicated into storage.

Each character ID owns its own appearance-only save key and its unsaved in-memory appearance edits. Switching characters keeps the previous character's edits for this page session. World progress is not saved; see [save/load](SAVE_LOAD.md). A character ID that still has an appearance save or a legacy offline checkpoint is never reissued to a new character.

Roster parsing fails closed for unsupported schemas, malformed records, duplicate IDs, duplicate names (including the built-in character), invalid class/sex values, excessive slots, and oversized data. If roster storage is unavailable or corrupt, the existing bytes are not silently replaced; newly created characters remain session-only.

## Authority boundary

This is local browser-demo state. It does not add account creation, durable multiplayer character records, inventory authority, or server-side character persistence. Those remain future server-owned boundaries (#30 owns the durable character record; #40 composes it with zone/world checkpoints for demo save/load). The feature does not change `mmorpg-core`, `physics-engine`, `game-server`, or control-plane authority.
