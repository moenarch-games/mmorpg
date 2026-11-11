//! Physique V1 (#633) through the character store: every slider survives create, commit,
//! reload, reconnect and later editing, and records saved before physique load neutral.

use mmorpg_characters::{CharacterStore, FileEntries, LeaseHolder};
use mmorpg_core::{
    CharacterId, CharacterRecord, ClassChoice, PHYSIQUE_PARAMS, Physique, PhysiqueParam,
    PhysiquePreset, PlayerClass, Sex, ZoneId, ZoneSimulation, greyhaven_vale,
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

/// The bytes the record had before physique: the earlier schema identity and no physique tail.
fn pre_physique_bytes(record: &CharacterRecord) -> Vec<u8> {
    let mut bytes = encode_character_record(record).unwrap();
    bytes.truncate(bytes.len() - 1 - 2 * PHYSIQUE_PARAMS);
    bytes[1..3].copy_from_slice(&0x6671_u16.to_be_bytes());
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

    let legacy = decode_character_record(&pre_physique_bytes(&customized)).unwrap();
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
    let tail = bytes.len() - 1 - 2 * PHYSIQUE_PARAMS;
    let mut five = bytes[..tail].to_vec();
    five.push(5);
    five.extend_from_slice(&bytes[tail + 1..tail + 11]);
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
