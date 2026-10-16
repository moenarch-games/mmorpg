//! Byte-level primitives shared by the snapshot scopes: the common prefix,
//! entity references, gameplay events and strict readers.

use mmorpg_core::{
    AbilityId, EntityKind, EntityRef, ErrorCode, MAX_QUEST_OBJECTIVES, QuestId,
    SNAPSHOT_SCHEMA_VERSION, ZoneEvent, ZoneId, ability_by_id,
};

use crate::{ProtocolError, SNAPSHOT_WIRE_VERSION};

pub(crate) const CANONICAL_SNAPSHOT_SCOPE: u8 = 1;
pub(crate) const PLAYER_SNAPSHOT_SCOPE: u8 = 2;
/// Wire version, scope, schema, zone ID and tick start every snapshot.
pub(crate) const COMMON_HEADER_BYTES: usize = 16;
/// Kind code then `u32` ID; kind 0 (with ID 0) means none.
pub(crate) const ENTITY_REF_BYTES: usize = 5;

const NO_ENTITY: u8 = 0;
const PLAYER_KIND: u8 = 1;
const CREATURE_KIND: u8 = 2;
const NPC_KIND: u8 = 3;

const DAMAGE_DEALT: u8 = 1;
const DAMAGE_TAKEN: u8 = 2;
const MISS: u8 = 3;
const DIED: u8 = 4;
const EVADE: u8 = 5;
const ERROR: u8 = 6;
const CAST_STARTED: u8 = 7;
const ABILITY_USED: u8 = 8;
const HEALED: u8 = 9;
const AURA_APPLIED: u8 = 10;
const AURA_REMOVED: u8 = 11;
const INTERRUPTED: u8 = 12;
const ABSORBED: u8 = 13;
const QUEST_PROGRESS: u8 = 14;
const QUEST_COMPLETED: u8 = 15;
const CRITICAL_FLAG: u8 = 1;

pub(crate) const fn entity_kind_code(kind: EntityKind) -> u8 {
    match kind {
        EntityKind::Player => PLAYER_KIND,
        EntityKind::Creature => CREATURE_KIND,
        EntityKind::Npc => NPC_KIND,
    }
}

pub(crate) fn decode_entity_kind(code: u8) -> Result<EntityKind, ProtocolError> {
    match code {
        PLAYER_KIND => Ok(EntityKind::Player),
        CREATURE_KIND => Ok(EntityKind::Creature),
        NPC_KIND => Ok(EntityKind::Npc),
        _ => Err(ProtocolError::new("unknown entity kind")),
    }
}

pub(crate) fn encode_entity_ref(payload: &mut Vec<u8>, entity: Option<EntityRef>) {
    match entity {
        Some(entity) => {
            payload.push(entity_kind_code(entity.kind()));
            payload.extend_from_slice(&entity.id().to_be_bytes());
        }
        None => payload.extend_from_slice(&[NO_ENTITY, 0, 0, 0, 0]),
    }
}

