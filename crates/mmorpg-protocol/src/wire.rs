//! Byte-level primitives shared by the snapshot scopes: the common prefix,
//! entity references, gameplay events and strict readers.

use std::collections::BTreeSet;

use mmorpg_core::{
    AbilityId, EntityKind, EntityRef, ErrorCode, MAX_QUEST_OBJECTIVES, QuestId, StrikeAim,
    StrikeHeight, StrikeSide, ZoneEvent, ZoneId, ability_by_id,
};

use crate::codec::{Codec, Field, record};
use crate::{PROTOCOL_ID, ProtocolError};

pub(crate) const CANONICAL_SNAPSHOT_SCOPE: u8 = 1;
pub(crate) const PLAYER_SNAPSHOT_SCOPE: u8 = 2;
/// A departed player's redirect notice in place of its projection (#145).
pub(crate) const REDIRECT_SCOPE: u8 = 3;
/// Protocol ID (3 bytes), scope, zone ID and tick start every snapshot.
pub(crate) const COMMON_HEADER_BYTES: usize = 16;
record! {
    /// An entity reference: kind code then `u32` ID; kind 0 (with ID 0) means none.
    pub(crate) struct EntityRefRecord {
        kind: u8,
        id: u32,
    }
}

/// Kind code then `u32` ID; kind 0 (with ID 0) means none.
pub(crate) const ENTITY_REF_BYTES: usize = EntityRefRecord::BYTES;

impl EntityRefRecord {
    pub(crate) fn of(entity: Option<EntityRef>) -> Self {
        entity.map_or(Self::default(), |entity| Self {
            kind: entity_kind_code(entity.kind()),
            id: entity.id(),
        })
    }

    pub(crate) fn entity(self) -> Result<Option<EntityRef>, ProtocolError> {
        match self {
            Self {
                kind: NO_ENTITY,
                id: 0,
            } => Ok(None),
            Self {
                kind: NO_ENTITY, ..
            } => Err(ProtocolError::new("an absent entity must have ID 0")),
            Self { kind, id } => Ok(Some(EntityRef::new(decode_entity_kind(kind)?, id))),
        }
    }
}

/// Every reference has a wire form; decoding fails closed on an unknown kind or an absent
/// reference with an ID.
impl Codec for Option<EntityRef> {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        EntityRefRecord::of(*self).put(payload);
        Ok(())
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        EntityRefRecord::read(payload, offset)?.entity()
    }
}

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
const RAID_NOTICE: u8 = 22;
const TRAP_HIT: u8 = 23;
const SWING_BEGAN: u8 = 24;
const CLASHED: u8 = 25;
const STRIKE_BEGAN: u8 = 26;
const CREATURE_ATTACK_BEGAN: u8 = 27;
const STRIKE_REWARDED: u8 = 28;
const FLANKED: u8 = 29;
const CRITICAL_FLAG: u32 = 1;

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
    EntityRefRecord::of(entity).put(payload);
}

/// An optional yaw (#884): a presence flag, then the yaw (`u16`), zero when absent.
pub(crate) fn encode_optional_yaw(payload: &mut Vec<u8>, yaw: Option<u16>) {
    payload.push(u8::from(yaw.is_some()));
    payload.extend_from_slice(&yaw.unwrap_or(0).to_be_bytes());
}

