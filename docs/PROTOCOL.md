# Zone snapshot protocol

All multibyte fields are big-endian. Integers are unsigned unless marked `i8`/`i16`/`i32`. Simulation positions use 100 units per metre; velocities use units per 30 Hz tick. Ticks and content revisions are 64-bit values; browser decoders retain them as `bigint`.

A yaw or facing is a `u16`: 65 536 steps are one full turn. Yaw 0 faces +Z and increasing yaw turns toward +X, so the facing direction is `(sin yaw, cos yaw)` in XZ. A character's right is the direction of `yaw − 90°`; facing +Z, right is −X.

## Session envelope

Commands and projections travel inside the pinned `game-server` session frames (protocol version 3), which `game-server` owns. A client datagram is a command frame `[version = 3, kind = 1, sequence: u32, length: u16, payload]`. A host datagram is a snapshot frame `[3, kind = 2, tick: u64, state hash: u64, length: u16, payload]` whose FNV-1a hash covers tick, payload length and payload, or one fragment of such a frame (kind 4, see [Datagram byte budget](#datagram-byte-budget)). The welcome is the host's first unidirectional stream: 46 bytes carrying player ID, tick rate, player capacity, current tick, connection epoch, the 16-byte reconnect token and the reconnect grace in ticks.

The shared fixture `fixtures/protocol/session-frames-v3.hex` pins these frames and the browser route contract: `crates/mmorpg-game-server/tests/session_frames.rs` renders it from the pinned encoders and decodes it with the pinned decoders, and the browser (`web/tests/session-frames.test.ts`) decodes and encodes the same bytes. A `game-server` pin bump that changes a frame fails in both languages.

## Commands (wire version 9)

The shared session runtime supplies player identity, connection epoch and command sequence separately. Core rejects zero, stale and duplicate sequences without changing state.

| Command | Bytes | Layout |
| --- | ---: | --- |
| Move | 6 | `[version = 9, tag = 1, forward: i8, strafe: i8, facing: u16]` |
| Jump | 2 | `[version = 9, tag = 2]` |
| SelectTarget | 7 | `[version = 9, tag = 3, kind: u8, id: u32]` |
| StartAttack | 2 | `[version = 9, tag = 4]` |
| StopAttack | 2 | `[version = 9, tag = 5]` |
| ReleaseSpirit | 2 | `[version = 9, tag = 6]` |
| MoveItem | 6 | `[version = 9, tag = 7, source_slot: u8, destination_slot: u8, quantity: u16]` |
| Loot | 14 | `[version = 9, tag = 8, creature_id: u32, died_at: u64]` |
| UseAbility | 8 | `[version = 9, tag = 9, ability: u8, kind: u8, id: u32]` |
| CancelCast | 2 | `[version = 9, tag = 10]` |
| ChooseClass | 4 | `[version = 9, tag = 11, class: u8, sex: u8]` |
| EquipItem | 3 | `[version = 9, tag = 12, bag_slot: u8]` |
| UnequipItem | 3 | `[version = 9, tag = 13, equipment_slot: u8]` |
| BuyItem | 9 | `[version = 9, tag = 14, npc: u32, offer: u8, quantity: u16]` |
| SellItem | 9 | `[version = 9, tag = 15, npc: u32, bag_slot: u8, quantity: u16]` |
| Chat | 4 + n | `[version = 9, tag = 16, channel: u8, length: u8, text: n bytes]` |
| Emote | 3 | `[version = 9, tag = 17, emote: u8]` |
| AcceptQuest | 7 | `[version = 9, tag = 18, npc: u32, quest: u8]` |
| CompleteQuest | 8 | `[version = 9, tag = 19, npc: u32, quest: u8, choice: u8]` |
| AbandonQuest | 3 | `[version = 9, tag = 20, quest: u8]` |

Version 9 adds quests; tags 1–17 retain their field layouts.

- `Move` is held intent relative to `facing`. `forward` and `strafe` are each in `[-1, 1]`; positive strafe is the character's right. The server rotates the intent into one of eight headings (multiples of 45° relative to `facing`) through its integer trigonometry table. Horizontal speed is 21 units/tick (6.3 m/s) when `forward ≥ 0` and 13 units/tick when `forward < 0`. Zero intent stops horizontal movement; vertical velocity always stays with physics. Dead units cannot move: death clears the held intent and a pending jump, and while dead `Move` and `Jump` consume their sequence without holding any intent, so the released spirit stands until the next `Move`. Dead units cannot move: death clears the held intent and a pending jump, and while dead `Move` and `Jump` consume their sequence without holding any intent, so the released spirit stands until the next `Move`.
- `Jump` is edge-triggered. It is recorded as pending and evaluated during the next tick before physics steps: when a thin probe directly below the feet touches any body other than the character (ground, geometry or another unit), vertical velocity becomes 16 units/tick. The tick always consumes the pending jump, so a mid-air jump has no effect and is not buffered until landing.
- `SelectTarget` carries an [entity reference](#entity-references): kind 1 player, 2 creature or 3 NPC with its ID, or kind 0 with ID 0 to clear the selection. `StartAttack` starts auto-attacking the selected target, `StopAttack` stops it, and `ReleaseSpirit` returns a dead player to the graveyard with half health.

The eighteen discrete intents (every command but `Move` and `Jump`) are queued in sequence order (at most 16 per player between ticks) and resolved during the next tick in `(player id, sequence)` order, never inside `apply_command`. A well-formed intent that is not allowed right now, such as attacking without a target or out of range, or selecting a unit that is not visible, is accepted and answered with an `Error` [event](#events); it never closes the session. An intent that finds the queue full is accepted the same way: it consumes its sequence, is dropped, and the next tick answers with one `too many intents` error. Only malformed payloads and stale or duplicate sequences fail.

`MoveItem` names only slots in the sender’s own bag. Slot/quantity values that fit the wire but cannot be applied are tick-time feedback, not session errors; there is no item grant command. `EquipItem` equips the item in one of the sender's bag slots into its catalog equipment slot, and `UnequipItem` moves the item in equipment slot 0 main hand, 1 off hand, 2 head, 3 chest, 4 legs or 5 feet to the lowest empty bag slot; every `u8` fits the wire and invalid slots are tick-time feedback. See [INVENTORY.md](INVENTORY.md#equipment).

`UseAbility` names a global ability ID and an [entity reference](#entity-references) as in `SelectTarget`; kind 0 (ID 0) means the current selection, and self-centred abilities ignore it. `CancelCast` stops the player's own cast or channel (nothing happens without one). `ChooseClass` picks class 0 Warden, 1 Ranger or 2 Arcanist and sex 0 female or 1 male, once per player. Every `u8` value fits the wire: unknown abilities, classes or sexes, repeats and every other refusal are `Error` events, never session errors (see [Classes and abilities](#classes-and-abilities)).

`BuyItem` buys `quantity` units of offer `offer` (its index in the vendor's stock) from vendor NPC `npc`; `SellItem` sells `quantity` units from one of the sender's bag slots to that vendor. Every value fits the wire; unknown vendors and offers, reach, copper, capacity and slot problems are tick-time feedback. See [INVENTORY.md](INVENTORY.md#vendors).

`Chat` says (channel 0) or yells (channel 1) one line of `length` UTF-8 bytes. The text must be 1–80 bytes, contain no control character and not be only whitespace; anything else is a malformed payload that fails the command. In tick step 1 a speaker who spoke within the last 30 ticks is refused with a `chat throttled` error; otherwise every player whose position is within 20 m (say) or 60 m (yell) of the speaker, horizontally and inclusively, the speaker included, hears the line in that tick's projection, in ascending player ID order and at most four lines per player per tick. Lines are not stored or replayed: like events, a lost datagram loses them.

`Emote` performs emote 1 wave, 2 bow, 3 cheer, 4 laugh or 5 point; any other ID is a malformed payload that fails the command. An emote is delivered exactly like a `/say` line: it shares the speaker's once-per-second limit with `Chat` (a refusal is the same `chat throttled` error), reaches every player within 20 m, the speaker included, in ascending player ID order, and counts toward the same four-per-tick bound. Core only names the emote; clients choose how to present it.

`AcceptQuest` accepts quest `quest` from its giver NPC `npc`; `CompleteQuest` turns it in at its ender NPC, taking reward `choice` (an index into the quest's reward items, 0 when it offers none); `AbandonQuest` drops an active quest and its progress. Every value fits the wire; unknown quests, wrong NPCs, prerequisites, a full log, reach (5 m XZ from the NPC's feet), incomplete objectives, invalid choices, bag capacity and copper overflow are tick-time feedback. A dead player may abandon but neither accept nor turn in. See [STARTER_ZONE.md](STARTER_ZONE.md#quests).

`Loot` names a creature spawn and its observed death tick. Core checks life, owner, expiry, authoritative 3D reach and remaining rewards during the tick, then atomically settles money and items. Refusals preserve rewards; no client supplies reward amounts. See [LOOT.md](LOOT.md).

Decoding is strict: exact lengths per tag, known tags only, `forward`/`strafe` in range, known entity kinds, ID 0 for the absent reference, and only version 9. Versions 1–8 are rejected.

The shared command fixture is `fixtures/protocol/commands-v9.hex`: one encoded command per line followed by its fields, covering every tag. `mmorpg-protocol` renders and verifies it, and the browser encoder (`web/src/command-wire.ts`) must produce the same bytes.

## Entity references

An entity reference is 5 bytes: kind (`u8`) then ID (`u32`). Kind 1 is a player (session-local player ID), 2 a creature (its spawn ID in zone content), 3 an NPC (its content ID). Kind 0 means no unit and requires ID 0. Ordering units by `(kind, id)` is the `EntityRef` order core uses for every ordered pass.

## Common snapshot prefix (16 bytes)

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Wire version: 14 |
| 1 | 1 | Scope: 1 canonical, 2 player-visible |
| 2 | 2 | Core schema version: 14 |
| 4 | 4 | Zone ID |
| 8 | 8 | Simulation tick |

## Player-visible scope

Sections follow in fixed order: header, self (including class, resource, cast, melee damage range, cooldowns and auras), target (target-of-target, then the target's cast and auras), self sheet (bag, equipment, stats and quests), self loot sheet, events, chat, entities.

| Offset | Width | Field |
| --- | --- | --- |
| 16 | 8 | Content revision |
| 24 | 4 | Acknowledged command sequence of the addressed player |
| 28 | 4 | Viewer player ID: the entity that is "self" |
| 32 | 4 | Self: health |
| 36 | 4 | Self: maximum health |
| 40 | 1 | Self: level (1–10) |
| 41 | 4 | Self: current-level XP |
| 45 | 4 | Self: XP to next level (0 at level 10) |
| 49 | 4 | Self: copper |
| 53 | 1 | Self flags: bit 0 dead, bit 1 in combat, bit 2 auto-attacking; other bits 0 |
| 54 | 5 | Self: target (entity reference, kind 0 for none) |
| 59 | 1 | Self: class code, 0 without a class, otherwise `1 + class × 2 + sex` (1–6) |
| 60 | 1 | Self: resource kind, 0 none, 1 rage, 2 focus, 3 mana |
| 61 | 2 | Self: resource value |
| 63 | 2 | Self: resource maximum |
| 65 | 2 | Self: global cooldown ticks left (at most 45) |
| 67 | 6 | Self: cast record |
| 73 | 2 | Self: melee damage minimum, the equipment bonus included |
| 75 | 2 | Self: melee damage maximum (at least the minimum) |
| 77 | 1 | Self: cooldown count, at most 4 |
| 78 | 3 × count | Cooldowns in ability order: ability ID (`u8`), ticks left (`u16`) |
| … | 1 | Self: aura count, at most 8 |
| … | 6 × count | Aura records in slot order |
| … | 5 | Target: the target's own target (entity reference) |
| … | 6 | Target: its cast record |
| … | 1 | Target: aura count, at most 8 |
| … | 6 × count | Target: aura records in slot order |
| … | 8 | Self inventory revision (nonzero), shared by bag and equipment |
| … | 1 | Self sheet present: 0 or 1 |
| … | 64 if present | Sixteen ordered bag slots: item ID (`u16`), quantity (`u16`) |
| … | 12 if present | Six equipment slots in slot order (main hand, off hand, head, chest, legs, feet): item ID (`u16`, 0 empty) |
| … | 8 if present | Stat totals over the equipment (`u16` each): stamina, strength, agility, intellect |
| … | 4 if present | Turned-in quests: bit `id − 1` per quest ID 1–32 |
| … | 1 if present | Active quest count, at most 10 |
| … | 4 × count | Active quests in ascending ID order: quest ID (`u8`), progress of objective slots 0–2 (`3 × u8`) |
| … | 1 if present | Marker count, at most 8 |
| … | 3 × count | Markers in ascending NPC order: NPC ID (`u16`), marker (`u8`: 1 `!` available, 2 grey `?` in progress, 3 `?` complete) |
| … | 1 | Self loot sheet present: 0 or 1 |
| … | 18 to 26 if present | Creature ID (`u32`), death tick (`u64`), copper reward (`u32`), item present (`u8`), optional item ID/quantity (`2 × u16`), quest item present (`u8`), optional quest item ID/quantity (`2 × u16`) |
| … | 1 | Event count, at most 16 |
| … | 14 × events | Event records |
| … | 1 | Chat line count, at most 4, then the [lines](#chat) |
| … | 2 | Entity count |
| … | 21 × count | Entity records |

The self section is the viewer's exact state: `dead` holds exactly when health is 0, and health never exceeds its maximum. A cast record is ability ID (`u8`, 0 for none), flags (`u8`, bit 0 channel), elapsed ticks (`u16`) and total ticks (`u16`); without a cast every field is 0, otherwise total and the channel flag equal the catalog's and elapsed stays below total. An aura record is ability ID (`u8`), aura kind (`u8`: 1 damage over time, 2 heal over time, 3 absorb, 4 root, 5 snare, 6 stun, 7 haste), ticks left (`u16`) and amount (`u16`: the total of a damage or heal over time, the remaining shield of an absorb, the percentage of a snare or haste, 0 for roots and stuns). The resource is present exactly with a class and its maximum is the class's at the viewer's level (rage and focus 100, mana 110 + 22 per level above 1). A viewer without a class has no global cooldown, cast, cooldowns or auras, and a dead viewer neither casts nor keeps auras. The target section is the target-of-target (a player's selection or an engaged creature's threat leader, so entity records need no per-entity target) and the target's cast and auras, which are empty unless the viewer's target is a living player or creature within the interest radius. The fixed part is 105 bytes; a projection is `105 + 3 × cooldowns + 6 × (auras + target auras) + (84 + 10 + 4 × quests + 3 × markers if the self sheet is present) + (18 to 26 if loot present) + 14 × events + chat + 21 × entities` bytes. Empty slots are exactly `(0, 0)`; occupied slots must match item catalog revision 3’s IDs and stack limits. Every equipped item must be a catalog item made for its slot, and the stat totals must equal the sums of the equipped items' stats. The melee damage range is the viewer's auto-attack and weapon roll range, the class damage bonus of its equipment included. A missing sheet means retain prior bag, equipment and quests, never an empty bag or log. Core sends the whole sheet on admission ticks, bag or quest change ticks and every ten ticks; the bag revision is repeated every tick so a receiver can detect a missed bag change. Queries do not consume the resend condition.

The quest part is the viewer's quest log ([STARTER_ZONE.md](STARTER_ZONE.md#quests)): which quests are turned in, each active quest's current progress per objective slot (kills, talks and explores as stored; collect objectives as the bag's count, capped at the required count; unused slots 0), and the per-player marker of every quest giver or ender that shows one. An NPC's marker is `?` when a quest ending there is complete, otherwise `!` when a quest starting there is available (not active, not turned in, prerequisite turned in), otherwise grey `?` when a quest ending there is active. Decoders require ascending quest IDs within 1–32 that are not also turned in, and ascending distinct NPC IDs with known marker codes; whether content defines them is the catalog's concern.

Copper and the complete optional loot sheet repeat every projection. A loot sheet's quest item is one unit of an item a quest of the corpse's tapper still needed when the creature died; it drew nothing from the loot stream. A sheet describes only the selected, owned, unexpired corpse within inclusive 300-unit 3D centre distance of a living viewer. Presence means replace the sheet; absence means clear it, unlike an omitted bag. It contains nonempty rewards and a death fence. Selected corpses lead packed entities, so a present sheet always has its matching dead, lootable entity. This unconditional state recovers lost changes without relying on cosmetic events.

### Events

The events are the viewer's feedback from the tick the snapshot describes (see [ARCHITECTURE.md](ARCHITECTURE.md#units-combat-and-creature-ai)). Each record is 14 bytes:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Kind: 1 damage dealt, 2 damage taken, 3 miss, 4 died, 5 evade, 6 error, 7 cast started, 8 ability used, 9 healed, 10 aura applied, 11 aura removed, 12 interrupted, 13 absorbed, 14 quest progress, 15 quest completed |
| 1 | 1 | Flags: bit 0 critical, allowed only for damage; for kinds 7–12 the ability ID instead (a catalog ability); for kinds 14 and 15 the quest ID (1–32); otherwise 0 |
| 2 | 5 | Source (entity reference) |
| 7 | 5 | Target (entity reference) |
| 12 | 2 | Amount |

- Damage dealt and damage taken carry both units and the damage in `amount`.
- Miss and evade carry both units and amount 0: a missed swing between the viewer and a unit, or the viewer's swing ignored by an evading creature.
- Died carries the dead unit as target, its killer as source (or none) and amount 0.
- Error carries no source, the unit the refused intent concerned as target (or none) and the error code in `amount`: 1 no target, 2 out of range, 3 target dead, 4 not attackable, 5 you are dead, 6 not dead, 7 invalid target, 8 too many intents (a full queue dropped an intent), 9 invalid inventory move, 10 inventory full, 11 invalid loot (unknown/non-corpse, expired or wrong death tick), 12 not loot owner, 13 empty loot, 14 money overflow, 15 no class, 16 not learned, 17 not ready (global or ability cooldown), 18 not enough resource, 19 stunned, 20 already casting, 21 invalid class (unknown or repeated choice), 22 not equippable (the bag item has no equipment slot), 23 invalid vendor (not a bound vendor NPC, or no such offer), 24 not enough money, 25 chat throttled (the speaker spoke within the last second), 26 invalid quest (unknown, not given or ended by that NPC, unavailable, not active, or an invalid reward choice), 27 quest log full (ten active quests), 28 quest incomplete.
- Cast started carries the caster, its target (or none) and the cast or channel ticks in `amount`. Ability used carries the user, its target (none for self-centred abilities) and amount 0.
- Healed carries the healer, the healed unit and the effective healing. Aura applied carries the caster, the unit and the aura's ticks; aura removed (expiry, a broken root, an emptied shield or replacement on a full unit) carries the caster, the unit and amount 0.
- Interrupted carries the interrupting unit (none for movement, a jump or cancellation), the interrupted unit and amount 0, with the interrupted ability in the flag byte.
- Absorbed carries the attacker, the shielded unit and the absorbed damage; its flags are 0.
- Quest progress carries no units, the quest in the flag byte and in `amount` the objective slot (high byte, 0–2) and its new progress (low byte, at least 1): a credited kill, a looted quest item, a talk or an explore. Quest completed carries no units, the quest in the flag byte and amount 0.

A unit's ability events reach the players among its source and target; events between creatures only (a bandit's bandage) reach the players engaged with them. When the queue overflows, deaths outrank damage taken, then errors and quest completions, then damage dealt and quest progress, then ability events (kinds 7–13), then misses and evades.

Events are cosmetic: a lost datagram may lose them. Health/XP, copper, complete loot presence/state and inventory revision repeat in every projection; the periodic complete sheet recovers lost bag and quest changes.

### Chat

After the events, a count (`u8`, at most 4) and that many lines the viewer heard this tick: speaker player ID (`u32`) and channel (`u8`). Channels 0 (say) and 1 (yell) continue with the text length (`u8`, 1–80) and the UTF-8 text, validated like the command; channel 2 (emote) continues with the emote ID (`u8`, 1–5). Four 80-byte lines take at most 344 bytes, so the viewer and its target still fit beside every other section.

### Entity records

Each entity record is 21 bytes:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 1 | Entity kind: 1 player, 2 creature, 3 NPC |
| 1 | 4 | Entity ID, unique per kind |
| 5 | 2 | Appearance: creature template ID, NPC ID, or for players the class code (0 without a class, otherwise `1 + class × 2 + sex`) |
| 7 | 6 | Position (`3 × i16`, absolute units) |
| 13 | 3 | Velocity (`3 × i8`, units per tick) |
| 16 | 2 | Facing (`u16` yaw) |
| 18 | 1 | Level |
| 19 | 1 | Health percent, 0–100, rounded up so a living unit never shows 0 |
| 20 | 1 | Flags: bit 0 dead, bit 1 in combat, bit 2 hostile, bit 3 attackable, bit 4 tapped by another player, bit 5 evading, bit 6 targets the viewer, bit 7 lootable by the viewer |

Records are in relevance-priority order: the viewer first, then the viewer's current target when it is a unit or corpse within the radius, then ascending `(squared XZ distance, kind, id)`. The interest policy keeps players, living creatures, corpses and NPCs within the inclusive 4,500-unit (45 m) XZ radius and caps relevance at the 64 (`MAX_VISIBLE_ENTITIES`) highest-priority units; the byte budget below then decides how many of those are written. Creature and NPC names come from content by appearance ID ([`mmorpg-wasm`'s catalog](../crates/mmorpg-wasm/src/catalog.rs) for the browser). A corpse keeps its record, flagged dead with health 0, until it despawns. Bit 7 means a creature corpse has nonempty rewards owned by the viewer; it promises neither reach nor a living viewer. It cannot accompany attackable or tapped-by-other flags. Successful claims clear it without shortening the existing corpse lifetime.

Positions are absolute `i16` units. Zone content keeps every collider bound and every spawned body within ±32,000 units (`MAX_CONTENT_COORDINATE_UNITS`). The simulation closes that cube with six fixed world-limit bodies that are not content, so no player admitted at a spawn slot can leave it, whatever the content's walls or gravity; content boundary walls, such as Greyhaven Vale's, are the gameplay boundary inside it. Recovery restores captured positions unchanged. The encoder still fails closed rather than clamping a position outside the `i16` range. Canonical state keeps full `i32` positions. Velocity is presentation data: core saturates each component to the `i8` range when projecting, while canonical state keeps the exact physics velocity (run speed is 21 and jump velocity 16 units/tick, well inside that range).

### Datagram byte budget

The pinned `game-server` sends a session snapshot frame unchanged when it fits the connection's current WebTransport datagram size. Otherwise it sends bounded, tick-keyed fragments; the native client and the browser's online mode reassemble and verify the session frame before decoding this payload. A lost fragment loses that snapshot, and newer complete ticks supersede older incomplete ones. The reassembler is reset on reconnect. This transport behavior does not change the MMO projection policy: its current budget derives from the smallest negotiated datagram size:

| Term | Bytes | Source |
| --- | ---: | --- |
| Measured floor (`MEASURED_MIN_DATAGRAM_BYTES`) | 1,161 | QUIC's 1,200-byte initial MTU before path MTU discovery, observed by both peers over the pinned wtransport/quinn stack |
| `game-server` snapshot frame header | − 20 | `game_server::SNAPSHOT_HEADER_BYTES` |
| Safety margin | − 64 | Headroom for unmeasured transport overhead |
| **`MAX_PLAYER_PROJECTION_BYTES`** | **1,077** | Largest player projection payload |

On loopback, path MTU discovery raises the host's value to 1,287 bytes before admission and the client's to 1,350–1,413 bytes, but on a real path the host can capture its limit before discovery completes, so the budget uses the floor. `mmorpg-client`'s `connected_world` tests log the negotiated size, pin the floor with discovery disabled on both peers, and deliver a crowded projection packed to the budget through a real host, with or without the self sheet.

Encoding is budget-driven in fixed section priority: `pack_snapshot` writes the header, self (with its cooldowns and auras), target (with its cast and auras), optional bag and loot sections and this tick's events, then entities in priority order until the next record would exceed the budget, and reports how many it packed. Hosts publish with it. Without events, sheets, cooldowns or auras, `(1,077 − 105) / 21 = 46` records fit (`MAX_WIRE_ENTITIES`). The self sheet takes 84 bytes plus its quest part: 10 bytes plus 4 per active quest and 3 per marker, at most 154 bytes with ten quests and eight markers, beside which 38 records fit. Beside all 16 events the counts are 35 without either sheet or 28 with the largest self sheet. The largest self sheet, a loot sheet with an item and a quest item and all events leave room for 27 records, and with full cooldown and aura lists for the viewer and its target (108 bytes) 23. The largest attainable payload is the whole 1,077 bytes. The viewer and its target lead the entities and always fit even beside full lists, both sheets, all events and four full chat lines: `105 + 108 + 26 + 154 + 16 × 14 + 4 × 86 + 2 × 21 = 1,003` bytes, which a compile-time assertion checks. The budget is unchanged from v8; at 30 Hz a full projection is at most 1,077 × 30 = 32,310 bytes per second per player (about 258 kbit/s), as before. Relevant units beyond the budget are omitted by priority, never truncated by transport. `encode_snapshot`, used by fixtures and tests, fails closed instead of omitting any entity. A test with extreme field values (maximum IDs, tick and revision, `i16`/`i8` extremes, a full event section and the whole relevance cap) proves the packing fits one datagram. Future payload growth requires a deliberate MMO budget and wire change; transport fragmentation alone does not raise the budget. Future sections take their place before the entities.

Player IDs are zone/session-local. These snapshots have no authority epoch field; the future online session/routing envelope must bind the stream to a grant and reset presentation on grant changes. An acknowledgement supports future prediction reconciliation, not permission to mutate authoritative state.

Decoders reject payloads above the byte budget, a wrong wire version, scope or schema, inconsistent self state (level 0, health above its maximum, a dead flag that disagrees with zero health), more than 16 events or 46 entities, zero inventory revision, an invalid sheet flag, quest logs with more than 10 entries, out-of-range, unordered or turned-in active quests, more than 8 markers, unordered markers or unknown marker codes, unknown items or invalid stack quantities, equipment items that are unknown or in the wrong slot, stat totals that do not match the equipment, an inverted melee damage range, inconsistent loot presence/rewards/target/death tick or corpse flags, unknown entity kinds, event kinds or error codes, event flags or fields that do not fit their kind, an absent reference with a non-zero ID, reserved flag bits, a health percent above 100, counts not matching the payload length, a first record that is not the viewer, truncation and trailing bytes. The browser decoder additionally rejects duplicate `(kind, id)` identities. The native client rejects duplicates, a projection addressed to another viewer, and a projection without its own player.

Decoders additionally reject class codes above 6, a resource that does not match the class or its maximum, a global cooldown above 45, casts with unknown abilities or a wrong total, channel flag or progress, more than 4 cooldowns or out-of-order, unknown or over-long cooldowns, more than 8 auras, auras whose kind or time left does not match the catalog, ability state without a class or on a dead viewer, target detail without a target, and unknown abilities in ability events; quest events need a quest ID in 1–32, no units, an objective slot below 3 with a nonzero count, or amount 0 for a completion.

The shared fixture is `fixtures/protocol/player-snapshot-v14.hex`: a level-4 Arcanist wearing an Apprentice Wand, a Cloth Hood and a Padded Tunic (3 stamina, 6 intellect: 110 maximum health and a 9–12 melee range) casting Firebolt behind an Arcane Barrier at a rooted Mirefin Lurker that casts Muck Bolt, next to an NPC and a corpse tapped by another player, with a sparse self bag, one turned-in quest, one active quest and two markers (`!` and grey `?`), one event of every kind, two chat lines (a say and a yell with non-ASCII text) and an emote. Rust encoding and browser decoding both verify these exact bytes. `player-loot-v14.hex` additionally pins an owned eligible corpse with two copper, two Torn Fur and a Wolf Pelt quest item. The v8–v13 fixtures remain as rejection evidence.

## Canonical scope

After the common prefix:

1. content revision (`u64`);
2. content fingerprint (`u64`);
3. zone AI/combat RNG state (`u64`);
4. independent loot RNG state (`u64`);
5. player count (`u16`, at most 512), then player records;
6. creature count (`u16`, at most 1,024), then creature records in creature-ID order.

Content is referenced, not embedded: the revision and the fingerprint (FNV-1a 64 over a canonical encoding of every content table, colliders included, plus item catalog, starter grant, declared RNG seed and any explicitly bound loot, ability, vendor and quest catalogs) identify the exact `ZoneContent`, and `ZoneSimulation::from_snapshot` fails closed unless the supplied content has both (the content-addressed checkpoint rule in [ARCHITECTURE.md](ARCHITECTURE.md#snapshots-and-compatibility)). Content that never bound the ability catalog (revision 0, such as `ZoneContent::new` or `from_definition` alone) leaves it out of the fingerprint, so its zones refuse `ChooseClass` with invalid class and every ability with no class, and recovery rejects a class player against it. Static colliders, NPCs and world-limit bodies come from that content during recovery.

A player record starts with 39 movement bytes, 92 inventory and equipment bytes and then its unit state:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 4 | Player ID |
| 4 | 12 | Position (`3 × i32`) |
| 16 | 12 | Velocity (`3 × i32`) |
| 28 | 2 | Facing (`u16` yaw) |
| 30 | 1 | Forward intent (`i8`) |
| 31 | 1 | Strafe intent (`i8`) |
| 32 | 1 | Jump pending: 0 or 1 |
| 33 | 4 | Last command sequence |
| 37 | 2 | Spawn slot |
| 39 | 8 | Inventory revision (nonzero) |
| 47 | 8 | Inventory last-change tick (at most snapshot tick) |
| 55 | 64 | Sixteen ordered item ID/quantity slots, as in the self sheet |
| 119 | 12 | Six equipment item IDs (`u16`, 0 empty), as in the self sheet |
| 131 | 4 | Copper |
| 135 | 1 | Level |
| 136 | 4 | Current-level XP |
| 140 | 4 | Health (0 means dead) |
| 144 | 5 | Target (entity reference) |
| 149 | 1 | Auto-attacking: 0 or 1 |
| 150 | 2 | Swing timer (ticks until the next swing) |
| 152 | 2 | Combat timer (ticks left in combat after the last blow) |
| 154 | 2 | Calm ticks (out-of-combat ticks driving regeneration) |
| 156 | 2 | Error cooldown (ticks until the next out-of-range error) |
| 158 | 1 | Pending intent count, at most 16 |
| 159 | Variable | Intent code: 1 select target, 2 start attack, 3 stop attack, 4 release spirit, 5 move item, 6 loot. Codes 1–5 occupy 6 bytes: codes 1–4 carry an entity reference (none except select); code 5 carries source/destination (`u8`), quantity (`u16`) and a reserved zero byte. Code 6 occupies 13 bytes: code plus creature ID (`u32`) and death tick (`u64`) |
| … | 1 | Intents dropped: 0 or 1; a full queue dropped a later intent, which the next tick reports |
| … | 1 | Event count, at most 16 |
| … | 14 × events | This tick's events, as in the player-visible scope |
| … | 1 | Class code: 0 without a class, otherwise `1 + class × 2 + sex` |
| … | 2 | Resource value |
| … | 2 | Resource ticks counted toward the next step (below 30) |
| … | 2 | Mana delay: ticks left of the five-second rule (at most 150) |
| … | 2 | Global cooldown ticks left |
| … | 1 | Cooldown count, at most 4 |
| … | 3 × cooldowns | Ability ID (`u8`) and ticks left (`u16`), in ability order |
| … | 17 | Cast: ability ID (`u8`, 0 for none, then every field 0), elapsed ticks (`u16`), target (entity reference), point present (`u8`), channel target point (`2 × i32`, XZ) |
| … | 1 | Aura count, at most 8 |
| … | 10 × auras | Ability ID (`u8`), caster (entity reference), ticks left (`u16`), amount (`u16`), in slot order |

Pending intent codes 7–11 extend the list: code 7 (use ability, 7 bytes) carries the ability ID and an entity reference, code 8 (cancel cast, 6 bytes) the absent reference, code 9 (choose class, 3 bytes) class and sex, code 10 (equip item, 2 bytes) the bag slot, code 11 (unequip item, 2 bytes) the equipment slot, codes 12 (buy item) and 13 (sell item), 8 bytes each, the vendor NPC (`u32`), the offer or bag slot (`u8`) and the quantity (`u16`), code 14 (chat) the channel, length and text as in the command, codes 15 (accept quest, 6 bytes) and 16 (complete quest, 7 bytes) the NPC (`u32`), the quest (`u8`) and for code 16 the choice (`u8`), and code 17 (abandon quest, 2 bytes) the quest. After each player's auras, the first tick it may speak again (`u64`, at most one interval ahead) and the chat lines it heard this tick (count `u8`, at most 4, then records as in the projection) follow, then the quest log: its last change tick (`u64`, at most the snapshot tick), the turned-in mask (`u32`), the active quest count (`u8`, at most 10) and the active quests (quest ID and three stored progress bytes each, ascending). Stored progress of a collect objective is 0: its progress is the bag's count.

A creature record:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 4 | Creature ID (its spawn ID) |
| 4 | 1 | Level |
| 5 | 4 | Health |
| 9 | 2 | Facing |
| 11 | 12 | Position (`3 × i32`): the body, or the corpse |
| 23 | 12 | Velocity (`3 × i32`) |
| 35 | 1 | Life: 1 alive, 2 corpse, 3 despawned |
| 36 | 8 | Death tick (0 while alive) |
| 44 | 1 | AI: 1 idle, 2 engaged, 3 evading |
| 45 | 2 | AI timer: idle wait or walk ticks, evade ticks so far; 0 when engaged |
| 47 | 1 | Wander destination present: 0 or 1 (idle only) |
| 48 | 8 | Wander destination (`2 × i32`, XZ), zero when absent |
| 56 | 1 | Threat entry count, at most 64 |
| 57 | 9 × entries | Threat entries in entity order: entity reference and threat (`u32`) |
| … | 2 | Swing timer |
| … | 2 | Combat timer |
| … | 1 | Tapped: 0 or 1 |
| … | 4 | Tapping player ID, 0 when untapped |
| … | 1 | Remaining loot present: 0 or 1 |
| … | 6 to 14 if present | Copper (`u32`), item present (`u8`), optional validated item ID/quantity (`2 × u16`), quest item present (`u8`), optional validated quest item ID/quantity (`2 × u16`) |
| … | 2 | Ability timer: ticks until its content-bound ability may be used |
| … | 17 | Cast, as in the player record |
| … | 1 | Aura count, at most 8 |
| … | 10 × auras | Aura records, as in the player record |

The decoder rejects booleans other than 0 or 1, unknown codes, unknown or misplaced equipment items, an alive creature with a death tick, AI fields that do not fit their state, an absent threat unit, an untapped creature with a tapper, excessive counts, truncation and trailing bytes. Core then validates the state against the content during recovery: player uniqueness, nonzero bag revision and change tick at most the snapshot tick, spawn slots, movement range, level and health bounds (the maximum includes the equipment's stamina), queue sizes, that only a full intent queue has dropped intents, that dead players do not auto-attack (a dead player's movement or jump intent from an earlier build is cleared on restore), that creature records match the content's spawns one-to-one in ID order with levels and health inside their template, positions inside the ±32,000-unit content range, wander destinations at most one unit beyond their spawn's wander radius, and state consistent with their life cycle (corpses and despawned creatures rest; only engaged creatures have threat, and threat tables hold only living players), and that player targets and tapping owners exist. Remaining loot requires an owned, unexpired corpse and rewards inside its exact authored template table (or none without a table), and a quest item must be one unit of a collect objective's item that the content's quests drop from that template; unknown items, impossible quantities/outcomes or money bounds fail closed. A quest log must name ascending, distinct quests of the content (none without a quest catalog), at most ten, none also turned in, each with its prerequisite turned in and stored progress within its objectives' counts (0 for collect objectives and unused slots); turned-in quests must exist and have their prerequisites turned in. Ability state is validated too: a player without a class has none; a class player's resource clock fits its class and level, its cooldowns are learned, ascending and within their catalog cooldowns, and its cast is a learned cast or channel in progress (only channels keep a point, only targeted abilities a target); a creature's ability timer stays within its bound ability's cooldown plus jitter (or the 120-tick lockout) and only an engaged creature casts its own ability; auras name catalog abilities with an aura, time left within the duration, the fixed amount of snares, hastes, roots and stuns, and a nonzero damage-over-time, heal-over-time or shield amount no larger than any player level reaches (the catalog effect at level 10), at most one per caster and ability, each cast by a living player who learned it, on that player for heal over time, haste and absorb and on a creature otherwise; cast targets and aura casters exist, players cast only at creatures and creatures only at living players (a death removes every creature cast at the fallen player); dead units neither cast nor keep auras, and a dead Warden has no rage and no decay ticks. Default engine configuration and pinned physics behavior are part of the continuation contract: recovering mid-run, mid-jump, with a pending jump, mid-chase, mid-swing, after a death, during an evade, mid-cast, mid-channel, during a creature cast or with a damage or heal over time, root or shield active reproduces the continuation exactly.

Canonical data is for trusted replay/recovery and server-side verification. It must never be passed to the browser renderer or substituted for a player projection.

## Versions and migration

Before progression, snapshot wire and core schema version 5 replaced version 4 for both scopes: player projections gained the self, target and event sections and 21-byte entity records for players, creatures and NPCs; canonical snapshots reference content by revision and fingerprint instead of embedding it and carry unit, creature, RNG and event state. Command wire version 2 gained tags 3–6 additively. Old bytes are never reinterpreted and there is no bundled migration:

- version 4 snapshots, canonical checkpoints and recovery bundles are rejected;
- a standalone host restarted with an old `MMORPG_RECOVERY_DIR` fails closed. Start from fresh development state or arrange an explicit migration.

Protocol changes require updating version handling, this specification, the shared fixture and both language contract tests together.

## Starter XP snapshot v6

Version 6 adds current-level XP (`u32`) after each canonical player's level,
and XP plus next-level threshold (two `u32`s) after the self section's level.
The 1,077-byte projection budget is unchanged: 63 fixed bytes, up to 48 entities
without events or 37 with all 16 events. The largest attainable payload is
1,071 bytes because records advance in multiples of seven.

Both Rust and browser decoders reject out-of-range player levels, nonzero
capped XP/threshold, zero uncapped thresholds and XP at or above its threshold.
Canonical restore validates XP against the exact shared curve. Version 5 is
retained as legacy fixture evidence and rejected; there is no implicit save or
recovery migration. Existing version-5 recovery directories need an explicit
migration or fresh development state. Commands remain version 2.

## Starter inventory snapshot v7

Version 7 adds the canonical bag/revision/change tick and optional self bag sheet
above. Core schema is also 7. Content identity includes the immutable catalog,
starter grant and declared RNG seed. Greyhaven revision 4 preserves revision 3’s
RNG seed, so adding bags does not reroll existing creature/combat scripts.

Version 6 and earlier snapshots and recovery bundles fail closed. The v5/v6
fixtures remain legacy rejection evidence; this change supplies no implicit
migration for saved recovery directories. Commands remain v2 with additive tag 7.
The browser validates the supported wire catalog’s stack shapes, not grant rules.

## Corpse loot snapshot v8 and command v3

Version 8 adds canonical copper, remaining corpse rewards, an independent loot RNG
state and queued death-fenced claims. Projections add copper, optional complete
loot state and creature flag bit 7. Command version 3 adds tag 8 and rejects v2.
Greyhaven revision 5 binds loot catalog revision 1; its fingerprint is
`5dcb5d3b46dc5451`. The existing AI/combat seed remains unchanged.

Version 7 and earlier snapshots/checkpoints, version 2 and earlier commands, and
old recovery bundles fail closed. Historical fixtures remain rejection evidence.
This change provides no automatic migration; arrange an explicit migration or
fresh development recovery state before upgrading a standalone host.

## Classes and abilities: snapshot v9 and command v4

Version 9 adds the class choice, class resources, the global cooldown, cooldowns, casts, channels and auras of players and creatures to canonical records, the viewer's ability state and the target's cast and auras to projections, player appearance by class, ability events 7–13 and error codes 15–21. Command version 4 adds tags 9–11 and rejects v3. Greyhaven revision 6 binds ability catalog revision 1 (fingerprint `19e2d33bf767bf2f`) and keeps the existing AI/combat seed; lurker and bandit abilities draw from it, so fights with them change.

### Classes and abilities

The catalog (`mmorpg_core::ABILITY_CATALOG`, revision 1) gives every ability a global `u8` ID; the WASM `catalog()` export (format v3) carries names, users, unlock levels, costs, cast times, cooldowns and aura kinds for clients. Abilities resolve in tick step 1 (instants) or step 6 (casts and channels); see [ARCHITECTURE.md](ARCHITECTURE.md#units-combat-and-creature-ai). A refused `UseAbility` reports the first failing check in this order: class (no class), learned (not learned, also for other classes' and unknown IDs), alive (you are dead), stunned, casting (already casting), global cooldown and cooldown (not ready), resource (not enough resource), target (invalid target: no living, visible creature) and range (out of range, XZ centre distance). A cast whose target died completes as interrupted; one that cannot be paid on completion reports not enough resource.

Version 8 and earlier snapshots/checkpoints, version 3 and earlier commands and old recovery bundles fail closed. Historical fixtures remain rejection evidence. This change provides no automatic migration; arrange an explicit migration or fresh development recovery state before upgrading a standalone host.

## Equipment: snapshot v10 and command v5

Version 10 adds the six canonical equipment slots after each player's bag, pending intent codes 10 and 11, the viewer's melee damage range after its cast record, equipment and stat totals in the self sheet (84 bytes when present) and error code 22. Command version 5 adds tags 12 and 13 and rejects v4. Item catalog revision 2 adds seven equippable items, loot catalog revision 2 drops them from humanoids, and Greyhaven revision 7 binds both (fingerprint `5a8f35c63f4c8849`) and keeps the existing AI/combat seed. The projection budget, `MAX_WIRE_ENTITIES` and the entity record are unchanged. The WASM `catalog()` export is format v4: each item gains `slot` (`null` or `"mainHand"`, `"offHand"`, `"head"`, `"chest"`, `"legs"`, `"feet"`) and `stats` (`{stamina, strength, agility, intellect}`).

Version 9 and earlier snapshots/checkpoints, version 4 and earlier commands and old recovery bundles fail closed. Historical fixtures remain rejection evidence. This change provides no automatic migration; arrange an explicit migration or fresh development recovery state before upgrading a standalone host.

## Vendors: snapshot v11 and command v6

Version 11 adds pending intent codes 12 and 13 to canonical player records and error codes 23 and 24 to events; the player-visible layout is otherwise unchanged. Command version 6 adds tags 14 and 15 and rejects v5. Vendor catalog revision 1 gives every catalog item a sale value and binds Innkeeper Bram Tolliver's eight-offer stock; Greyhaven revision 8 binds it (fingerprint `e07f6bec8e077beb`) and keeps the existing AI/combat seed. The projection budget, `MAX_WIRE_ENTITIES` and the entity record are unchanged. The WASM `catalog()` export is format v5: each item gains `sellPrice`, and `vendorCatalogRevision` and `vendors` (`[{npc, offers: [{item, price}]}]`, offer order is the wire offer index) list the stock.

Version 10 and earlier snapshots/checkpoints, version 5 and earlier commands and old recovery bundles fail closed. Historical fixtures remain rejection evidence. This change provides no automatic migration; arrange an explicit migration or fresh development recovery state before upgrading a standalone host.

## Zone chat: snapshot v12 and command v7

Version 12 adds the chat section after the events of player projections (one count byte when empty, so the fixed part is 105 bytes), pending intent code 14 and the per-player chat rate limit and heard lines to canonical records, and error code 25. Command version 7 adds tag 16 and rejects v6. Content, catalogs and the entity record are unchanged; whole entity records now fill a projection to at most 1,074 bytes.

## Zone emotes: snapshot v13 and command v8

Version 13 adds channel 2 (emote) to chat records in player projections and canonical heard lines, and to pending chat intents (code 14) in canonical records. Command version 8 adds tag 17 and rejects v7. Content, catalogs, the entity record and the projection budget are unchanged; an emote record (6 bytes) is smaller than any chat line.

Version 12 and earlier snapshots/checkpoints, version 7 and earlier commands and old recovery bundles fail closed. Historical fixtures remain rejection evidence. This change provides no automatic migration; arrange an explicit migration or fresh development recovery state before upgrading a standalone host.

## Quests: snapshot v14 and command v9

Version 14 adds the quest part to the self sheet, a quest item to loot sheets and canonical corpse rewards, quest events 14 and 15, error codes 26–28, pending intent codes 15–17 and each player's quest log to canonical records. The projection's fixed part is unchanged (105 bytes), so `MAX_WIRE_ENTITIES` stays 46; with every section full the viewer and its target still fit (1,003 of 1,077 bytes). Command version 9 adds tags 18–20 and rejects v8. Item catalog revision 3 adds the Wolf Pelt (item 10, stacks to 10), vendor catalog revision 2 prices it, quest catalog revision 1 is the Greyhaven chain, and Greyhaven revision 9 binds all three (fingerprint `2630ca15716973f3`) and keeps the existing AI/combat seed; quest drops draw nothing from the loot stream. The WASM `catalog()` export is format v6: `questCatalogRevision` and `quests` (`[{id, name, text, giver, ender, prerequisite, objectives: [{kind, target, source, count}], experience, copper, choices}]`; objective order is the progress slot, choice order the reward index) list the quests.

Version 13 and earlier snapshots/checkpoints, version 8 and earlier commands and old recovery bundles fail closed. Historical fixtures remain rejection evidence. This change provides no automatic migration; arrange an explicit migration or fresh development recovery state before upgrading a standalone host.
