//! Byte-level primitives shared by the snapshot scopes: the common prefix,
//! entity references, gameplay events and strict readers.

use mmorpg_core::{
    AbilityId, EntityKind, EntityRef, ErrorCode, MAX_QUEST_OBJECTIVES, QuestId, StrikeAim,
    StrikeHeight, StrikeSide, ZoneEvent, ZoneId, ability_by_id,
};

use crate::{PROTOCOL_ID, ProtocolError};

pub(crate) const CANONICAL_SNAPSHOT_SCOPE: u8 = 1;
pub(crate) const PLAYER_SNAPSHOT_SCOPE: u8 = 2;
/// Protocol ID (3 bytes), scope, zone ID and tick start every snapshot.
pub(crate) const COMMON_HEADER_BYTES: usize = 16;
/// Kind code then `u32` ID; kind 0 (with ID 0) means none.
pub(crate) const ENTITY_REF_BYTES: usize = 5;

const NO_ENTITY: u8 = 0;
const PLAYER_KIND: u8 = 1;
const CREATURE_KIND: u8 = 2;
const NPC_KIND: u8 = 3;
const MINION_KIND: u8 = 4;

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
const ATTACK_PERFORMED: u8 = 16;
const DAMAGE_OBSERVED: u8 = 17;
const STRIKE_SWEPT: u8 = 18;
const BLOCKED: u8 = 19;
const GUARD_BROKEN: u8 = 20;
const LOST_INTEREST: u8 = 21;
const CRITICAL_FLAG: u8 = 1;

pub(crate) const fn entity_kind_code(kind: EntityKind) -> u8 {
    match kind {
        EntityKind::Player => PLAYER_KIND,
        EntityKind::Creature => CREATURE_KIND,
        EntityKind::Npc => NPC_KIND,
        EntityKind::Minion => MINION_KIND,
    }
}

