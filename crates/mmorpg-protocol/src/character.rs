//! Durable character record encoding (#30).
//!
//! `version u8 · schema u16 · id length u8 · id · appearance u8 · level u8 ·
//! experience u32 · copper u32 · inventory · equipment · quest entries ·
//! health u32 · resource u16 · resource ticks u16 · mana delay u16 · global
//! cooldown u16 · cooldown count u8 · (ability u8 · remaining u16)* · zone
//! u32 · position 3 × i32 · facing u16`, big-endian. The decoder is strict:
//! another version or schema, a truncated or over-long payload or an invalid
//! field fails closed. Gameplay validity against content is checked when a
//! zone admits the record.

use mmorpg_core::{
    AbilityId, CHARACTER_RECORD_SCHEMA_VERSION, CharacterId, CharacterLocation, CharacterRecord,
    ClassChoice, Cooldown, MAX_CHARACTER_ID_BYTES, MAX_COOLDOWNS, QuestLog, ZoneId,
};

use crate::ProtocolError;
use crate::inventory::{decode_equipment, decode_inventory, encode_equipment, encode_inventory};
use crate::quest::{decode_entries, encode_entries};
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
    encode_entries(
        &mut payload,
        record.quests.completed,
        &record.quests.entries,
    )?;
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
        payload.push(cooldown.ability.get());
        payload.extend_from_slice(&cooldown.remaining.to_be_bytes());
    }
    let location = record.location;
    payload.extend_from_slice(&location.zone_id.get().to_be_bytes());
    for component in location.position {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    payload.extend_from_slice(&location.facing.to_be_bytes());
    Ok(payload)
}

pub fn decode_character_record(payload: &[u8]) -> Result<CharacterRecord, ProtocolError> {
    let offset = &mut 0;
    if read_u8(payload, offset)? != CHARACTER_RECORD_WIRE_VERSION {
        return Err(ProtocolError::new(
            "unsupported character record wire version",
        ));
    }
    if u16::from_be_bytes(take(payload, offset)?) != CHARACTER_RECORD_SCHEMA_VERSION {
        return Err(ProtocolError::new(
            "unsupported character record schema version",
        ));
    }
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
    let class = ClassChoice::from_appearance(u16::from(read_u8(payload, offset)?))
        .ok_or_else(|| ProtocolError::new("character class is invalid"))?;
    let level = read_u8(payload, offset)?;
    let experience = u32::from_be_bytes(take(payload, offset)?);
    let copper = u32::from_be_bytes(take(payload, offset)?);
    let inventory = decode_inventory(payload, offset)?;
    let equipment = decode_equipment(payload, offset)?;
    let (completed, entries) = decode_entries(payload, offset)?;
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
        let ability = AbilityId::new(read_u8(payload, offset)?);
        cooldowns.push(Cooldown {
            ability,
            remaining: u16::from_be_bytes(take(payload, offset)?),
        });
    }
    let zone_id = ZoneId::new(u32::from_be_bytes(take(payload, offset)?));
    let position = read_vector(payload, offset)?;
    let facing = u16::from_be_bytes(take(payload, offset)?);
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
        quests: QuestLog { entries, completed },
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
    })
}