pub(crate) fn decode_optional_yaw(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<u16>, ProtocolError> {
    let present = read_bool(payload, offset)?;
    let yaw = u16::from_be_bytes(take(payload, offset)?);
    match (present, yaw) {
        (true, yaw) => Ok(Some(yaw)),
        (false, 0) => Ok(None),
        (false, _) => Err(ProtocolError::new("an absent yaw must be 0")),
    }
}

pub(crate) fn decode_entity_ref(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Option<EntityRef>, ProtocolError> {
    Option::<EntityRef>::decode(payload, offset)
}

pub const fn error_code(code: ErrorCode) -> u16 {
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
        ErrorCode::NotWhileSwimming => 34,
        ErrorCode::HandoffCorridor => 35,
        ErrorCode::InCombat => 36,
        ErrorCode::InvalidWaystone => 37,
        ErrorCode::PrivateHome => 38,
        ErrorCode::NoAnswer => 39,
        ErrorCode::NotAtNight => 40,
        ErrorCode::DepthsLocked => 41,
        ErrorCode::DepthsClosed => 42,
        ErrorCode::DepthsWipe => 43,
        ErrorCode::DepthsBossOpen => 44,
        ErrorCode::NoRoom => 45,
        ErrorCode::NoLanding => 46,
        ErrorCode::MovementImmune => 47,
        ErrorCode::SeatTaken => 48,
        ErrorCode::CannotSitHere => 49,
        ErrorCode::WeaponDrawn => 50,
        ErrorCode::MissingMaterials => 51,
        ErrorCode::WrongForm => 52,
        ErrorCode::RewardsBusy => 53,
        ErrorCode::Distrusted => 54,
        ErrorCode::NpcBusy => 55,
        ErrorCode::EscortLost => 56,
        ErrorCode::NotSellable => 57,
        ErrorCode::HandsFull => 58,
        ErrorCode::NotCarrying => 59,
        ErrorCode::NoFooting => 60,
        ErrorCode::LoadLost => 61,
        ErrorCode::SiteNotReady => 62,
        ErrorCode::SealOpen => 63,
        ErrorCode::Warded => 64,
        ErrorCode::FlockUnsettled => 65,
        ErrorCode::FlockFell => 66,
        ErrorCode::StrayBolted => 67,
        ErrorCode::OutOfOrder => 68,
        ErrorCode::StaffShown => 69,
    }
}

pub fn decode_error_code(code: u16) -> Result<ErrorCode, ProtocolError> {
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
        34 => ErrorCode::NotWhileSwimming,
        35 => ErrorCode::HandoffCorridor,
        36 => ErrorCode::InCombat,
        37 => ErrorCode::InvalidWaystone,
        38 => ErrorCode::PrivateHome,
        39 => ErrorCode::NoAnswer,
        40 => ErrorCode::NotAtNight,
        41 => ErrorCode::DepthsLocked,
        42 => ErrorCode::DepthsClosed,
        43 => ErrorCode::DepthsWipe,
        44 => ErrorCode::DepthsBossOpen,
        45 => ErrorCode::NoRoom,
        46 => ErrorCode::NoLanding,
        47 => ErrorCode::MovementImmune,
        48 => ErrorCode::SeatTaken,
        49 => ErrorCode::CannotSitHere,
        50 => ErrorCode::WeaponDrawn,
        51 => ErrorCode::MissingMaterials,
        52 => ErrorCode::WrongForm,
        53 => ErrorCode::RewardsBusy,
        54 => ErrorCode::Distrusted,
        55 => ErrorCode::NpcBusy,
        56 => ErrorCode::EscortLost,
        57 => ErrorCode::NotSellable,
        58 => ErrorCode::HandsFull,
        59 => ErrorCode::NotCarrying,
        60 => ErrorCode::NoFooting,
        61 => ErrorCode::LoadLost,
        62 => ErrorCode::SiteNotReady,
        63 => ErrorCode::SealOpen,
        64 => ErrorCode::Warded,
        65 => ErrorCode::FlockUnsettled,
        66 => ErrorCode::FlockFell,
        67 => ErrorCode::StrayBolted,
        68 => ErrorCode::OutOfOrder,
        69 => ErrorCode::StaffShown,
        _ => return Err(ProtocolError::new("unknown error code")),
    })
}

/// A waystone ID, never `0` (#145); whether the network has it is core's to decide.
pub(crate) fn decode_waystone(code: u32) -> Result<mmorpg_core::WaystoneId, ProtocolError> {
    mmorpg_core::WaystoneId::new(code).ok_or_else(|| ProtocolError::new("unknown waystone"))
}

/// Unlocked waystones (#145, #882): `count u16 · count × id u32`, strictly ascending, network
/// stones only.
pub(crate) fn encode_waystones(
    payload: &mut Vec<u8>,
    waystones: &mmorpg_core::WaystoneSet,
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(
            waystones.len(),
            usize::from(u16::MAX),
            "too many unlocked waystones",
        )?
        .to_be_bytes(),
    );
    for id in waystones.iter() {
        payload.extend_from_slice(&id.get().to_be_bytes());
    }
    Ok(())
}

pub(crate) fn decode_waystones(
    payload: &[u8],
    offset: &mut usize,
) -> Result<mmorpg_core::WaystoneSet, ProtocolError> {
    let count = decode_u16_count(
        payload,
        offset,
        usize::from(u16::MAX),
        "too many unlocked waystones",
    )?;
    let mut ids: Vec<mmorpg_core::WaystoneId> = Vec::with_capacity(count);
    for _ in 0..count {
        let id = decode_waystone(u32::from_be_bytes(take(payload, offset)?))?;
        if ids.last().is_some_and(|last| *last >= id) {
            return Err(ProtocolError::new("unlocked waystones are not ordered"));
        }
        ids.push(id);
    }
    mmorpg_core::WaystoneSet::from_ids(ids)
        .ok_or_else(|| ProtocolError::new("unlocked waystones name an unknown stone"))
}

/// Discovered areas (#916): `count u16 · count × id u32`, strictly ascending; whether content
/// names them is core's to decide.
pub(crate) fn encode_discoveries(
    payload: &mut Vec<u8>,
    areas: &std::collections::BTreeSet<mmorpg_core::AreaId>,
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(
            areas.len(),
            usize::from(u16::MAX),
            "too many discovered areas",
        )?
        .to_be_bytes(),
    );
    for id in areas {
        payload.extend_from_slice(&id.get().to_be_bytes());
    }
    Ok(())
}