pub(crate) fn decode_entity_ref(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<EntityRef>, ProtocolError> {
    let code = read_u8(payload, offset)?;
    let id = u32::from_be_bytes(take(payload, offset)?);
    if code == NO_ENTITY {
        return if id == 0 {
            Ok(None)
        } else {
            Err(ProtocolError::new("an absent entity must have ID 0"))
        };
    }
    Ok(Some(EntityRef::new(decode_entity_kind(code)?, id)))
}

const fn error_code(code: ErrorCode) -> u16 {
    match code {
        ErrorCode::NoTarget => 1,
        ErrorCode::OutOfRange => 2,
        ErrorCode::TargetDead => 3,
        ErrorCode::NotAttackable => 4,
        ErrorCode::YouAreDead => 5,
        ErrorCode::NotDead => 6,
        ErrorCode::InvalidTarget => 7,
        ErrorCode::TooManyIntents => 8,
        ErrorCode::InvalidInventoryMove => 9,
        ErrorCode::InventoryFull => 10,
        ErrorCode::InvalidLoot => 11,
        ErrorCode::NotLootOwner => 12,
        ErrorCode::EmptyLoot => 13,
        ErrorCode::MoneyOverflow => 14,
        ErrorCode::NoClass => 15,
        ErrorCode::NotLearned => 16,
        ErrorCode::NotReady => 17,
        ErrorCode::NotEnoughResource => 18,
        ErrorCode::Stunned => 19,
        ErrorCode::AlreadyCasting => 20,
        ErrorCode::InvalidClass => 21,
        ErrorCode::NotEquippable => 22,
        ErrorCode::InvalidVendor => 23,
        ErrorCode::NotEnoughMoney => 24,
        ErrorCode::ChatThrottled => 25,
        ErrorCode::InvalidQuest => 26,
        ErrorCode::QuestLogFull => 27,
        ErrorCode::QuestIncomplete => 28,
    }
}

fn decode_error_code(code: u16) -> Result<ErrorCode, ProtocolError> {
    Ok(match code {
        1 => ErrorCode::NoTarget,
        2 => ErrorCode::OutOfRange,
        3 => ErrorCode::TargetDead,
        4 => ErrorCode::NotAttackable,
        5 => ErrorCode::YouAreDead,
        6 => ErrorCode::NotDead,
        7 => ErrorCode::InvalidTarget,
        8 => ErrorCode::TooManyIntents,
        9 => ErrorCode::InvalidInventoryMove,
        10 => ErrorCode::InventoryFull,
        11 => ErrorCode::InvalidLoot,
        12 => ErrorCode::NotLootOwner,
        13 => ErrorCode::EmptyLoot,
        14 => ErrorCode::MoneyOverflow,
        15 => ErrorCode::NoClass,
        16 => ErrorCode::NotLearned,
        17 => ErrorCode::NotReady,
        18 => ErrorCode::NotEnoughResource,
        19 => ErrorCode::Stunned,
        20 => ErrorCode::AlreadyCasting,
        21 => ErrorCode::InvalidClass,
        22 => ErrorCode::NotEquippable,
        23 => ErrorCode::InvalidVendor,
        24 => ErrorCode::NotEnoughMoney,
        25 => ErrorCode::ChatThrottled,
        26 => ErrorCode::InvalidQuest,
        27 => ErrorCode::QuestLogFull,
        28 => ErrorCode::QuestIncomplete,
        _ => return Err(ProtocolError::new("unknown error code")),
    })
}

/// A quest ID, `1..=MAX_QUESTS`; whether content defines it is the
/// client's concern.
pub(crate) fn decode_quest(code: u8) -> Result<QuestId, ProtocolError> {
    let quest = QuestId::new(code);
    if quest.bit().is_some() {
        Ok(quest)
    } else {
        Err(ProtocolError::new("quest id is out of range"))
    }
}

/// A catalog ability ID carried in an event's flag byte.
pub(crate) fn decode_ability(code: u8) -> Result<AbilityId, ProtocolError> {
    let ability = AbilityId::new(code);
    if ability_by_id(ability).is_some() {
        Ok(ability)
    } else {
        Err(ProtocolError::new("unknown ability"))
    }
}

/// 14 bytes: kind, flags (or the ability ID of ability events, or the
/// quest ID of quest events), source reference, target reference, `u16`
/// amount (a quest progress event packs the objective index high and the
/// count low).
pub(crate) fn encode_event(payload: &mut Vec<u8>, event: &ZoneEvent) {
    let (kind, flags, source, target, amount) = match *event {
        ZoneEvent::DamageDealt {
            source,
            target,
            amount,
            critical,
        } => (
            DAMAGE_DEALT,
            u8::from(critical),
            Some(source),
            Some(target),
            amount,
        ),
        ZoneEvent::DamageTaken {
            source,
            target,
            amount,
            critical,
        } => (
            DAMAGE_TAKEN,
            u8::from(critical),
            Some(source),
            Some(target),
            amount,
        ),
        ZoneEvent::Miss { source, target } => (MISS, 0, Some(source), Some(target), 0),
        ZoneEvent::Died { entity, killer } => (DIED, 0, killer, Some(entity), 0),
        ZoneEvent::Evade { source, target } => (EVADE, 0, Some(source), Some(target), 0),
        ZoneEvent::Error { code, target } => (ERROR, 0, None, target, error_code(code)),
        ZoneEvent::CastStarted {
            source,
            target,
            ability,
            ticks,
        } => (CAST_STARTED, ability.get(), Some(source), target, ticks),
        ZoneEvent::AbilityUsed {
            source,
            target,
            ability,
        } => (ABILITY_USED, ability.get(), Some(source), target, 0),
        ZoneEvent::Healed {
            source,
            target,
            ability,
            amount,
        } => (HEALED, ability.get(), Some(source), Some(target), amount),
        ZoneEvent::AuraApplied {
            source,
            target,
            ability,
            ticks,
        } => (
            AURA_APPLIED,
            ability.get(),
            Some(source),
            Some(target),
            ticks,
        ),
        ZoneEvent::AuraRemoved {
            source,
            target,
            ability,
        } => (AURA_REMOVED, ability.get(), Some(source), Some(target), 0),
        ZoneEvent::Interrupted {
            source,
            target,
            ability,
        } => (INTERRUPTED, ability.get(), source, Some(target), 0),
        ZoneEvent::Absorbed {
            source,
            target,
            amount,
        } => (ABSORBED, 0, Some(source), Some(target), amount),
        ZoneEvent::QuestProgress {
            quest,
            objective,
            count,
        } => (
            QUEST_PROGRESS,
            quest.get(),
            None,
            None,
            u16::from_be_bytes([objective, count]),
        ),
        ZoneEvent::QuestCompleted { quest } => (QUEST_COMPLETED, quest.get(), None, None, 0),
    };
    payload.push(kind);
    payload.push(flags);
    encode_entity_ref(payload, source);
    encode_entity_ref(payload, target);
    payload.extend_from_slice(&amount.to_be_bytes());
}

pub(crate) fn decode_event(payload: &[u8], offset: &mut usize) -> Result<ZoneEvent, ProtocolError> {
    let kind = read_u8(payload, offset)?;
    let flags = read_u8(payload, offset)?;
    let source = decode_entity_ref(payload, offset)?;
    let target = decode_entity_ref(payload, offset)?;
    let amount = u16::from_be_bytes(take(payload, offset)?);
    let invalid = || ProtocolError::new("malformed event record");
    let damage = |critical_allowed: bool| -> Result<bool, ProtocolError> {
        match flags {
            0 => Ok(false),
            CRITICAL_FLAG if critical_allowed => Ok(true),
            _ => Err(ProtocolError::new("event flags are invalid")),
        }
    };
    let pair = || source.zip(target).ok_or_else(invalid);
    Ok(match kind {
        DAMAGE_DEALT | DAMAGE_TAKEN => {
            let critical = damage(true)?;
            let (source, target) = pair()?;
            if kind == DAMAGE_DEALT {
                ZoneEvent::DamageDealt {
                    source,
                    target,
                    amount,
                    critical,
                }
            } else {
                ZoneEvent::DamageTaken {
                    source,
                    target,
                    amount,
                    critical,
                }
            }
        }
        MISS | EVADE => {
            damage(false)?;
            let (source, target) = pair()?;
            if amount != 0 {
                return Err(invalid());
            }
            if kind == MISS {
                ZoneEvent::Miss { source, target }
            } else {
                ZoneEvent::Evade { source, target }
            }
        }
        DIED => {
            damage(false)?;
            let entity = target.ok_or_else(invalid)?;
            if amount != 0 {
                return Err(invalid());
            }
            ZoneEvent::Died {
                entity,
                killer: source,
            }
        }
        ERROR => {
            damage(false)?;
            if source.is_some() {
                return Err(invalid());
            }
            ZoneEvent::Error {
                code: decode_error_code(amount)?,
                target,
            }
        }
        CAST_STARTED | ABILITY_USED => {
            let ability = decode_ability(flags)?;
            let source = source.ok_or_else(invalid)?;
            if kind == CAST_STARTED {
                ZoneEvent::CastStarted {
                    source,
                    target,
                    ability,
                    ticks: amount,
                }
            } else {
                if amount != 0 {
                    return Err(invalid());
                }
                ZoneEvent::AbilityUsed {
                    source,
                    target,
                    ability,
                }
            }
        }
        HEALED | AURA_APPLIED | AURA_REMOVED => {
            let ability = decode_ability(flags)?;
            let (source, target) = pair()?;
            match kind {
                HEALED => ZoneEvent::Healed {
                    source,
                    target,
                    ability,
                    amount,
                },
                AURA_APPLIED => ZoneEvent::AuraApplied {
                    source,
                    target,
                    ability,
                    ticks: amount,
                },
                _ if amount == 0 => ZoneEvent::AuraRemoved {
                    source,
                    target,
                    ability,
                },
                _ => return Err(invalid()),
            }
        }
        INTERRUPTED => {
            let ability = decode_ability(flags)?;
            let target = target.ok_or_else(invalid)?;
            if amount != 0 {
                return Err(invalid());
            }
            ZoneEvent::Interrupted {
                source,
                target,
                ability,
            }
        }
        ABSORBED => {
            damage(false)?;
            let (source, target) = pair()?;
            ZoneEvent::Absorbed {
                source,
                target,
                amount,
            }
        }
        QUEST_PROGRESS | QUEST_COMPLETED => {
            let quest = decode_quest(flags)?;
            if source.is_some() || target.is_some() {
                return Err(invalid());
            }
            let [objective, count] = amount.to_be_bytes();
            if kind == QUEST_COMPLETED {
                if amount != 0 {
                    return Err(invalid());
                }
                ZoneEvent::QuestCompleted { quest }
            } else {
                if usize::from(objective) >= MAX_QUEST_OBJECTIVES || count == 0 {
                    return Err(invalid());
                }
                ZoneEvent::QuestProgress {
                    quest,
                    objective,
                    count,
                }
            }
        }
        _ => return Err(ProtocolError::new("unknown event kind")),
    })
}

/// Packs booleans into a flag byte, bit 0 first.
pub(crate) fn flag_byte(flags: &[bool]) -> u8 {
    flags
        .iter()
        .enumerate()
        .fold(0, |byte, (bit, &set)| byte | (u8::from(set) << bit))
}

/// Unpacks `N` flag bits; any higher bit fails.
pub(crate) fn read_flags<const N: usize>(byte: u8) -> Result<[bool; N], ProtocolError> {
    if u32::from(byte) >> N != 0 {
        return Err(ProtocolError::new("reserved flag bits are set"));
    }
    Ok(std::array::from_fn(|bit| byte & (1 << bit) != 0))
}

pub(crate) fn read_bool(payload: &[u8], offset: &mut usize) -> Result<bool, ProtocolError> {
    match read_u8(payload, offset)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ProtocolError::new("boolean field must be 0 or 1")),
    }
}

