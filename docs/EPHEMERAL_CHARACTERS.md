# Temporary max-level characters (#340)

A mode host can admit a fresh, session-local MMORPG build through
`ZoneSimulation::admit_ephemeral`. It uses `MAX_PLAYER_LEVEL`, the existing class and
item catalogs, the normal equipped-stat/health calculation and normal resource starts.
No private `PlayerState` edits, duplicate stat calculations, account or progression grind
are required in a consuming mode.

```rust
use std::sync::Arc;
use mmorpg_core::{CharacterPersistence, ClassChoice, EphemeralCharacterBuild,
    PlayerClass, Sex, ZoneId, ZoneSimulation, greyhaven_vale};

let class = ClassChoice { class: PlayerClass::Ranger, sex: Sex::Female };
let build = EphemeralCharacterBuild::standard(class)?;
let content = Arc::new(Arc::unwrap_or_clone(greyhaven_vale::content())
    .with_character_persistence(CharacterPersistence::Ephemeral));
let mut zone = ZoneSimulation::with_content(ZoneId::new(100), content)?;
zone.admit_ephemeral(1, &build)?;
```

`EphemeralCharacterBuild::new` accepts six ordered optional item IDs and validates them
through `Equipment::from_slots`. Unknown items and wrong slots fail closed.
`from_character(&record)` copies only class/sex and equipped choices from a real character;
it does not copy its ID, level, XP, currency, bag, quests, health or combat continuation,
and never mutates the record. Guest choices use the same constructor.

## Existing catalog presets

| Class | Main hand |
| --- | --- |
| Warden | Militia Shortsword (3) |
| Ranger | Worn Dagger (2), the current agility weapon |
| Arcanist | Apprentice Wand (4) |

The rest is the current catalog's Pine Buckler, Cloth Hood, Padded Tunic, Padded Trousers
and Worn Boots (5–9). The catalog currently has no item-level or class restrictions and
only these armor choices. This does not invent top-tier PvP gear or new balance rules.
The current supported player cap is 10, read from `MAX_PLAYER_LEVEL` rather than a
mode-specific number. All current class ability level requirements fit that cap;
resources, cooldowns, targeting and other legality rules still apply. Wardens start
with empty rage; focus and mana start full, exactly as after normal class choice.

## Persistence and life boundaries

Ephemeral admission requires an explicitly ephemeral zone. Ordinary content defaults to
`CharacterPersistence::Durable` and refuses temporary builds. Conversely, ephemeral zones
refuse `capture_character`, `create_character` and `admit_character`: no arena loadout,
match damage or earned temporary progress can accidentally be written back through the
durable character-record APIs. The policy is immutable zone content and its fingerprint
binds canonical recovery. It does not prohibit match checkpoints: those restore the same
temporary session state, not a durable account character.

Fresh temporary lives have zero XP/copper, empty bags and quest logs, chosen equipment,
full derived health and cleared combat state. Admission refuses an existing player ID,
so it cannot change an active life. The mode decides when a life ends and when another
build may be admitted. Retirement and a fresh admission reset the temporary state; the
host/runtime must provide the corresponding command/session lifetime. This API does not
add an in-round respawn rule, client loadout command or character lease.

PvP eligibility is a separate arena content policy (#339); temporary setup does not
silently make an ordinary zone hostile. A combat arena additionally selects
`with_player_combat(PlayerCombatPolicy::FreeForAll)` before creating the zone. Both
policy construction orders produce the same content identity, and recovery binds both. Battle Royale match phases, between-life UI,
elimination/winner and real hosted play remain battle-royale #13 / MMORPG #341.

## Focused checks

`cargo test -p mmorpg-core --locked --test ephemeral_builds` covers all three supported
classes, legal max-level gear/health/resources, exact continuation, retirement/re-admission,
refusal to edit an existing life, input validation, unchanged durable records and the
capture/import boundary. No command, projection or canonical layout change is needed;
content identity distinguishes the persistence policy.

Convention source revision: `6cc05789dab39cee6758b288f83fb48b4ee6dbdb`.