/// Opened discovery caches (#913): `count u16 · count × id u32`, strictly ascending; whether
/// content names them is core's to decide.
pub(crate) fn encode_caches(
    payload: &mut Vec<u8>,
    caches: &std::collections::BTreeSet<u32>,
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(
            caches.len(),
            usize::from(u16::MAX),
            "too many opened caches",
        )?
        .to_be_bytes(),
    );
    for id in caches {
        payload.extend_from_slice(&id.to_be_bytes());
    }
    Ok(())
}

pub(crate) fn decode_discoveries(
    payload: &[u8],
    offset: &mut usize,
) -> Result<std::collections::BTreeSet<mmorpg_core::AreaId>, ProtocolError> {
    let count = decode_u16_count(
        payload,
        offset,
        usize::from(u16::MAX),
        "too many discovered areas",
    )?;
    let mut areas = std::collections::BTreeSet::new();
    for _ in 0..count {
        let id = mmorpg_core::AreaId::new(u32::from_be_bytes(take(payload, offset)?));
        if areas.last().is_some_and(|last| *last >= id) {
            return Err(ProtocolError::new("discovered areas are not ordered"));
        }
        areas.insert(id);
    }
    Ok(areas)
}

/// Weapon-family mastery (#915): the sword's `level u8 · progress u16`.
pub(crate) fn encode_mastery(payload: &mut Vec<u8>, mastery: mmorpg_core::WeaponMastery) {
    payload.push(mastery.sword.level);
    payload.extend_from_slice(&mastery.sword.progress.to_be_bytes());
}

pub(crate) fn decode_mastery(
    payload: &[u8],
    offset: &mut usize,
) -> Result<mmorpg_core::WeaponMastery, ProtocolError> {
    let mastery = mmorpg_core::WeaponMastery {
        sword: mmorpg_core::Mastery {
            level: read_u8(payload, offset)?,
            progress: u16::from_be_bytes(take(payload, offset)?),
        },
    };
    if !mastery.is_valid() {
        return Err(ProtocolError::new("weapon mastery is out of range"));
    }
    Ok(mastery)
}

/// Practice banked against living foes (#915): `count u8 · count × (creature u32 · life u64 ·
/// points u8)`.
pub(crate) fn encode_practice(
    payload: &mut Vec<u8>,
    practice: &[mmorpg_core::Practice],
) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        practice.len(),
        mmorpg_core::MAX_PRACTICE,
        "too much banked practice",
    )?);
    for entry in practice {
        payload.extend_from_slice(&entry.creature.get().to_be_bytes());
        payload.extend_from_slice(&entry.life.to_be_bytes());
        payload.push(entry.points);
    }
    Ok(())
}

