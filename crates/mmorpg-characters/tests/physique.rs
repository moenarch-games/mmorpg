//! Physique V1 (#633) through the character store: every slider survives create, commit,
//! reload, reconnect and later editing, and records saved before physique load neutral. Records
//! saved before the turned-in quest set (#862) load with their `u32` mask migrated to it, and
//! records saved before 32-bit content IDs (#882) load with their narrow IDs and waystone mask
//! migrated.

use mmorpg_characters::{CharacterStore, FileEntries, LeaseHolder};
use mmorpg_core::{
    AbilityId, BARROWDOWN, CharacterId, CharacterRecord, ClassChoice, Cooldown, Equipment,
    GREYHAVEN_OUTPOST, ItemId, PHYSIQUE_PARAMS, Physique, PhysiqueParam, PhysiquePreset,
    PlayerClass, QuestEntry, QuestId, Sex, WaystoneId, WaystoneSet, ZoneId, ZoneSimulation,
    greyhaven_vale,
};
use mmorpg_protocol::{decode_character_record, encode_character_record};

const ZONE: ZoneId = ZoneId::new(1);

fn holder(epoch: u64) -> LeaseHolder {
    LeaseHolder {
        zone_id: ZONE,
        zone_epoch: epoch,
    }
}

fn zone() -> ZoneSimulation {
    ZoneSimulation::with_content(ZONE, greyhaven_vale::content()).unwrap()
}

/// Every slider distinct, the cosmetic ones included.
fn custom() -> Physique {
    let values: [f64; PHYSIQUE_PARAMS] =
        [0.1, 0.9, 0.25, 0.75, 0.6, 1.0, 0.0, 0.33, 0.67, 0.2, 0.8];
    Physique::from_normalized(values).unwrap()
}