pub(crate) fn decode_entity_kind(code: u8) -> Result<EntityKind, ProtocolError> {
    match code {
        PLAYER_KIND => Ok(EntityKind::Player),
        CREATURE_KIND => Ok(EntityKind::Creature),
        NPC_KIND => Ok(EntityKind::Npc),
        MINION_KIND => Ok(EntityKind::Minion),
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
        ErrorCode::DoorBlocked => 30,
        ErrorCode::LineOfSight => 29,
        ErrorCode::VendorClosed => 31,
        ErrorCode::DoorBusy => 32,
        ErrorCode::NoMinions => 33,
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
        29 => ErrorCode::LineOfSight,
        30 => ErrorCode::DoorBlocked,
        31 => ErrorCode::VendorClosed,
        32 => ErrorCode::DoorBusy,
        33 => ErrorCode::NoMinions,
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

/// A directed strike's aim as its side and height codes.
pub(crate) const fn encode_aim(aim: StrikeAim) -> [u8; 2] {
    [aim.side.code(), aim.height.code()]
}

pub(crate) fn decode_aim([side, height]: [u8; 2]) -> Result<StrikeAim, ProtocolError> {
    match (StrikeSide::from_code(side), StrikeHeight::from_code(height)) {
        (Some(side), Some(height)) => Ok(StrikeAim::new(side, height)),
        _ => Err(ProtocolError::new("unknown strike aim")),
    }
}

/// A guard lane as one byte: 0 lowered (or no lane), 1 + the height code.
pub(crate) const fn encode_lane(lane: Option<StrikeHeight>) -> u8 {
    match lane {
        Some(height) => 1 + height.code(),
        None => 0,
    }
}

pub(crate) fn decode_lane(code: u8) -> Result<Option<StrikeHeight>, ProtocolError> {
    match code {
        0 => Ok(None),
        code => StrikeHeight::from_code(code - 1)
            .map(Some)
            .ok_or_else(|| ProtocolError::new("unknown guard lane")),
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
/// count low; a strike sweep packs the aim's side code high and its height
/// code low). Guard events carry the lane's height code as flags, and the
/// blocked amount or the break's recovery ticks.
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
        ZoneEvent::AttackPerformed { source, target } => {
            (ATTACK_PERFORMED, 0, Some(source), Some(target), 0)
        }
        ZoneEvent::DamageObserved {
            source,
            target,
            amount,
            critical,
        } => (
            DAMAGE_OBSERVED,
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
        ZoneEvent::StrikeSwept {
            source,
            target,
            ability,
            aim,
        } => (
            STRIKE_SWEPT,
            ability.get(),
            Some(source),
            target,
            u16::from_be_bytes(encode_aim(aim)),
        ),
        ZoneEvent::Blocked {
            source,
            target,
            lane,
            amount,
        } => (BLOCKED, lane.code(), Some(source), Some(target), amount),
        ZoneEvent::GuardBroken {
            source,
            target,
            lane,
            ticks,
        } => (GUARD_BROKEN, lane.code(), Some(source), Some(target), ticks),
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
        ZoneEvent::LostInterest { source } => (LOST_INTEREST, 0, Some(source), None, 0),
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
        DAMAGE_DEALT | DAMAGE_TAKEN | DAMAGE_OBSERVED => {
            let critical = damage(true)?;
            let (source, target) = pair()?;
            if kind == DAMAGE_DEALT {
                ZoneEvent::DamageDealt {
                    source,
                    target,
                    amount,
                    critical,
                }
            } else if kind == DAMAGE_OBSERVED {
                ZoneEvent::DamageObserved {
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
        MISS | EVADE | ATTACK_PERFORMED => {
            damage(false)?;
            let (source, target) = pair()?;
            if amount != 0 {
                return Err(invalid());
            }
            if kind == MISS {
                ZoneEvent::Miss { source, target }
            } else if kind == ATTACK_PERFORMED {
                ZoneEvent::AttackPerformed { source, target }
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
        STRIKE_SWEPT => ZoneEvent::StrikeSwept {
            source: source.ok_or_else(invalid)?,
            target,
            ability: decode_ability(flags)?,
            aim: decode_aim(amount.to_be_bytes())?,
        },
        BLOCKED | GUARD_BROKEN => {
            let lane = StrikeHeight::from_code(flags)
                .ok_or_else(|| ProtocolError::new("unknown guard lane"))?;
            let (source, target) = pair()?;
            if kind == BLOCKED {
                ZoneEvent::Blocked {
                    source,
                    target,
                    lane,
                    amount,
                }
            } else {
                ZoneEvent::GuardBroken {
                    source,
                    target,
                    lane,
                    ticks: amount,
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
        LOST_INTEREST => {
            damage(false)?;
            let source = source.ok_or_else(invalid)?;
            if target.is_some() || amount != 0 {
                return Err(invalid());
            }
            ZoneEvent::LostInterest { source }
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

pub(crate) fn encode_common_header(payload: &mut Vec<u8>, scope: u8, zone_id: ZoneId, tick: u64) {
    payload.extend_from_slice(&protocol_id_bytes());
    payload.push(scope);
    payload.extend_from_slice(&zone_id.get().to_be_bytes());
    payload.extend_from_slice(&tick.to_be_bytes());
}

pub(crate) fn decode_common_header(
    payload: &[u8],
    offset: &mut usize,
    expected_scope: u8,
) -> Result<(ZoneId, u64), ProtocolError> {
    if take::<3>(payload, offset)? != protocol_id_bytes() {
        return Err(ProtocolError::new("unsupported snapshot protocol id"));
    }
    if read_u8(payload, offset)? != expected_scope {
        return Err(ProtocolError::new("unexpected snapshot scope"));
    }
    let zone_id = ZoneId::new(u32::from_be_bytes(take(payload, offset)?));
    let tick = u64::from_be_bytes(take(payload, offset)?);
    Ok((zone_id, tick))
}

/// The 24-bit [`PROTOCOL_ID`] as it leads every snapshot.
const fn protocol_id_bytes() -> [u8; 3] {
    let [_, high, middle, low] = PROTOCOL_ID.to_be_bytes();
    [high, middle, low]
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

/// Three finite big-endian `f32` values (exact canonical physics state, #271).
pub(crate) fn read_f32_vector(
    payload: &[u8],
    offset: &mut usize,
) -> Result<[f32; 3], ProtocolError> {
    let vector = [
        f32::from_be_bytes(take(payload, offset)?),
        f32::from_be_bytes(take(payload, offset)?),
        f32::from_be_bytes(take(payload, offset)?),
    ];
    if vector.iter().all(|value| value.is_finite()) {
        Ok(vector)
    } else {
        Err(ProtocolError::new("physics state must be finite"))
    }
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

/// A door-state list: count (`u8`), then door ID and open flag per door (#158).
pub(crate) fn encode_doors(
    payload: &mut Vec<u8>,
    doors: &[mmorpg_core::DoorView],
) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        doors.len(),
        mmorpg_core::MAX_DOORS,
        "too many doors",
    )?);
    for door in doors {
        payload.push(door.door.get());
        payload.push(u8::from(door.open));
    }
    Ok(())
}

pub(crate) fn decode_doors(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::DoorView>, ProtocolError> {
    let count = decode_u8_count(payload, offset, mmorpg_core::MAX_DOORS, "too many doors")?;
    let mut doors: Vec<mmorpg_core::DoorView> = Vec::with_capacity(count);
    for _ in 0..count {
        let door = mmorpg_core::DoorId::new(read_u8(payload, offset)?);
        let open = match read_u8(payload, offset)? {
            0 => false,
            1 => true,
            _ => return Err(ProtocolError::new("door state must be 0 or 1")),
        };
        if doors.last().is_some_and(|last| last.door >= door) {
            return Err(ProtocolError::new("doors must be in ascending ID order"));
        }
        doors.push(mmorpg_core::DoorView { door, open });
    }
    Ok(doors)
}

/// A destructible piece reference (#305): 0 for none, else the piece ID plus one.
pub(crate) fn encode_piece(piece: Option<mmorpg_core::PieceId>) -> u16 {
    piece.map_or(0, |piece| piece.get().saturating_add(1))
}

pub(crate) fn decode_piece(value: u16) -> Option<mmorpg_core::PieceId> {
    value.checked_sub(1).map(mmorpg_core::PieceId::new)
}

/// A minion order (#295): its code (1 follow, 2 attack, 3 defend, 4 retreat) then 8 bytes, the
/// attacked unit's reference and three zero bytes, the defended point's `x` and `z` (`i32`
/// each), or zeros.
pub(crate) const MINION_ORDER_BYTES: usize = 9;

pub(crate) fn encode_minion_order(payload: &mut Vec<u8>, order: mmorpg_core::MinionOrder) {
    use mmorpg_core::MinionOrder;
    match order {
        MinionOrder::Follow => payload.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0]),
        MinionOrder::Attack(target) => {
            payload.push(2);
            encode_entity_ref(payload, Some(target));
            payload.extend_from_slice(&[0; 3]);
        }
        MinionOrder::Defend([x, z]) => {
            payload.push(3);
            payload.extend_from_slice(&x.to_be_bytes());
            payload.extend_from_slice(&z.to_be_bytes());
        }
        MinionOrder::Retreat => payload.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0, 0, 0]),
    }
}

pub(crate) fn decode_minion_order(
    payload: &[u8],
    offset: &mut usize,
) -> Result<mmorpg_core::MinionOrder, ProtocolError> {
    use mmorpg_core::MinionOrder;
    let code = read_u8(payload, offset)?;
    let body: [u8; 8] = take(payload, offset)?;
    let zero = |bytes: &[u8]| {
        if bytes.iter().all(|&byte| byte == 0) {
            Ok(())
        } else {
            Err(ProtocolError::new("minion order padding must be zero"))
        }
    };
    Ok(match code {
        1 => {
            zero(&body)?;
            MinionOrder::Follow
        }
        2 => {
            zero(&body[5..])?;
            let mut at = 0;
            let target = decode_entity_ref(&body[..5], &mut at)?
                .ok_or_else(|| ProtocolError::new("an attack order needs a target"))?;
            MinionOrder::Attack(target)
        }
        3 => MinionOrder::Defend([
            i32::from_be_bytes([body[0], body[1], body[2], body[3]]),
            i32::from_be_bytes([body[4], body[5], body[6], body[7]]),
        ]),
        4 => {
            zero(&body)?;
            MinionOrder::Retreat
        }
        _ => return Err(ProtocolError::new("unknown minion order")),
    })
}

/// The not-intact pieces of a projection (#305): count (`u8`), then per piece its ID (`u16`)
/// and stage (`u8`), in ascending ID order.
pub(crate) fn encode_piece_views(
    payload: &mut Vec<u8>,
    pieces: &[mmorpg_core::PieceView],
) -> Result<(), ProtocolError> {
    payload.push(
        u8::try_from(pieces.len())
            .ok()
            .filter(|&count| usize::from(count) <= mmorpg_core::MAX_PIECES)
            .ok_or_else(|| ProtocolError::new("too many structure pieces"))?,
    );
    for piece in pieces {
        payload.extend_from_slice(&piece.piece.get().to_be_bytes());
        payload.push(piece.stage.code());
    }
    Ok(())
}

pub(crate) fn decode_piece_views(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::PieceView>, ProtocolError> {
    let count = usize::from(read_u8(payload, offset)?);
    if count > mmorpg_core::MAX_PIECES {
        return Err(ProtocolError::new("too many structure pieces"));
    }
    let mut pieces: Vec<mmorpg_core::PieceView> = Vec::with_capacity(count);
    for _ in 0..count {
        let piece = mmorpg_core::PieceId::new(u16::from_be_bytes([
            read_u8(payload, offset)?,
            read_u8(payload, offset)?,
        ]));
        let stage = mmorpg_core::PieceStage::from_code(read_u8(payload, offset)?)
            .filter(|stage| *stage != mmorpg_core::PieceStage::Intact)
            .ok_or_else(|| ProtocolError::new("unknown structure piece stage"))?;
        if pieces.last().is_some_and(|last| last.piece >= piece) {
            return Err(ProtocolError::new(
                "structure pieces must be in ascending ID order",
            ));
        }
        pieces.push(mmorpg_core::PieceView { piece, stage });
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_events_carry_their_lane_and_amount() {
        let wolf = EntityRef::Creature(mmorpg_core::CreatureId::new(3));
        let player = EntityRef::Player(9);
        for event in [
            ZoneEvent::Blocked {
                source: wolf,
                target: player,
                lane: StrikeHeight::Middle,
                amount: 4,
            },
            ZoneEvent::GuardBroken {
                source: wolf,
                target: player,
                lane: StrikeHeight::High,
                ticks: 45,
            },
        ] {
            let mut payload = Vec::new();
            encode_event(&mut payload, &event);
            assert_eq!(
                &payload[..2],
                [
                    if matches!(event, ZoneEvent::Blocked { .. }) {
                        BLOCKED
                    } else {
                        GUARD_BROKEN
                    },
                    event_lane(event)
                ]
            );
            assert_eq!(decode_event(&payload, &mut 0).unwrap(), event);
            payload[1] = 3;
            assert_eq!(
                decode_event(&payload, &mut 0).unwrap_err().to_string(),
                "unknown guard lane"
            );
        }
        assert_eq!(decode_lane(0).unwrap(), None);
        assert_eq!(decode_lane(3).unwrap(), Some(StrikeHeight::High));
        assert!(decode_lane(4).is_err());
    }

    fn event_lane(event: ZoneEvent) -> u8 {
        match event {
            ZoneEvent::Blocked { lane, .. } | ZoneEvent::GuardBroken { lane, .. } => lane.code(),
            _ => u8::MAX,
        }
    }

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
            ErrorCode::LineOfSight,
            ErrorCode::DoorBlocked,
            ErrorCode::VendorClosed,
            ErrorCode::DoorBusy,
            ErrorCode::NoMinions,
        ];
        for (wire, code) in (1..).zip(codes) {
            assert_eq!(error_code(code), wire, "{code:?}");
            assert_eq!(decode_error_code(wire).unwrap(), code);
        }
        for unknown in [0, 34, u16::MAX] {
            assert_eq!(
                decode_error_code(unknown).unwrap_err().to_string(),
                "unknown error code"
            );
        }
    }
}
