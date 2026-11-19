//! Durable character record encoding (#30).
//!
//! `version u8 · schema u16 · id length u8 · id · appearance u8 · level u8 ·
//! experience u32 · copper u32 · inventory · equipment · quest log · unlocked waystones ·
//! depths record (`thorn_defeated` u8, receipt run u64, credited u8, drops u8) ·
//! health u32 · resource u16 · resource ticks u16 · mana delay u16 · global
//! cooldown u16 · cooldown count u8 · (ability u32 · remaining u16)* · zone
//! u32 · position 3 × i32 · facing u16 · physique · discovered areas · opened caches`,
//! big-endian (physique: see [`encode_physique`]; quest log: turned-in quest count u16 and IDs,
//! then entries; waystones, discovered areas and opened caches: count u16 and IDs). Every content ID is a `u32` (#882). The decoder is strict:
//! another version or schema, a truncated or over-long payload or an invalid
//! field fails closed. Gameplay validity against content is checked when a
//! zone admits the record.

use mmorpg_core::{
    AbilityId, CHARACTER_RECORD_SCHEMA_VERSION, CharacterId, CharacterLocation, CharacterRecord,
    ClassChoice, Cooldown, DepthsRecord, MAX_CHARACTER_ID_BYTES, MAX_COOLDOWNS, PHYSIQUE_PARAMS,
    Physique, QuestLog, WaystoneId, WaystoneSet, ZoneId,
};

use crate::ProtocolError;
use crate::inventory::{
    ItemIdWidth, decode_equipment_ids, decode_inventory_ids, encode_equipment, encode_inventory,
};
use crate::quest::{QuestIdWidth, decode_completed_mask, decode_entries, decode_log, encode_log};
use crate::wire::{decode_u8_count, encode_u8_count, read_u8, read_vector, take};

/// Version of the byte layout above, independent of the record schema.
pub const CHARACTER_RECORD_WIRE_VERSION: u8 = 1;