#[test]
fn physique_survives_create_save_reload_reconnect_and_edit() {
    let directory =
        std::env::temp_dir().join(format!("mmorpg-characters-physique-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let character = CharacterId::new("physique-1").unwrap();

    // Create with a customized physique and commit it under the lease.
    let mut first = zone();
    first
        .create_character(
            1,
            ClassChoice {
                class: PlayerClass::Arcanist,
                sex: Sex::Female,
            },
        )
        .unwrap();
    first.customize_physique(1, custom()).unwrap();
    let created = first.capture_character(1, character.clone()).unwrap();
    assert_eq!(created.physique, custom());
    let mut store = CharacterStore::new(FileEntries::open(&directory).unwrap());
    let lease = store.acquire(&character, holder(1)).unwrap().lease;
    store.release(&lease, Some(&created)).unwrap();

    // Reload from disk in a new process and reconnect to a new zone session.
    let mut store = CharacterStore::new(FileEntries::open(&directory).unwrap());
    let acquired = store.acquire(&character, holder(2)).unwrap();
    let loaded = acquired.record.unwrap();
    assert_eq!(loaded, created);
    let mut second = zone();
    second.admit_character(9, &loaded).unwrap();
    assert_eq!(second.player_physique(9), Some(custom()));

    // Edit again in the zone: one cosmetic slider and a preset, then commit and reload.
    let edited = PhysiquePreset::Stocky
        .physique()
        .with(PhysiqueParam::JawProjection, 10_000)
        .unwrap();
    second.customize_physique(9, edited).unwrap();
    let record = second.capture_character(9, character.clone()).unwrap();
    store.release(&acquired.lease, Some(&record)).unwrap();
    let reloaded = CharacterStore::new(FileEntries::open(&directory).unwrap())
        .committed(&character)
        .unwrap()
        .unwrap()
        .record;
    assert_eq!(reloaded.physique, edited);
    assert_eq!(reloaded.class, loaded.class);
    let _ = std::fs::remove_dir_all(&directory);
}

/// The schema before 32-bit content IDs and the waystone set (#882).
const PRE_WIDE_IDS: u16 = 0x20ba;
/// The schema before the turned-in quest set (#862).
const PRE_QUEST_SET: u16 = 0x9d41;
/// The schema before physique (#633).
const PRE_PHYSIQUE: u16 = 0x6671;

/// The record's bytes in a layout from before #882 under `schema`: `u16` item IDs, one-byte
/// quest and ability IDs and a `u32` waystone mask (bit `id - 1`). Before the quest set (#862)
/// the turned-in quests were a `u32` mask too, and before physique (#633) the tail was absent.
fn legacy_bytes(record: &CharacterRecord, schema: u16) -> Vec<u8> {
    let byte = |id: u32| u8::try_from(id).unwrap();
    let half = |id: u32| u16::try_from(id).unwrap().to_be_bytes();
    let mask = |ids: Vec<u32>| ids.into_iter().fold(0_u32, |mask, id| mask | 1 << (id - 1));
    let id = record.character_id.as_str().as_bytes();
    let mut bytes = vec![1];
    bytes.extend_from_slice(&schema.to_be_bytes());
    bytes.push(byte(u32::try_from(id.len()).unwrap()));
    bytes.extend_from_slice(id);
    bytes.push(byte(u32::from(record.class.appearance())));
    bytes.push(record.level);
    bytes.extend_from_slice(&record.experience.to_be_bytes());
    bytes.extend_from_slice(&record.copper.to_be_bytes());
    for slot in record.inventory.slots() {
        let (item, quantity) = slot.map_or((0, 0), |stack| (stack.item().get(), stack.quantity()));
        bytes.extend_from_slice(&half(item));
        bytes.extend_from_slice(&quantity.to_be_bytes());
    }
    for slot in record.equipment.slots() {
        bytes.extend_from_slice(&half(slot.map_or(0, ItemId::get)));
    }
    let quests = &record.quests;
    if schema == PRE_WIDE_IDS {
        bytes.push(byte(u32::try_from(quests.completed.len()).unwrap()));
        bytes.extend(quests.completed.iter().map(|quest| byte(quest.get())));
    } else {
        let completed = quests.completed.iter().map(|quest| quest.get()).collect();
        bytes.extend_from_slice(&mask(completed).to_be_bytes());
    }
    bytes.push(byte(u32::try_from(quests.entries.len()).unwrap()));
    for entry in &quests.entries {
        bytes.push(byte(entry.quest.get()));
        bytes.extend_from_slice(&entry.progress);
    }
    let waystones = record.waystones.iter().map(WaystoneId::get).collect();
    bytes.extend_from_slice(&mask(waystones).to_be_bytes());
    let depths = record.depths;
    bytes.push(u8::from(depths.thorn_defeated));
    bytes.extend_from_slice(&depths.receipt_run.to_be_bytes());
    bytes.push(u8::from(depths.credited));
    bytes.push(depths.drops);
    bytes.extend_from_slice(&record.health.to_be_bytes());
    for value in [
        record.resource,
        record.resource_ticks,
        record.mana_delay,
        record.global_cooldown,
    ] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes.push(byte(u32::try_from(record.cooldowns.len()).unwrap()));
    for cooldown in &record.cooldowns {
        bytes.push(byte(cooldown.ability.get()));
        bytes.extend_from_slice(&cooldown.remaining.to_be_bytes());
    }
    let location = record.location;
    bytes.extend_from_slice(&location.zone_id.get().to_be_bytes());
    for component in location.position {
        bytes.extend_from_slice(&component.to_be_bytes());
    }
    bytes.extend_from_slice(&location.facing.to_be_bytes());
    if schema != PRE_PHYSIQUE {
        let values = record.physique.fixed();
        bytes.push(byte(u32::try_from(values.len()).unwrap()));
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
    }
    bytes
}

#[test]
fn older_records_load_neutral_and_keep_everything_else() {
    let mut zone = zone();
    zone.create_character(
        1,
        ClassChoice {
            class: PlayerClass::Ranger,
            sex: Sex::Male,
        },
    )
    .unwrap();
    let record = zone
        .capture_character(1, CharacterId::new("before-physique").unwrap())
        .unwrap();
    let mut customized = record.clone();
    customized.customize_physique(custom());

    let legacy = decode_character_record(&legacy_bytes(&customized, PRE_PHYSIQUE)).unwrap();
    assert_eq!(legacy.physique, Physique::NEUTRAL);
    assert_eq!(
        CharacterRecord {
            physique: customized.physique,
            ..legacy.clone()
        },
        customized
    );

    // A shorter physique list (five sliders) leaves the rest neutral; values past 1.0 and more
    // than eleven values fail closed.
    let bytes = encode_character_record(&customized).unwrap();
    // The discovered areas (#916) and opened caches (#913) follow the physique: a u16 count and
    // a u32 per entry each; then the sword mastery (#915): a u8 level and a u16 progress.
    let after = 4 + 4 * (customized.discoveries.len() + customized.caches.len()) + 3;
    let tail = bytes.len() - after - 1 - 2 * PHYSIQUE_PARAMS;
    let mut five = bytes[..tail].to_vec();
    five.push(5);
    five.extend_from_slice(&bytes[tail + 1..tail + 11]);
    five.extend_from_slice(&bytes[bytes.len() - after..]);
    let five = decode_character_record(&five).unwrap();
    assert_eq!(five.physique.fixed()[..5], custom().fixed()[..5]);
    assert_eq!(five.physique.fixed()[5..], Physique::NEUTRAL.fixed()[5..]);
    let mut too_large = bytes.clone();
    too_large[tail + 1..tail + 3].copy_from_slice(&10_001_u16.to_be_bytes());
    assert!(decode_character_record(&too_large).is_err());
    let mut too_many = bytes.clone();
    too_many[tail] = 12;
    too_many.extend_from_slice(&[0, 0]);
    assert!(decode_character_record(&too_many).is_err());
}

#[test]
fn records_with_the_quest_mask_load_with_the_same_quests_turned_in() {
    let mut zone = zone();
    zone.create_character(
        1,
        ClassChoice {
            class: PlayerClass::Warden,
            sex: Sex::Female,
        },
    )
    .unwrap();
    let mut record = zone
        .capture_character(1, CharacterId::new("before-quest-set").unwrap())
        .unwrap();
    record.quests.completed = [1, 2, 13, 32].map(QuestId::new).into();

    // The schema before the set (#862), and the pre-physique one before that.
    let migrated = decode_character_record(&legacy_bytes(&record, PRE_QUEST_SET)).unwrap();
    assert_eq!(migrated, record);
    let migrated = decode_character_record(&legacy_bytes(&record, PRE_PHYSIQUE)).unwrap();
    assert_eq!(migrated.quests, record.quests);

    // Written again, it is in the current schema and reads back unchanged.
    let bytes = encode_character_record(&migrated).unwrap();
    assert_eq!(decode_character_record(&bytes).unwrap(), migrated);
}

#[test]
fn records_with_narrow_ids_load_with_their_items_quests_abilities_and_waystones() {
    let mut zone = zone();
    zone.create_character(
        1,
        ClassChoice {
            class: PlayerClass::Arcanist,
            sex: Sex::Male,
        },
    )
    .unwrap();
    let mut record = zone
        .capture_character(1, CharacterId::new("before-wide-ids").unwrap())
        .unwrap();
    record.customize_physique(custom());
    record.quests.completed = [1, 2, 13].map(QuestId::new).into();
    record.quests.entries = vec![QuestEntry {
        quest: QuestId::new(14),
        progress: [2, 0, 1],
    }];
    record.waystones = WaystoneSet::from_ids([GREYHAVEN_OUTPOST, BARROWDOWN]).unwrap();
    record.cooldowns = vec![Cooldown {
        ability: AbilityId::new(9),
        remaining: 40,
    }];
    assert!(record.inventory.slots().iter().flatten().next().is_some());
    record.equipment =
        Equipment::from_slots([Some(ItemId::new(4)), None, None, None, None, None]).unwrap();

    // The schema before #882 reads back as the same character.
    let migrated = decode_character_record(&legacy_bytes(&record, PRE_WIDE_IDS)).unwrap();
    assert_eq!(migrated, record);

    // Written again, it is in the current schema, with room for IDs past the old widths.
    let bytes = encode_character_record(&migrated).unwrap();
    assert_eq!(decode_character_record(&bytes).unwrap(), migrated);
    let mut wide = migrated;
    wide.quests.completed.insert(QuestId::new(70_000));
    wide.cooldowns[0].ability = AbilityId::new(70_000);
    let bytes = encode_character_record(&wide).unwrap();
    assert_eq!(decode_character_record(&bytes).unwrap(), wide);
}