pub(crate) fn encode_common_header(
    payload: &mut Vec<u8>,
    scope: u8,
    schema_version: u16,
    zone_id: ZoneId,
    tick: u64,
) -> Result<(), ProtocolError> {
    if schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(ProtocolError::new(
            "unsupported core snapshot schema version",
        ));
    }
    payload.push(SNAPSHOT_WIRE_VERSION);
    payload.push(scope);
    payload.extend_from_slice(&schema_version.to_be_bytes());
    payload.extend_from_slice(&zone_id.get().to_be_bytes());
    payload.extend_from_slice(&tick.to_be_bytes());
    Ok(())
}

pub(crate) fn decode_common_header(
    payload: &[u8],
    offset: &mut usize,
    expected_scope: u8,
) -> Result<(u16, ZoneId, u64), ProtocolError> {
    if read_u8(payload, offset)? != SNAPSHOT_WIRE_VERSION {
        return Err(ProtocolError::new("unsupported snapshot wire version"));
    }
    if read_u8(payload, offset)? != expected_scope {
        return Err(ProtocolError::new("unexpected snapshot scope"));
    }
    let schema_version = u16::from_be_bytes(take(payload, offset)?);
    if schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(ProtocolError::new(
            "unsupported core snapshot schema version",
        ));
    }
    let zone_id = ZoneId::new(u32::from_be_bytes(take(payload, offset)?));
    let tick = u64::from_be_bytes(take(payload, offset)?);
    Ok((schema_version, zone_id, tick))
}