pub fn encode_character_record(record: &CharacterRecord) -> Result<Vec<u8>, ProtocolError> {
    let id = record.character_id.as_str().as_bytes();
    let mut payload = Vec::with_capacity(256);
    payload.push(CHARACTER_RECORD_WIRE_VERSION);
    payload.extend_from_slice(&CHARACTER_RECORD_SCHEMA_VERSION.to_be_bytes());
    payload.push(encode_u8_count(
        id.len(),
        MAX_CHARACTER_ID_BYTES,
        "character ID is too long",
    )?);
    payload.extend_from_slice(id);
    payload.push(
        u8::try_from(record.class.appearance())
            .map_err(|_| ProtocolError::new("class appearance is out of range"))?,
    );
    payload.push(record.level);
    payload.extend_from_slice(&record.experience.to_be_bytes());
    payload.extend_from_slice(&record.copper.to_be_bytes());
    encode_inventory(&mut payload, &record.inventory);
    encode_equipment(&mut payload, &record.equipment);
    encode_log(&mut payload, &record.quests)?;
    crate::wire::encode_waystones(&mut payload, &record.waystones)?;
    encode_depths_record(&mut payload, record.depths);
    payload.extend_from_slice(&record.health.to_be_bytes());
    for value in [
        record.resource,
        record.resource_ticks,
        record.mana_delay,
        record.global_cooldown,
    ] {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.push(encode_u8_count(
        record.cooldowns.len(),
        MAX_COOLDOWNS,
        "character has too many cooldowns",
    )?);
    for cooldown in &record.cooldowns {
        payload.extend_from_slice(&cooldown.ability.get().to_be_bytes());
        payload.extend_from_slice(&cooldown.remaining.to_be_bytes());
    }
    let location = record.location;
    payload.extend_from_slice(&location.zone_id.get().to_be_bytes());
    for component in location.position {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    payload.extend_from_slice(&location.facing.to_be_bytes());
    encode_physique(&mut payload, &record.physique);
    crate::wire::encode_discoveries(&mut payload, &record.discoveries)?;
    crate::wire::encode_caches(&mut payload, &record.caches)?;
    Ok(payload)
}

pub fn decode_character_record(payload: &[u8]) -> Result<CharacterRecord, ProtocolError> {
    let offset = &mut 0;
    if read_u8(payload, offset)? != CHARACTER_RECORD_WIRE_VERSION {
        return Err(ProtocolError::new(
            "unsupported character record wire version",
        ));
    }
    let schema = u16::from_be_bytes(take(payload, offset)?);
    let (has_physique, quest_set, wide_ids, has_discoveries, has_caches) = match schema {
        CHARACTER_RECORD_SCHEMA_VERSION => (true, true, true, true, true),
        PRE_CACHE_SCHEMA_VERSION => (true, true, true, true, false),
        PRE_DISCOVERY_SCHEMA_VERSION => (true, true, true, false, false),
        PRE_WIDE_ID_SCHEMA_VERSION => (true, true, false, false, false),
        PRE_QUEST_SET_SCHEMA_VERSION => (true, false, false, false, false),
        PRE_PHYSIQUE_SCHEMA_VERSION => (false, false, false, false, false),
        _ => {
            return Err(ProtocolError::new(
                "unsupported character record schema version",
            ));
        }
    };
    let id_length = decode_u8_count(
        payload,
        offset,
        MAX_CHARACTER_ID_BYTES,
        "character ID is too long",
    )?;
    let end = *offset + id_length;
    let id = payload
        .get(*offset..end)
        .ok_or_else(|| ProtocolError::new("payload is truncated"))?;
    *offset = end;
    let character_id = std::str::from_utf8(id)
        .ok()
        .and_then(|id| CharacterId::new(id).ok())
        .ok_or_else(|| ProtocolError::new("character ID is invalid"))?;
    let class = ClassChoice::from_appearance(u32::from(read_u8(payload, offset)?))
        .ok_or_else(|| ProtocolError::new("character class is invalid"))?;
    let level = read_u8(payload, offset)?;
    let experience = u32::from_be_bytes(take(payload, offset)?);
    let copper = u32::from_be_bytes(take(payload, offset)?);
    let (items, quest_ids) = if wide_ids {
        (ItemIdWidth::Word, QuestIdWidth::Word)
    } else {
        (ItemIdWidth::Half, QuestIdWidth::Byte)
    };
    let inventory = decode_inventory_ids(payload, offset, items)?;
    let equipment = decode_equipment_ids(payload, offset, items)?;
    let quests = if quest_set {
        decode_log(payload, offset, quest_ids)?
    } else {
        let completed = decode_completed_mask(payload, offset)?;
        QuestLog {
            entries: decode_entries(payload, offset, quest_ids)?,
            completed,
        }
    };
    let waystones = if wide_ids {
        crate::wire::decode_waystones(payload, offset)?
    } else {
        decode_waystone_mask(u32::from_be_bytes(take(payload, offset)?))?
    };
    let depths = decode_depths_record(payload, offset)?;
    let health = u32::from_be_bytes(take(payload, offset)?);
    let resource = u16::from_be_bytes(take(payload, offset)?);
    let resource_ticks = u16::from_be_bytes(take(payload, offset)?);
    let mana_delay = u16::from_be_bytes(take(payload, offset)?);
    let global_cooldown = u16::from_be_bytes(take(payload, offset)?);
    let cooldown_count = decode_u8_count(
        payload,
        offset,
        MAX_COOLDOWNS,
        "character has too many cooldowns",
    )?;
    let mut cooldowns = Vec::with_capacity(cooldown_count);
    for _ in 0..cooldown_count {
        let ability = AbilityId::new(if wide_ids {
            u32::from_be_bytes(take(payload, offset)?)
        } else {
            u32::from(read_u8(payload, offset)?)
        });
        cooldowns.push(Cooldown {
            ability,
            remaining: u16::from_be_bytes(take(payload, offset)?),
        });
    }
    let zone_id = ZoneId::new(u32::from_be_bytes(take(payload, offset)?));
    let position = read_vector(payload, offset)?;
    let facing = u16::from_be_bytes(take(payload, offset)?);
    let physique = if has_physique {
        decode_physique(payload, offset)?
    } else {
        Physique::NEUTRAL
    };
    let discoveries = if has_discoveries {
        crate::wire::decode_discoveries(payload, offset)?
    } else {
        std::collections::BTreeSet::new()
    };
    let caches = if has_caches {
        crate::wire::decode_caches(payload, offset)?
    } else {
        std::collections::BTreeSet::new()
    };
    if *offset != payload.len() {
        return Err(ProtocolError::new(
            "character record contains trailing bytes",
        ));
    }
    Ok(CharacterRecord {
        character_id,
        class,
        level,
        experience,
        copper,
        inventory,
        equipment,
        quests,
        waystones,
        discoveries,
        caches,
        depths,
        health,
        resource,
        resource_ticks,
        mana_delay,
        global_cooldown,
        cooldowns,
        location: CharacterLocation {
            zone_id,
            position,
            facing,
        },
        physique,
    })
}

/// The record schema before discovered areas (#916). Such records still load with none
/// discovered; they are written again in the current schema.
const PRE_DISCOVERY_SCHEMA_VERSION: u16 = 0xb809;
/// The record schema before opened discovery caches (#913). Such records still load with none
/// opened; they are written again in the current schema.
const PRE_CACHE_SCHEMA_VERSION: u16 = 0x51ee;

/// The record schema before physique (#633). Such records still load, with every slider neutral;
/// they are written again in the current schema.
const PRE_PHYSIQUE_SCHEMA_VERSION: u16 = 0x6671;

/// The record schema before the turned-in quest set (#862), which stored a `u32` mask (bit
/// `id - 1`). Such records still load, migrated to the set; they are written again in the
/// current schema.
const PRE_QUEST_SET_SCHEMA_VERSION: u16 = 0x9d41;

/// The record schema before 32-bit content IDs and the unlocked waystone set (#882): item IDs
/// were `u16`, quest and ability IDs one byte, and the unlocked waystones a `u32` mask (bit
/// `id - 1`). Such records still load, migrated; they are written again in the current schema.
const PRE_WIDE_ID_SCHEMA_VERSION: u16 = 0x20ba;

/// The unlocked waystones of a record before #882: bit `id - 1` per stone of the network.
fn decode_waystone_mask(mask: u32) -> Result<WaystoneSet, ProtocolError> {
    WaystoneSet::from_ids(
        (1..=32_u32)
            .filter(|id| mask & (1 << (id - 1)) != 0)
            .filter_map(WaystoneId::new),
    )
    .ok_or_else(|| ProtocolError::new("unlocked waystones name an unknown stone"))
}

/// Physique (#633): `count u8 · count × value u16`, each value at most
/// [`mmorpg_core::PHYSIQUE_SCALE`], in canonical parameter order. A shorter list (such as a
/// five-slider record) leaves the missing sliders neutral; more than eleven values fail closed.
pub(crate) fn encode_physique(payload: &mut Vec<u8>, physique: &Physique) {
    let values = physique.fixed();
    // Eleven values: always fits.
    #[allow(clippy::cast_possible_truncation)]
    payload.push(values.len() as u8);
    for value in values {
        payload.extend_from_slice(&value.to_be_bytes());
    }
}

pub(crate) fn decode_physique(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Physique, ProtocolError> {
    let count = decode_u8_count(
        payload,
        offset,
        PHYSIQUE_PARAMS,
        "physique has too many values",
    )?;
    let mut values = Physique::NEUTRAL.fixed();
    for value in values.iter_mut().take(count) {
        *value = u16::from_be_bytes(take(payload, offset)?);
    }
    Physique::from_fixed(values).map_err(|_| ProtocolError::new("physique value is out of range"))
}

/// `thorn_defeated` u8 · receipt run u64 · credited u8 · drops u8.
pub(crate) fn encode_depths_record(payload: &mut Vec<u8>, record: DepthsRecord) {
    payload.push(u8::from(record.thorn_defeated));
    payload.extend_from_slice(&record.receipt_run.to_be_bytes());
    payload.push(u8::from(record.credited));
    payload.push(record.drops);
}

pub(crate) fn decode_depths_record(
    payload: &[u8],
    offset: &mut usize,
) -> Result<DepthsRecord, ProtocolError> {
    Ok(DepthsRecord {
        thorn_defeated: read_u8(payload, offset)? != 0,
        receipt_run: u64::from_be_bytes(take(payload, offset)?),
        credited: read_u8(payload, offset)? != 0,
        drops: read_u8(payload, offset)?,
    })
}

/// A present flag, then `u8` length and bytes: a canonical player's durable character.
pub(crate) fn encode_optional_character(
    payload: &mut Vec<u8>,
    character: Option<&CharacterId>,
) -> Result<(), ProtocolError> {
    payload.push(u8::from(character.is_some()));
    if let Some(character) = character {
        let bytes = character.as_str().as_bytes();
        payload.push(encode_u8_count(
            bytes.len(),
            MAX_CHARACTER_ID_BYTES,
            "character ID is too long",
        )?);
        payload.extend_from_slice(bytes);
    }
    Ok(())
}

pub(crate) fn decode_optional_character(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<CharacterId>, ProtocolError> {
    if read_u8(payload, offset)? == 0 {
        return Ok(None);
    }
    let length = decode_u8_count(
        payload,
        offset,
        MAX_CHARACTER_ID_BYTES,
        "character ID is too long",
    )?;
    let end = *offset + length;
    let id = payload
        .get(*offset..end)
        .ok_or_else(|| ProtocolError::new("payload is truncated"))?;
    *offset = end;
    std::str::from_utf8(id)
        .ok()
        .and_then(|id| CharacterId::new(id).ok())
        .map(Some)
        .ok_or_else(|| ProtocolError::new("character ID is invalid"))
}