pub(crate) fn decode_practice(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::Practice>, ProtocolError> {
    let count = decode_u8_count(
        payload,
        offset,
        mmorpg_core::MAX_PRACTICE,
        "too much banked practice",
    )?;
    let mut practice = Vec::with_capacity(count);
    for _ in 0..count {
        practice.push(mmorpg_core::Practice {
            creature: mmorpg_core::CreatureId::new(u32::from_be_bytes(take(payload, offset)?)),
            life: u64::from_be_bytes(take(payload, offset)?),
            points: read_u8(payload, offset)?,
        });
    }
    if !mmorpg_core::valid_practice(&practice) {
        return Err(ProtocolError::new("banked practice is invalid"));
    }
    Ok(practice)
}

pub(crate) fn decode_caches(
    payload: &[u8],
    offset: &mut usize,
) -> Result<std::collections::BTreeSet<u32>, ProtocolError> {
    let count = decode_u16_count(
        payload,
        offset,
        usize::from(u16::MAX),
        "too many opened caches",
    )?;
    let mut caches = std::collections::BTreeSet::new();
    for _ in 0..count {
        let id = u32::from_be_bytes(take(payload, offset)?);
        if id == 0 || caches.last().is_some_and(|last| *last >= id) {
            return Err(ProtocolError::new(
                "opened caches are not ordered nonzero IDs",
            ));
        }
        caches.insert(id);
    }
    Ok(caches)
}

/// A quest ID, never `0`; whether content defines it is the
/// client's concern.
pub(crate) fn decode_quest(code: u32) -> Result<QuestId, ProtocolError> {
    let quest = QuestId::new(code);
    if code != 0 {
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

/// A timed swing's start (#679) in an event's `u16` amount: the aim's side code in bits 0–1,
/// its height code in bits 2–3, the startup ticks in bits 4–9 and the active ticks in 10–15.
pub(crate) fn encode_swing_began(aim: StrikeAim, startup: u8, active: u8) -> u16 {
    let six = mmorpg_core::MAX_SWING_PHASE_TICKS;
    u16::from(aim.side.code())
        | u16::from(aim.height.code()) << 2
        | (u16::from(startup) & six) << 4
        | (u16::from(active) & six) << 10
}

pub(crate) fn decode_swing_began(amount: u16) -> Result<(StrikeAim, u8, u8), ProtocolError> {
    #[allow(clippy::cast_possible_truncation)]
    let field = |shift: u16, mask: u16| ((amount >> shift) & mask) as u8;
    let aim = decode_aim([field(0, 0b11), field(2, 0b11)])?;
    let six = mmorpg_core::MAX_SWING_PHASE_TICKS;
    let (startup, active) = (field(4, six), field(10, six));
    if active == 0 {
        return Err(ProtocolError::new("timed swing has no active phase"));
    }
    Ok((aim, startup, active))
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

/// A catalog ability ID.
pub(crate) fn decode_ability(code: u32) -> Result<AbilityId, ProtocolError> {
    let ability = AbilityId::new(code);
    if ability_by_id(ability).is_some() {
        Ok(ability)
    } else {
        Err(ProtocolError::new("unknown ability"))
    }
}

record! {
    /// An event record ([`EventRecord::BYTES`], 17): kind, `u32` flags (or the ability ID of
    /// ability events, the quest ID of quest events or the trap ID of trap hits, #882), source
    /// reference, target reference, `u16` amount (a quest progress event packs the objective
    /// index high and the count low; a strike sweep packs the aim's side code high and its
    /// height code low). Guard events carry the lane's height code as flags, and the blocked
    /// amount or the break's recovery ticks.
    pub(crate) struct EventRecord {
        kind: u8,
        flags: u32,
        source: EntityRefRecord,
        target: EntityRefRecord,
        amount: u16,
    }
}

impl Codec for ZoneEvent {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        encode_event(payload, self);
        Ok(())
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        decode_event(payload, offset)
    }
}

fn encode_event(payload: &mut Vec<u8>, event: &ZoneEvent) {
    let (kind, flags, source, target, amount) = match *event {
        ZoneEvent::DamageDealt {
            source,
            target,
            amount,
            critical,
        } => (
            DAMAGE_DEALT,
            u32::from(critical),
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
            u32::from(critical),
            Some(source),
            Some(target),
            amount,
        ),
        ZoneEvent::AttackPerformed { source, target } => {
            (ATTACK_PERFORMED, 0, Some(source), target, 0)
        }
        ZoneEvent::DamageObserved {
            source,
            target,
            amount,
            critical,
        } => (
            DAMAGE_OBSERVED,
            u32::from(critical),
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
        ZoneEvent::SwingBegan {
            source,
            ability,
            aim,
            startup,
            active,
            ..
        } => (
            SWING_BEGAN,
            ability.get(),
            Some(source),
            None,
            encode_swing_began(aim, startup, active),
        ),
        // A basic strike (#885): the variant code and the family code in the flags' low bytes,
        // the contact countdown in the amount.
        ZoneEvent::StrikeBegan {
            source,
            family,
            variant,
            contact,
        } => (
            STRIKE_BEGAN,
            u32::from(variant.code()) | u32::from(family.code()) << 8,
            Some(source),
            None,
            contact,
        ),
        // A strike's timing reward (#928): the reward code in the flags, its ticks in the amount.
        ZoneEvent::StrikeRewarded {
            source,
            reward,
            ticks,
        } => (
            STRIKE_REWARDED,
            u32::from(reward.code()),
            Some(source),
            None,
            ticks,
        ),
        // A flanking Sinister Strike (#949).
        ZoneEvent::Flanked { source, target } => (FLANKED, 0, Some(source), Some(target), 0),
        // A beast's attack (#889): the attack code in the flags, the contact countdown in the
        // amount.
        ZoneEvent::CreatureAttackBegan {
            source,
            attack,
            contact,
        } => (
            CREATURE_ATTACK_BEGAN,
            u32::from(attack.code()),
            Some(source),
            None,
            contact,
        ),
        ZoneEvent::Clashed {
            source,
            target,
            kind,
        } => (
            CLASHED,
            u32::from(kind.code()),
            Some(source),
            Some(target),
            0,
        ),
        ZoneEvent::Blocked {
            source,
            target,
            lane,
            amount,
        } => (
            BLOCKED,
            u32::from(lane.code()),
            Some(source),
            Some(target),
            amount,
        ),
        ZoneEvent::GuardBroken {
            source,
            target,
            lane,
            ticks,
        } => (
            GUARD_BROKEN,
            u32::from(lane.code()),
            Some(source),
            Some(target),
            ticks,
        ),
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
        ZoneEvent::TrapHit {
            trap,
            target,
            amount,
        } => (TRAP_HIT, trap.get(), None, Some(target), amount),
        ZoneEvent::Raid { notice } => (
            RAID_NOTICE,
            u32::from(crate::raid::notice_code(notice)),
            None,
            None,
            0,
        ),
    };
    let target = if let ZoneEvent::SwingBegan { facing, .. } = event {
        // This event has no target: reserve that slot for its accepted heading, as an absent
        // reference's ID below 2^16. The event stays the same size in both snapshot scopes.
        EntityRefRecord {
            kind: NO_ENTITY,
            id: u32::from(*facing),
        }
    } else {
        EntityRefRecord::of(target)
    };
    EventRecord {
        kind,
        flags,
        source: EntityRefRecord::of(source),
        target,
        amount,
    }
    .put(payload);
}

fn decode_event(payload: &[u8], offset: &mut usize) -> Result<ZoneEvent, ProtocolError> {
    let EventRecord {
        kind,
        flags,
        source,
        target,
        amount,
    } = EventRecord::read(payload, offset)?;
    let code = || u8::try_from(flags).map_err(|_| ProtocolError::new("event flags are invalid"));
    let source = source.entity()?;
    let (target, swing_facing) = if kind == SWING_BEGAN {
        match (target.kind, u16::try_from(target.id)) {
            (NO_ENTITY, Ok(facing)) => (None, facing),
            _ => return Err(ProtocolError::new("malformed swing heading")),
        }
    } else {
        (target.entity()?, 0)
    };
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
        TRAP_HIT => {
            let target = target.ok_or_else(invalid)?;
            if source.is_some() {
                return Err(invalid());
            }
            ZoneEvent::TrapHit {
                trap: mmorpg_core::TrapId::new(flags),
                target,
                amount,
            }
        }
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
        // A whiffed swing (#517) names no target.
        ATTACK_PERFORMED => {
            damage(false)?;
            if amount != 0 {
                return Err(invalid());
            }
            ZoneEvent::AttackPerformed {
                source: source.ok_or_else(invalid)?,
                target,
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
        SWING_BEGAN => {
            if target.is_some() {
                return Err(invalid());
            }
            let (aim, startup, active) = decode_swing_began(amount)?;
            ZoneEvent::SwingBegan {
                source: source.ok_or_else(invalid)?,
                ability: decode_ability(flags)?,
                facing: swing_facing,
                aim,
                startup,
                active,
            }
        }
        STRIKE_BEGAN => {
            if target.is_some() || flags > 0xffff {
                return Err(invalid());
            }
            let [family, variant] = u16::try_from(flags).map_err(|_| invalid())?.to_be_bytes();
            ZoneEvent::StrikeBegan {
                source: source.ok_or_else(invalid)?,
                family: mmorpg_core::WeaponFamily::from_code(family).ok_or_else(invalid)?,
                variant: mmorpg_core::StrikeVariant::from_code(variant).ok_or_else(invalid)?,
                contact: amount,
            }
        }
        STRIKE_REWARDED => {
            if target.is_some() {
                return Err(invalid());
            }
            ZoneEvent::StrikeRewarded {
                source: source.ok_or_else(invalid)?,
                reward: mmorpg_core::StrikeReward::from_code(code()?).ok_or_else(invalid)?,
                ticks: amount,
            }
        }
        FLANKED => {
            damage(false)?;
            let (source, target) = pair()?;
            if amount != 0 {
                return Err(invalid());
            }
            ZoneEvent::Flanked { source, target }
        }
        CREATURE_ATTACK_BEGAN => {
            if target.is_some() || amount == 0 {
                return Err(invalid());
            }
            ZoneEvent::CreatureAttackBegan {
                source: source.ok_or_else(invalid)?,
                attack: mmorpg_core::CreatureAttack::from_code(code()?).ok_or_else(invalid)?,
                contact: amount,
            }
        }
        CLASHED => {
            let (source, target) = pair()?;
            if amount != 0 {
                return Err(invalid());
            }
            ZoneEvent::Clashed {
                source,
                target,
                kind: mmorpg_core::ClashKind::from_code(code()?)
                    .ok_or_else(|| ProtocolError::new("unknown clash kind"))?,
            }
        }
        STRIKE_SWEPT => ZoneEvent::StrikeSwept {
            source: source.ok_or_else(invalid)?,
            target,
            ability: decode_ability(flags)?,
            aim: decode_aim(amount.to_be_bytes())?,
        },
        BLOCKED | GUARD_BROKEN => {
            let lane = StrikeHeight::from_code(code()?)
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
        RAID_NOTICE => {
            if source.is_some() || target.is_some() || amount != 0 {
                return Err(invalid());
            }
            ZoneEvent::Raid {
                notice: crate::raid::decode_notice(code()?)?,
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

/// An ordered set of `u32` indices or IDs (#882): `count u16 · count × u32`, strictly
/// ascending.
pub(crate) fn encode_u32_set(
    payload: &mut Vec<u8>,
    set: &BTreeSet<u32>,
    message: &str,
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(set.len(), usize::from(u16::MAX), message)?.to_be_bytes(),
    );
    for value in set {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    Ok(())
}

pub(crate) fn decode_u32_set(
    payload: &[u8],
    offset: &mut usize,
    message: &str,
) -> Result<BTreeSet<u32>, ProtocolError> {
    let count = decode_u16_count(payload, offset, usize::from(u16::MAX), message)?;
    let mut set = BTreeSet::new();
    for _ in 0..count {
        let value = u32::from_be_bytes(take(payload, offset)?);
        if set.last().is_some_and(|last| *last >= value) {
            return Err(ProtocolError::new(message));
        }
        set.insert(value);
    }
    Ok(set)
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
    #[cfg(test)]
    crate::layout::trace::read(payload, *offset, N);
    *offset = end;
    Ok(array)
}

/// A door-state list: count (`u16`), then door ID (`u32`) and open flag per door, in ascending
/// ID order (#158, #882).
pub(crate) fn encode_doors(
    payload: &mut Vec<u8>,
    doors: &[mmorpg_core::DoorView],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(doors.len(), usize::from(u16::MAX), "too many doors")?.to_be_bytes(),
    );
    for door in doors {
        payload.extend_from_slice(&door.door.get().to_be_bytes());
        payload.push(u8::from(door.open));
    }
    Ok(())
}

pub(crate) fn decode_doors(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::DoorView>, ProtocolError> {
    let count = decode_u16_count(payload, offset, usize::from(u16::MAX), "too many doors")?;
    let mut doors: Vec<mmorpg_core::DoorView> = Vec::with_capacity(count);
    for _ in 0..count {
        let door = mmorpg_core::DoorId::new(u32::from_be_bytes(take(payload, offset)?));
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
pub(crate) fn encode_piece(piece: Option<mmorpg_core::PieceId>) -> u32 {
    piece.map_or(0, |piece| piece.get().saturating_add(1))
}

pub(crate) fn decode_piece(value: u32) -> Option<mmorpg_core::PieceId> {
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

/// The not-intact pieces of a projection (#305): count (`u16`), then per piece its ID (`u32`)
/// and stage (`u8`), in ascending ID order.
/// The shrine's seals and wards (#1019): `count u8 · count × (piece u32 · set mask u8 · open u8)`,
/// then `count u8 · count × (mechanism u32 · exposed-until tick u64)`, in content order.
pub(crate) fn encode_shrine_views(
    payload: &mut Vec<u8>,
    seals: &[mmorpg_core::SealView],
    wards: &[mmorpg_core::WardView],
) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        seals.len(),
        mmorpg_core::MAX_SEALS,
        "too many seals",
    )?);
    for seal in seals {
        payload.extend_from_slice(&seal.piece.get().to_be_bytes());
        payload.push(seal.set);
        payload.push(u8::from(seal.open));
    }
    payload.push(encode_u8_count(
        wards.len(),
        mmorpg_core::MAX_SEALS,
        "too many wards",
    )?);
    for ward in wards {
        payload.extend_from_slice(&ward.mechanism.get().to_be_bytes());
        payload.extend_from_slice(&ward.exposed_until.to_be_bytes());
    }
    Ok(())
}

pub(crate) fn decode_shrine_views(
    payload: &[u8],
    offset: &mut usize,
) -> Result<(Vec<mmorpg_core::SealView>, Vec<mmorpg_core::WardView>), ProtocolError> {
    let count = decode_u8_count(payload, offset, mmorpg_core::MAX_SEALS, "too many seals")?;
    let seals = (0..count)
        .map(|_| {
            Ok(mmorpg_core::SealView {
                piece: mmorpg_core::PieceId::new(u32::from_be_bytes(take(payload, offset)?)),
                set: read_u8(payload, offset)?,
                open: read_bool(payload, offset)?,
            })
        })
        .collect::<Result<_, ProtocolError>>()?;
    let count = decode_u8_count(payload, offset, mmorpg_core::MAX_SEALS, "too many wards")?;
    let wards = (0..count)
        .map(|_| {
            Ok(mmorpg_core::WardView {
                mechanism: mmorpg_core::MechanismId::new(u32::from_be_bytes(take(
                    payload, offset,
                )?)),
                exposed_until: u64::from_be_bytes(take(payload, offset)?),
            })
        })
        .collect::<Result<_, ProtocolError>>()?;
    Ok((seals, wards))
}

pub(crate) fn encode_piece_views(
    payload: &mut Vec<u8>,
    pieces: &[mmorpg_core::PieceView],
) -> Result<(), ProtocolError> {
    payload.extend_from_slice(
        &encode_u16_count(
            pieces.len(),
            usize::from(u16::MAX),
            "too many structure pieces",
        )?
        .to_be_bytes(),
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
    let count = decode_u16_count(
        payload,
        offset,
        usize::from(u16::MAX),
        "too many structure pieces",
    )?;
    let mut pieces: Vec<mmorpg_core::PieceView> = Vec::with_capacity(count);
    for _ in 0..count {
        let piece = mmorpg_core::PieceId::new(u32::from_be_bytes(take(payload, offset)?));
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

record! {
    /// A fired trap (#300): `u32` ID and `u16` age in ticks.
    pub(crate) struct TrapRecord {
        id: u32,
        age: u16,
    }
}

impl Codec for mmorpg_core::TrapView {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        TrapRecord {
            id: self.id.get(),
            age: self.age,
        }
        .encode(payload)
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let TrapRecord { id, age } = TrapRecord::read(payload, offset)?;
        Ok(Self {
            id: mmorpg_core::TrapId::new(id),
            age,
        })
    }
}

/// Fired pressure traps near the viewer (#300): count (`u8`), then a [`TrapRecord`] per trap,
/// nearest first.
pub(crate) fn encode_trap_views(
    payload: &mut Vec<u8>,
    views: &[mmorpg_core::TrapView],
) -> Result<(), ProtocolError> {
    payload.push(
        u8::try_from(views.len())
            .ok()
            .filter(|&count| usize::from(count) <= mmorpg_core::MAX_VISIBLE_TRAPS)
            .ok_or_else(|| ProtocolError::new("too many trap views"))?,
    );
    for view in views {
        view.encode(payload)?;
    }
    Ok(())
}

pub(crate) fn decode_trap_views(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::TrapView>, ProtocolError> {
    let count = usize::from(read_u8(payload, offset)?);
    if count > mmorpg_core::MAX_VISIBLE_TRAPS {
        return Err(ProtocolError::new("too many trap views"));
    }
    (0..count)
        .map(|_| mmorpg_core::TrapView::decode(payload, offset))
        .collect()
}

record! {
    /// A summoned obstacle (#293): `u32` ID, kind, `3 × i32` centre, `u16` facing, health
    /// percent.
    pub(crate) struct SummonedRecord {
        id: u32,
        kind: u8,
        centre: [i32; 3],
        facing: u16,
        health_percent: u8,
    }
}

impl Codec for mmorpg_core::SummonedView {
    fn encode(&self, payload: &mut Vec<u8>) -> Result<(), ProtocolError> {
        SummonedRecord {
            id: self.id,
            kind: self.kind.code(),
            centre: self.centre,
            facing: self.facing,
            health_percent: self.health_percent,
        }
        .encode(payload)
    }

    fn decode(payload: &[u8], offset: &mut usize) -> Result<Self, ProtocolError> {
        let record = SummonedRecord::read(payload, offset)?;
        Ok(Self {
            id: record.id,
            kind: mmorpg_core::SummonedTerrainKind::from_code(record.kind)
                .ok_or_else(|| ProtocolError::new("unknown summoned terrain kind"))?,
            centre: record.centre,
            facing: record.facing,
            health_percent: record.health_percent,
        })
    }
}

/// Summoned terrain near the viewer (#293): count (`u8`), then a [`SummonedRecord`] per
/// obstacle, nearest first.
pub(crate) fn encode_summoned_views(
    payload: &mut Vec<u8>,
    views: &[mmorpg_core::SummonedView],
) -> Result<(), ProtocolError> {
    payload.push(
        u8::try_from(views.len())
            .ok()
            .filter(|&count| usize::from(count) <= mmorpg_core::MAX_VISIBLE_SUMMONED)
            .ok_or_else(|| ProtocolError::new("too much summoned terrain"))?,
    );
    for view in views {
        view.encode(payload)?;
    }
    Ok(())
}

pub(crate) fn decode_summoned_views(
    payload: &[u8],
    offset: &mut usize,
) -> Result<Vec<mmorpg_core::SummonedView>, ProtocolError> {
    let count = usize::from(read_u8(payload, offset)?);
    if count > mmorpg_core::MAX_VISIBLE_SUMMONED {
        return Err(ProtocolError::new("too much summoned terrain"));
    }
    (0..count)
        .map(|_| mmorpg_core::SummonedView::decode(payload, offset))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::generate::{CASES, Gen, round_trip};

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
                [payload[0], payload[4]],
                [
                    if matches!(event, ZoneEvent::Blocked { .. }) {
                        BLOCKED
                    } else {
                        GUARD_BROKEN
                    },
                    event_lane(event)
                ]
            );
            assert_eq!(&payload[1..4], [0, 0, 0]);
            assert_eq!(decode_event(&payload, &mut 0).unwrap(), event);
            payload[4] = 3;
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
            ErrorCode::NotWhileSwimming,
            ErrorCode::HandoffCorridor,
            ErrorCode::InCombat,
            ErrorCode::InvalidWaystone,
            ErrorCode::PrivateHome,
            ErrorCode::NoAnswer,
            ErrorCode::NotAtNight,
            ErrorCode::DepthsLocked,
            ErrorCode::DepthsClosed,
            ErrorCode::DepthsWipe,
            ErrorCode::DepthsBossOpen,
            ErrorCode::NoRoom,
            ErrorCode::NoLanding,
            ErrorCode::MovementImmune,
            ErrorCode::SeatTaken,
            ErrorCode::CannotSitHere,
        ];
        for (wire, code) in (1..).zip(codes) {
            assert_eq!(error_code(code), wire, "{code:?}");
            assert_eq!(decode_error_code(wire).unwrap(), code);
        }
        assert_eq!(error_code(ErrorCode::WeaponDrawn), 50);
        assert_eq!(error_code(ErrorCode::MissingMaterials), 51);
        assert_eq!(error_code(ErrorCode::WrongForm), 52);
        assert_eq!(decode_error_code(52).unwrap(), ErrorCode::WrongForm);
        assert_eq!(error_code(ErrorCode::RewardsBusy), 53);
        assert_eq!(decode_error_code(53).unwrap(), ErrorCode::RewardsBusy);
        for (wire, code) in [
            (54, ErrorCode::Distrusted),
            (55, ErrorCode::NpcBusy),
            (56, ErrorCode::EscortLost),
            (57, ErrorCode::NotSellable),
            (58, ErrorCode::HandsFull),
            (59, ErrorCode::NotCarrying),
            (60, ErrorCode::NoFooting),
            (61, ErrorCode::LoadLost),
            (62, ErrorCode::SiteNotReady),
            (63, ErrorCode::SealOpen),
            (64, ErrorCode::Warded),
            (65, ErrorCode::FlockUnsettled),
            (66, ErrorCode::FlockFell),
            (67, ErrorCode::StrayBolted),
            (68, ErrorCode::OutOfOrder),
            (69, ErrorCode::StaffShown),
        ] {
            assert_eq!(error_code(code), wire);
            assert_eq!(decode_error_code(wire).unwrap(), code);
        }
        for unknown in [0, 70, u16::MAX] {
            assert_eq!(
                decode_error_code(unknown).unwrap_err().to_string(),
                "unknown error code"
            );
        }
    }

    #[test]
    fn swing_heading_uses_the_reserved_target_slot_without_an_entity_reference() {
        let event = ZoneEvent::SwingBegan {
            source: EntityRef::Player(1),
            ability: AbilityId::new(1),
            facing: 16384,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
            startup: 18,
            active: 9,
        };
        let mut bytes = Vec::new();
        encode_event(&mut bytes, &event);
        assert_eq!(bytes.len(), crate::EVENT_RECORD_BYTES);
        assert_eq!(&bytes[10..15], &[0, 0, 0, 0x40, 0]);
        assert_eq!(decode_event(&bytes, &mut 0).unwrap(), event);
        bytes[10] = PLAYER_KIND;
        assert!(decode_event(&bytes, &mut 0).is_err());
    }

    fn entity_ref(rng: &mut Gen) -> Option<EntityRef> {
        let kind = rng.pick(&[
            None,
            Some(EntityKind::Player),
            Some(EntityKind::Creature),
            Some(EntityKind::Npc),
            Some(EntityKind::Minion),
        ])?;
        Some(EntityRef::new(kind, rng.int()))
    }

    fn some_ref(rng: &mut Gen) -> EntityRef {
        loop {
            if let Some(entity) = entity_ref(rng) {
                return entity;
            }
        }
    }

    #[test]
    fn entity_references_round_trip() {
        let mut rng = Gen::new(6);
        for _ in 0..CASES {
            round_trip(&entity_ref(&mut rng), Some(ENTITY_REF_BYTES));
        }
        let mut offset = 0;
        assert_eq!(
            Option::<EntityRef>::decode(&[0, 0, 0, 0, 1], &mut offset)
                .unwrap_err()
                .to_string(),
            "an absent entity must have ID 0"
        );
    }

    #[test]
    fn events_round_trip() {
        let mut rng = Gen::new(7);
        let lanes = [StrikeHeight::Low, StrikeHeight::Middle, StrikeHeight::High];
        for _ in 0..CASES {
            let (source, target) = (some_ref(&mut rng), some_ref(&mut rng));
            let event = match rng.below(9) {
                0 => ZoneEvent::DamageDealt {
                    source,
                    target,
                    amount: rng.int(),
                    critical: rng.bool(),
                },
                1 => ZoneEvent::DamageTaken {
                    source,
                    target,
                    amount: rng.int(),
                    critical: rng.bool(),
                },
                2 => ZoneEvent::DamageObserved {
                    source,
                    target,
                    amount: rng.int(),
                    critical: rng.bool(),
                },
                3 => ZoneEvent::Miss { source, target },
                4 => ZoneEvent::Evade { source, target },
                5 => ZoneEvent::Died {
                    entity: target,
                    killer: entity_ref(&mut rng),
                },
                6 => ZoneEvent::AttackPerformed {
                    source,
                    target: entity_ref(&mut rng),
                },
                7 => ZoneEvent::TrapHit {
                    trap: mmorpg_core::TrapId::new(rng.int()),
                    target,
                    amount: rng.int(),
                },
                _ => ZoneEvent::Blocked {
                    source,
                    target,
                    lane: rng.pick(&lanes),
                    amount: rng.int(),
                },
            };
            round_trip(&event, Some(EventRecord::BYTES));
        }
    }

    #[test]
    fn traps_and_summoned_terrain_round_trip() {
        let kinds = (0..=u8::MAX)
            .filter_map(mmorpg_core::SummonedTerrainKind::from_code)
            .collect::<Vec<_>>();
        let mut rng = Gen::new(8);
        for _ in 0..CASES {
            let trap = mmorpg_core::TrapView {
                id: mmorpg_core::TrapId::new(rng.int()),
                age: rng.int(),
            };
            round_trip(&trap, Some(TrapRecord::BYTES));
            let summoned = mmorpg_core::SummonedView {
                id: rng.int(),
                kind: rng.pick(&kinds),
                centre: [rng.int(), rng.int(), rng.int()],
                facing: rng.int(),
                health_percent: rng.int(),
            };
            round_trip(&summoned, Some(SummonedRecord::BYTES));
        }
    }
}