pub(crate) fn encode_u8_count(
    count: usize,
    limit: usize,
    message: &str,
) -> Result<u8, ProtocolError> {
    if count > limit {
        return Err(ProtocolError::new(message));
    }
    u8::try_from(count).map_err(|_| ProtocolError::new(message))
}

pub(crate) fn encode_u16_count(
    count: usize,
    limit: usize,
    message: &str,
) -> Result<u16, ProtocolError> {
    if count > limit {
        return Err(ProtocolError::new(message));
    }
    u16::try_from(count).map_err(|_| ProtocolError::new(message))
}

/// Rejects excessive counts before any allocation sized by them.
pub(crate) fn decode_u8_count(
    payload: &[u8],
    offset: &mut usize,
    limit: usize,
    message: &str,
) -> Result<usize, ProtocolError> {
    let count = usize::from(read_u8(payload, offset)?);
    if count > limit {
        return Err(ProtocolError::new(message));
    }
    Ok(count)
}

/// Rejects excessive counts before any allocation sized by them.
pub(crate) fn decode_u16_count(
    payload: &[u8],
    offset: &mut usize,
    limit: usize,
    message: &str,
) -> Result<usize, ProtocolError> {
    let count = usize::from(u16::from_be_bytes(take(payload, offset)?));
    if count > limit {
        return Err(ProtocolError::new(message));
    }
    Ok(count)
}

pub(crate) fn ensure_fully_consumed(payload: &[u8], offset: usize) -> Result<(), ProtocolError> {
    if offset != payload.len() {
        return Err(ProtocolError::new(
            "snapshot payload contains trailing bytes",
        ));
    }
    Ok(())
}

pub(crate) fn read_u8(payload: &[u8], offset: &mut usize) -> Result<u8, ProtocolError> {
    Ok(take::<1>(payload, offset)?[0])
}

pub(crate) fn read_vector(payload: &[u8], offset: &mut usize) -> Result<[i32; 3], ProtocolError> {
    Ok([
        i32::from_be_bytes(take(payload, offset)?),
        i32::from_be_bytes(take(payload, offset)?),
        i32::from_be_bytes(take(payload, offset)?),
    ])
}

pub(crate) fn take<const N: usize>(
    payload: &[u8],
    offset: &mut usize,
) -> Result<[u8; N], ProtocolError> {
    let end = offset
        .checked_add(N)
        .ok_or_else(|| ProtocolError::new("payload offset overflow"))?;
    let bytes = payload
        .get(*offset..end)
        .ok_or_else(|| ProtocolError::new("payload is truncated"))?;
    let array = bytes
        .try_into()
        .map_err(|_| ProtocolError::new("invalid payload width"))?;
    *offset = end;
    Ok(array)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_have_stable_distinct_wire_values() {
        let codes = [
            ErrorCode::NoTarget,
            ErrorCode::OutOfRange,
            ErrorCode::TargetDead,
            ErrorCode::NotAttackable,
            ErrorCode::YouAreDead,
            ErrorCode::NotDead,
            ErrorCode::InvalidTarget,
            ErrorCode::TooManyIntents,
            ErrorCode::InvalidInventoryMove,
            ErrorCode::InventoryFull,
            ErrorCode::InvalidLoot,
            ErrorCode::NotLootOwner,
            ErrorCode::EmptyLoot,
            ErrorCode::MoneyOverflow,
            ErrorCode::NoClass,
            ErrorCode::NotLearned,
            ErrorCode::NotReady,
            ErrorCode::NotEnoughResource,
            ErrorCode::Stunned,
            ErrorCode::AlreadyCasting,
            ErrorCode::InvalidClass,
            ErrorCode::NotEquippable,
            ErrorCode::InvalidVendor,
            ErrorCode::NotEnoughMoney,
            ErrorCode::ChatThrottled,
            ErrorCode::InvalidQuest,
            ErrorCode::QuestLogFull,
            ErrorCode::QuestIncomplete,
        ];
        for (wire, code) in (1..).zip(codes) {
            assert_eq!(error_code(code), wire, "{code:?}");
            assert_eq!(decode_error_code(wire).unwrap(), code);
        }
        for unknown in [0, 29, u16::MAX] {
            assert_eq!(
                decode_error_code(unknown).unwrap_err().to_string(),
                "unknown error code"
            );
        }
    }
}
