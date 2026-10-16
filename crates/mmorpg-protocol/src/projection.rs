//! Player-visible scope: one player's projection, packed into a single
//! datagram by fixed section priority (header, self with its class,
//! resource, cast, cooldowns and auras, target details with the target's
//! cast and auras, sheets, events, then entities by relevance until the
//! budget is used).

use mmorpg_core::{
    AbilityId, AuraKind, AuraView, CastView, ClassChoice, Cooldown, EntityFlags, EntityKind,
    EntitySnapshot, GLOBAL_COOLDOWN_TICKS, MAX_AURAS, MAX_CHAT_PER_TICK, MAX_COOLDOWNS,
    MAX_EVENTS_PER_PLAYER, MAX_VISIBLE_ENTITIES, ResourceKind, ResourceView, TargetDetail,
    ViewerState, ZoneSnapshot, ability_by_id,
};

use crate::inventory::{
    EQUIPMENT_RECORD_BYTES, INVENTORY_RECORD_BYTES, decode_equipment, decode_inventory,
    encode_equipment, encode_inventory,
};
use crate::wire::{
    COMMON_HEADER_BYTES, ENTITY_REF_BYTES, PLAYER_SNAPSHOT_SCOPE, decode_common_header,
    decode_entity_kind, decode_entity_ref, decode_event, decode_u8_count, decode_u16_count,
    encode_common_header, encode_entity_ref, encode_event, encode_u8_count, ensure_fully_consumed,
    entity_kind_code, flag_byte, read_bool, read_flags, read_u8, take,
};
use crate::{MAX_PLAYER_PROJECTION_BYTES, ProtocolError};

/// Content revision, acknowledged sequence and viewer ID after the prefix.
const HEADER_BYTES: usize = COMMON_HEADER_BYTES + 8 + 4 + 4;
/// Health, maximum health, level, flags and the viewer's target.
const SELF_BYTES: usize = 4 + 4 + 1 + 8 + 4 + 1 + ENTITY_REF_BYTES;
/// Ability ID, flags (bit 0 channel), elapsed and total ticks; ability 0
/// means no cast.
pub const CAST_RECORD_BYTES: usize = 1 + 1 + 2 + 2;
/// Class code, resource kind/value/max, global cooldown, cast, melee damage
/// range, then the cooldown and aura counts.
const SELF_ABILITY_BYTES: usize = 1 + 1 + 2 + 2 + 2 + CAST_RECORD_BYTES + 2 + 2 + 1 + 1;
/// Four `u16` stat totals: stamina, strength, agility and intellect.
const STAT_TOTALS_BYTES: usize = 4 * 2;
/// The self sheet's fixed part: bag, equipment and stat totals; the quest
/// part follows (at most [`crate::quest::MAX_QUEST_SHEET_BYTES`]).
pub const SELF_SHEET_BYTES: usize =
    INVENTORY_RECORD_BYTES + EQUIPMENT_RECORD_BYTES + STAT_TOTALS_BYTES;
const MAX_SELF_SHEET_BYTES: usize = SELF_SHEET_BYTES + crate::quest::MAX_QUEST_SHEET_BYTES;
/// The target's own target, the target's cast and its aura count.
const TARGET_BYTES: usize = ENTITY_REF_BYTES + CAST_RECORD_BYTES + 1;
/// Every section's fixed part: header, self, target, inventory revision
/// and presence, loot presence, event count (`u8`), chat count (`u8`) and
/// entity count (`u16`).
pub const PLAYER_SNAPSHOT_FIXED_BYTES: usize =
    HEADER_BYTES + SELF_BYTES + SELF_ABILITY_BYTES + TARGET_BYTES + 8 + 1 + 1 + 1 + 1 + 2;
/// Ability ID and remaining ticks.
pub const COOLDOWN_RECORD_BYTES: usize = 1 + 2;
/// Ability ID, aura kind, remaining ticks and amount.
pub const AURA_RECORD_BYTES: usize = 1 + 1 + 2 + 2;
/// The largest cooldown and aura lists of the viewer and its target.
const MAX_ABILITY_LIST_BYTES: usize =
    MAX_COOLDOWNS * COOLDOWN_RECORD_BYTES + 2 * MAX_AURAS * AURA_RECORD_BYTES;
/// Kind, flags, source and target references, `u16` amount.
pub const EVENT_RECORD_BYTES: usize = 2 + 2 * ENTITY_REF_BYTES + 2;
/// Kind, ID, appearance, `3 × i16` position, `3 × i8` velocity, `u16`
/// facing, level, health percent and flags.
pub const ENTITY_RECORD_BYTES: usize = 1 + 4 + 2 + 6 + 3 + 2 + 1 + 1 + 1;
/// Records that fit the budget without events, sheets, cooldowns or auras:
/// (1,077 − 105) / 21 = 46.
pub const MAX_WIRE_ENTITIES: usize =
    (MAX_PLAYER_PROJECTION_BYTES - PLAYER_SNAPSHOT_FIXED_BYTES) / ENTITY_RECORD_BYTES;
const MAX_EVENT_SECTION_BYTES: usize = MAX_EVENTS_PER_PLAYER * EVENT_RECORD_BYTES;
const MAX_CHAT_SECTION_BYTES: usize = MAX_CHAT_PER_TICK * crate::chat::MAX_CHAT_RECORD_BYTES;

// With every list full, both sheets (a full quest log and eight markers),
// a full event section and four 80-byte chat lines, the viewer and its
// target always fit:
// 105 + 108 + 26 + 154 + 16 × 14 + 4 × 86 + 2 × 21 = 1,003 ≤ 1,077 bytes.
const _: () = assert!(
    PLAYER_SNAPSHOT_FIXED_BYTES
        + MAX_ABILITY_LIST_BYTES
        + crate::loot::MAX_LOOT_SHEET_BYTES
        + MAX_SELF_SHEET_BYTES
        + MAX_EVENT_SECTION_BYTES
        + MAX_CHAT_SECTION_BYTES
        + 2 * ENTITY_RECORD_BYTES
        <= MAX_PLAYER_PROJECTION_BYTES
);
// Budget packing, not the relevance cap, bounds the entity count.
const _: () = assert!(MAX_WIRE_ENTITIES <= MAX_VISIBLE_ENTITIES);

/// A player projection packed within [`MAX_PLAYER_PROJECTION_BYTES`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedSnapshot {
    pub payload: Vec<u8>,
    /// Leading entities of the projection that fit; the rest were omitted.
    pub packed_entities: usize,
}

/// Budget-driven packing: writes the header, the viewer's own state, its
/// target's target and this tick's events, then entities in the projection's
/// priority order until the next record would exceed
/// [`MAX_PLAYER_PROJECTION_BYTES`]. The viewer and its target lead the
/// entities and always fit. Hosts publish this; positions outside the `i16`
/// range fail closed rather than being clamped.
pub fn pack_snapshot(snapshot: &ZoneSnapshot) -> Result<PackedSnapshot, ProtocolError> {
    if snapshot
        .entities
        .first()
        .is_none_or(|entity| (entity.kind, entity.id) != (EntityKind::Player, snapshot.viewer_id))
    {
        return Err(ProtocolError::new(
            "player projection must start with the viewer",
        ));
    }
    let event_count = encode_u8_count(
        snapshot.events.len(),
        MAX_EVENTS_PER_PLAYER,
        "player projection has too many events",
    )?;
    let mut payload = Vec::with_capacity(MAX_PLAYER_PROJECTION_BYTES);
    encode_common_header(
        &mut payload,
        PLAYER_SNAPSHOT_SCOPE,
        snapshot.schema_version,
        snapshot.zone_id,
        snapshot.tick,
    )?;
    payload.extend_from_slice(&snapshot.content_revision.to_be_bytes());
    payload.extend_from_slice(&snapshot.acknowledged_sequence.to_be_bytes());
    payload.extend_from_slice(&snapshot.viewer_id.to_be_bytes());

    let viewer = &snapshot.viewer;
    payload.extend_from_slice(&viewer.health.to_be_bytes());
    payload.extend_from_slice(&viewer.max_health.to_be_bytes());
    payload.push(viewer.level);
    payload.extend_from_slice(&viewer.experience.to_be_bytes());
    payload.extend_from_slice(&viewer.experience_to_next_level.to_be_bytes());
    payload.extend_from_slice(&viewer.copper.to_be_bytes());
    payload.push(flag_byte(&[
        viewer.dead,
        viewer.in_combat,
        viewer.auto_attacking,
    ]));
    encode_entity_ref(&mut payload, viewer.target);
    payload.push(
        viewer
            .class
            .map_or(0, |choice| u8::try_from(choice.appearance()).unwrap_or(0)),
    );
    match viewer.resource {
        Some(resource) => {
            payload.push(resource_code(resource.kind));
            payload.extend_from_slice(&resource.value.to_be_bytes());
            payload.extend_from_slice(&resource.max.to_be_bytes());
        }
        None => payload.extend_from_slice(&[0; 5]),
    }
    payload.extend_from_slice(&viewer.global_cooldown.to_be_bytes());
    encode_cast(&mut payload, viewer.cast);
    for value in viewer.damage {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.push(encode_u8_count(
        snapshot.cooldowns.len(),
        MAX_COOLDOWNS,
        "player projection has too many cooldowns",
    )?);
    for cooldown in &snapshot.cooldowns {
        payload.push(cooldown.ability.get());
        payload.extend_from_slice(&cooldown.remaining.to_be_bytes());
    }
    encode_auras(&mut payload, &snapshot.auras)?;
    encode_entity_ref(&mut payload, snapshot.target_of_target);
    encode_cast(&mut payload, snapshot.target_detail.cast);
    encode_auras(&mut payload, &snapshot.target_detail.auras)?;

    payload.extend_from_slice(&snapshot.inventory_revision.to_be_bytes());
    match (&snapshot.inventory, &snapshot.equipment, &snapshot.quests) {
        (Some(inventory), Some(equipment), Some(quests)) => {
            payload.push(1);
            encode_inventory(&mut payload, inventory);
            encode_equipment(&mut payload, equipment);
            let totals = equipment.totals();
            for total in [
                totals.stamina,
                totals.strength,
                totals.agility,
                totals.intellect,
            ] {
                payload.extend_from_slice(&total.to_be_bytes());
            }
            crate::quest::encode_sheet(&mut payload, quests)?;
        }
        (None, None, None) => payload.push(0),
        _ => {
            return Err(ProtocolError::new(
                "the self sheet needs the bag, the equipment and the quests",
            ));
        }
    }
    payload.push(u8::from(snapshot.loot.is_some()));
    if let Some(view) = snapshot.loot {
        crate::loot::encode_claim(&mut payload, view.claim);
        crate::loot::encode_rewards(&mut payload, view.rewards);
    }
    payload.push(event_count);
    for event in &snapshot.events {
        encode_event(&mut payload, event);
    }
    payload.push(encode_u8_count(
        snapshot.chat.len(),
        MAX_CHAT_PER_TICK,
        "player projection has too many chat lines",
    )?);
    for line in &snapshot.chat {
        crate::chat::encode_line(&mut payload, line);
    }

    let count_offset = payload.len();
    payload.extend_from_slice(&0_u16.to_be_bytes());
    let mut packed_entities = 0_u16;
    for entity in snapshot.entities.iter().take(MAX_WIRE_ENTITIES) {
        if payload.len() + ENTITY_RECORD_BYTES > MAX_PLAYER_PROJECTION_BYTES {
            break;
        }
        encode_entity(&mut payload, entity)?;
        packed_entities += 1;
    }
    payload[count_offset..count_offset + 2].copy_from_slice(&packed_entities.to_be_bytes());
    Ok(PackedSnapshot {
        payload,
        packed_entities: usize::from(packed_entities),
    })
}

/// Encodes a complete player projection. Fails closed, never truncating
/// silently, when any entity would not fit [`MAX_PLAYER_PROJECTION_BYTES`].
/// Fixtures and tests use it; hosts publish with [`pack_snapshot`].
pub fn encode_snapshot(snapshot: &ZoneSnapshot) -> Result<Vec<u8>, ProtocolError> {
    let packed = pack_snapshot(snapshot)?;
    if packed.packed_entities < snapshot.entities.len() {
        return Err(ProtocolError::new(
            "player projection exceeds the byte budget",
        ));
    }
    Ok(packed.payload)
}

const fn resource_code(kind: ResourceKind) -> u8 {
    match kind {
        ResourceKind::Rage => 1,
        ResourceKind::Focus => 2,
        ResourceKind::Mana => 3,
    }
}

fn encode_cast(payload: &mut Vec<u8>, cast: Option<CastView>) {
    match cast {
        Some(cast) => {
            payload.extend_from_slice(&[cast.ability.get(), u8::from(cast.channel)]);
            payload.extend_from_slice(&cast.elapsed.to_be_bytes());
            payload.extend_from_slice(&cast.total.to_be_bytes());
        }
        None => payload.extend_from_slice(&[0; CAST_RECORD_BYTES]),
    }
}

/// A cast must name a casting catalog ability with its exact cast time and
/// channel flag, and be in progress.
fn decode_cast(payload: &[u8], offset: &mut usize) -> Result<Option<CastView>, ProtocolError> {
    let ability = read_u8(payload, offset)?;
    let [channel] = read_flags(read_u8(payload, offset)?)?;
    let elapsed = u16::from_be_bytes(take(payload, offset)?);
    let total = u16::from_be_bytes(take(payload, offset)?);
    if ability == 0 {
        if channel || elapsed != 0 || total != 0 {
            return Err(ProtocolError::new("cast state is inconsistent"));
        }
        return Ok(None);
    }
    let ability = AbilityId::new(ability);
    let valid = ability_by_id(ability).is_some_and(|catalog| {
        catalog.cast.ticks() == total && catalog.cast.is_channel() == channel && elapsed < total
    });
    if !valid {
        return Err(ProtocolError::new("cast state is inconsistent"));
    }
    Ok(Some(CastView {
        ability,
        elapsed,
        total,
        channel,
    }))
}

fn encode_auras(payload: &mut Vec<u8>, auras: &[AuraView]) -> Result<(), ProtocolError> {
    payload.push(encode_u8_count(
        auras.len(),
        MAX_AURAS,
        "player projection has too many auras",
    )?);
    for aura in auras {
        payload.extend_from_slice(&[aura.ability.get(), aura.kind.code()]);
        payload.extend_from_slice(&aura.remaining.to_be_bytes());
        payload.extend_from_slice(&aura.amount.to_be_bytes());
    }
    Ok(())
}

/// Auras name catalog abilities with their aura kind and time left.
fn decode_auras(payload: &[u8], offset: &mut usize) -> Result<Vec<AuraView>, ProtocolError> {
    let count = decode_u8_count(payload, offset, MAX_AURAS, "snapshot has too many auras")?;
    let mut auras = Vec::with_capacity(count);
    for _ in 0..count {
        let ability = AbilityId::new(read_u8(payload, offset)?);
        let kind = AuraKind::from_code(read_u8(payload, offset)?);
        let remaining = u16::from_be_bytes(take(payload, offset)?);
        let amount = u16::from_be_bytes(take(payload, offset)?);
        let spec = ability_by_id(ability).and_then(|catalog| catalog.aura());
        match (spec, kind) {
            (Some(spec), Some(kind))
                if spec.kind == kind && remaining > 0 && remaining <= spec.duration =>
            {
                auras.push(AuraView {
                    ability,
                    kind,
                    remaining,
                    amount,
                });
            }
            _ => return Err(ProtocolError::new("aura record is inconsistent")),
        }
    }
    Ok(auras)
}

fn encode_entity(payload: &mut Vec<u8>, entity: &EntitySnapshot) -> Result<(), ProtocolError> {
    payload.push(entity_kind_code(entity.kind));
    payload.extend_from_slice(&entity.id.to_be_bytes());
    payload.extend_from_slice(&entity.appearance.to_be_bytes());
    for component in entity.position {
        let component = i16::try_from(component)
            .map_err(|_| ProtocolError::new("entity position is outside the compact wire range"))?;
        payload.extend_from_slice(&component.to_be_bytes());
    }
    for component in entity.velocity {
        payload.extend_from_slice(&component.to_be_bytes());
    }
    payload.extend_from_slice(&entity.facing.to_be_bytes());
    payload.push(entity.level);
    if entity.health_percent > 100 {
        return Err(ProtocolError::new("health percent exceeds 100"));
    }
    payload.push(entity.health_percent);
    let flags = entity.flags;
    payload.push(flag_byte(&[
        flags.dead,
        flags.in_combat,
        flags.hostile,
        flags.attackable,
        flags.tapped_by_other,
        flags.evading,
        flags.targets_viewer,
        flags.lootable,
    ]));
    Ok(())
}

pub fn decode_snapshot(payload: &[u8]) -> Result<ZoneSnapshot, ProtocolError> {
    if payload.len() > MAX_PLAYER_PROJECTION_BYTES {
        return Err(ProtocolError::new(
            "snapshot exceeds the player projection byte budget",
        ));
    }
    let mut offset = 0;
    let (schema_version, zone_id, tick) =
        decode_common_header(payload, &mut offset, PLAYER_SNAPSHOT_SCOPE)?;
    let content_revision = u64::from_be_bytes(take(payload, &mut offset)?);
    let acknowledged_sequence = u32::from_be_bytes(take(payload, &mut offset)?);
    let viewer_id = u32::from_be_bytes(take(payload, &mut offset)?);

    let health = u32::from_be_bytes(take(payload, &mut offset)?);
    let max_health = u32::from_be_bytes(take(payload, &mut offset)?);
    let level = read_u8(payload, &mut offset)?;
    let experience = u32::from_be_bytes(take(payload, &mut offset)?);
    let experience_to_next_level = u32::from_be_bytes(take(payload, &mut offset)?);
    let copper = u32::from_be_bytes(take(payload, &mut offset)?);
    let [dead, in_combat, auto_attacking] = read_flags(read_u8(payload, &mut offset)?)?;
    let target = decode_entity_ref(payload, &mut offset)?;
    if !(1..=10).contains(&level)
        || (level == 10 && (experience != 0 || experience_to_next_level != 0))
        || (level < 10 && (experience_to_next_level == 0 || experience >= experience_to_next_level))
        || health > max_health
        || dead != (health == 0)
    {
        return Err(ProtocolError::new("viewer state is inconsistent"));
    }
    let class_code = read_u8(payload, &mut offset)?;
    let class = ClassChoice::from_appearance(u16::from(class_code));
    if class_code != 0 && class.is_none() {
        return Err(ProtocolError::new("viewer ability state is inconsistent"));
    }
    let resource_kind = read_u8(payload, &mut offset)?;
    let resource_value = u16::from_be_bytes(take(payload, &mut offset)?);
    let resource_max = u16::from_be_bytes(take(payload, &mut offset)?);
    let global_cooldown = u16::from_be_bytes(take(payload, &mut offset)?);
    let cast = decode_cast(payload, &mut offset)?;
    let damage = [
        u16::from_be_bytes(take(payload, &mut offset)?),
        u16::from_be_bytes(take(payload, &mut offset)?),
    ];
    if damage[0] > damage[1] {
        return Err(ProtocolError::new("viewer damage range is inverted"));
    }
    let cooldown_count = decode_u8_count(
        payload,
        &mut offset,
        MAX_COOLDOWNS,
        "snapshot has too many cooldowns",
    )?;
    let mut cooldowns = Vec::with_capacity(cooldown_count);
    for _ in 0..cooldown_count {
        let ability = AbilityId::new(read_u8(payload, &mut offset)?);
        let remaining = u16::from_be_bytes(take(payload, &mut offset)?);
        let ordered = cooldowns
            .last()
            .is_none_or(|last: &Cooldown| last.ability < ability);
        let valid = ability_by_id(ability)
            .is_some_and(|catalog| remaining > 0 && remaining <= catalog.cooldown);
        if !ordered || !valid {
            return Err(ProtocolError::new("cooldown record is inconsistent"));
        }
        cooldowns.push(Cooldown { ability, remaining });
    }
    let auras = decode_auras(payload, &mut offset)?;
    let resource = match (class, resource_kind) {
        (None, 0) if resource_value == 0 && resource_max == 0 => None,
        (Some(choice), code) if code == resource_code(choice.class.resource()) => {
            let kind = choice.class.resource();
            if resource_max != kind.max(level) || resource_value > resource_max {
                return Err(ProtocolError::new("viewer resource is inconsistent"));
            }
            Some(ResourceView {
                kind,
                value: resource_value,
                max: resource_max,
            })
        }
        _ => return Err(ProtocolError::new("viewer resource is inconsistent")),
    };
    if global_cooldown > GLOBAL_COOLDOWN_TICKS
        || (class.is_none()
            && (global_cooldown != 0
                || cast.is_some()
                || !cooldowns.is_empty()
                || !auras.is_empty()))
        || (dead && (cast.is_some() || !auras.is_empty()))
    {
        return Err(ProtocolError::new("viewer ability state is inconsistent"));
    }
    let target_of_target = decode_entity_ref(payload, &mut offset)?;
    let target_detail = TargetDetail {
        cast: decode_cast(payload, &mut offset)?,
        auras: decode_auras(payload, &mut offset)?,
    };
    if target.is_none() && target_detail != TargetDetail::default() {
        return Err(ProtocolError::new("target detail needs a target"));
    }

    let inventory_revision = u64::from_be_bytes(take(payload, &mut offset)?);
    if inventory_revision == 0 {
        return Err(ProtocolError::new("inventory revision must be nonzero"));
    }
    let (inventory, equipment, quests) = if read_bool(payload, &mut offset)? {
        let inventory = decode_inventory(payload, &mut offset)?;
        let equipment = decode_equipment(payload, &mut offset)?;
        let totals = equipment.totals();
        let mut wire = [0_u16; 4];
        for total in &mut wire {
            *total = u16::from_be_bytes(take(payload, &mut offset)?);
        }
        if wire
            != [
                totals.stamina,
                totals.strength,
                totals.agility,
                totals.intellect,
            ]
        {
            return Err(ProtocolError::new("stat totals do not match the equipment"));
        }
        let quests = crate::quest::decode_sheet(payload, &mut offset)?;
        (Some(inventory), Some(equipment), Some(quests))
    } else {
        (None, None, None)
    };
    let loot = if read_bool(payload, &mut offset)? {
        Some(mmorpg_core::LootView {
            claim: crate::loot::decode_claim(payload, &mut offset)?,
            rewards: crate::loot::decode_rewards(payload, &mut offset)?,
        })
    } else {
        None
    };
    let event_count = decode_u8_count(
        payload,
        &mut offset,
        MAX_EVENTS_PER_PLAYER,
        "snapshot exceeds the per-player event capacity",
    )?;
    let mut events = Vec::with_capacity(event_count);
    for _ in 0..event_count {
        events.push(decode_event(payload, &mut offset)?);
    }
    let chat_count = decode_u8_count(
        payload,
        &mut offset,
        MAX_CHAT_PER_TICK,
        "snapshot exceeds the per-player chat capacity",
    )?;
    let mut chat = Vec::with_capacity(chat_count);
    for _ in 0..chat_count {
        chat.push(crate::chat::decode_line(payload, &mut offset)?);
    }

    let count = decode_u16_count(
        payload,
        &mut offset,
        MAX_WIRE_ENTITIES,
        "snapshot exceeds configured visible entity capacity",
    )?;
    let mut entities = Vec::with_capacity(count);
    for _ in 0..count {
        entities.push(decode_entity(payload, &mut offset)?);
    }
    ensure_fully_consumed(payload, offset)?;
    if entities
        .first()
        .is_none_or(|entity| (entity.kind, entity.id) != (EntityKind::Player, viewer_id))
    {
        return Err(ProtocolError::new(
            "player projection must start with the viewer",
        ));
    }

    if let Some(view) = loot {
        let corpse = entities.iter().find(|entity| {
            entity.entity() == mmorpg_core::EntityRef::Creature(view.claim.creature)
        });
        if dead
            || target != Some(mmorpg_core::EntityRef::Creature(view.claim.creature))
            || view.claim.died_at > tick
            || (view.rewards.money == 0
                && view.rewards.item.is_none()
                && view.rewards.quest_item.is_none())
            // A quest drop is always one unit.
            || view
                .rewards
                .quest_item
                .is_some_and(|stack| stack.quantity() != 1)
            || corpse.is_none_or(|entity| {
                !entity.flags.dead
                    || !entity.flags.lootable
                    || entity.flags.tapped_by_other
                    || entity.health_percent != 0
            })
        {
            return Err(ProtocolError::new("corpse loot sheet is inconsistent"));
        }
    }
    Ok(ZoneSnapshot {
        content_revision,
        acknowledged_sequence,
        viewer_id,
        schema_version,
        zone_id,
        tick,
        viewer: ViewerState {
            experience,
            experience_to_next_level,
            copper,
            health,
            max_health,
            level,
            dead,
            in_combat,
            auto_attacking,
            target,
            class,
            resource,
            cast,
            global_cooldown,
            damage,
        },
        cooldowns,
        auras,
        target_of_target,
        target_detail,
        inventory_revision,
        inventory,
        equipment,
        loot,
        events,
        chat,
        entities,
        quests,
    })
}

fn decode_entity(payload: &[u8], offset: &mut usize) -> Result<EntitySnapshot, ProtocolError> {
    let kind = decode_entity_kind(read_u8(payload, offset)?)?;
    let id = u32::from_be_bytes(take(payload, offset)?);
    let appearance = u16::from_be_bytes(take(payload, offset)?);
    let position = [
        i16::from_be_bytes(take(payload, offset)?),
        i16::from_be_bytes(take(payload, offset)?),
        i16::from_be_bytes(take(payload, offset)?),
    ]
    .map(i32::from);
    let velocity = [
        i8::from_be_bytes(take(payload, offset)?),
        i8::from_be_bytes(take(payload, offset)?),
        i8::from_be_bytes(take(payload, offset)?),
    ];
    let facing = u16::from_be_bytes(take(payload, offset)?);
    let level = read_u8(payload, offset)?;
    let health_percent = read_u8(payload, offset)?;
    if health_percent > 100 {
        return Err(ProtocolError::new("health percent exceeds 100"));
    }
    let [
        dead,
        in_combat,
        hostile,
        attackable,
        tapped_by_other,
        evading,
        targets_viewer,
        lootable,
    ] = read_flags(read_u8(payload, offset)?)?;
    if lootable
        && (kind != EntityKind::Creature
            || !dead
            || health_percent != 0
            || attackable
            || tapped_by_other)
    {
        return Err(ProtocolError::new("lootable entity flags are inconsistent"));
    }
    Ok(EntitySnapshot {
        kind,
        id,
        appearance,
        position,
        velocity,
        facing,
        level,
        health_percent,
        flags: EntityFlags {
            dead,
            in_combat,
            hostile,
            attackable,
            tapped_by_other,
            evading,
            targets_viewer,
            lootable,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DATAGRAM_SAFETY_MARGIN_BYTES, MEASURED_MIN_DATAGRAM_BYTES, SESSION_SNAPSHOT_HEADER_BYTES,
    };
    use mmorpg_core::{
        CreatureId, EntityRef, ErrorCode, NpcId, SNAPSHOT_SCHEMA_VERSION, ZoneEvent, ZoneId,
    };

    const VIEWER: EntityRef = EntityRef::Player(7);
    const WOLF: EntityRef = EntityRef::Creature(CreatureId::new(108));
    const FIREBOLT: AbilityId = AbilityId::new(9);
    const FROST_NOVA: AbilityId = AbilityId::new(10);
    const ARCANE_BARRIER: AbilityId = AbilityId::new(11);
    const MUCK_BOLT: AbilityId = AbilityId::new(13);

    fn player(id: u32, position: [i32; 3], velocity: [i8; 3], facing: u16) -> EntitySnapshot {
        EntitySnapshot {
            kind: EntityKind::Player,
            id,
            appearance: 0,
            position,
            velocity,
            facing,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        }
    }

    /// The shared Rust/browser golden projection: a level-4 Arcanist casting
    /// Firebolt behind an Arcane Barrier at a rooted Mirefin Lurker that
    /// casts Muck Bolt, next to an NPC and a corpse tapped by another player,
    /// wearing a wand, hood and tunic (3 stamina, 6 intellect), with one event
    /// of every kind.
    fn fixture_snapshot() -> ZoneSnapshot {
        let mut slots = [None; mmorpg_core::INVENTORY_SLOTS];
        slots[0] = Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(1), 3).unwrap());
        slots[1] = Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(2), 1).unwrap());
        slots[15] = Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(1), 20).unwrap());
        ZoneSnapshot {
            quests: Some(quest_sheet()),
            loot: None,
            content_revision: 4,
            acknowledged_sequence: 81,
            viewer_id: 7,
            inventory_revision: 9,
            inventory: Some(mmorpg_core::Inventory::from_slots(slots)),
            equipment: Some(equipment(&[4, 6, 7])),
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(42),
            tick: 99,
            viewer: ViewerState {
                copper: 42,
                experience: 37,
                experience_to_next_level: 400,
                health: 38,
                // 95 at level 4 plus 5 per stamina.
                max_health: 110,
                level: 4,
                dead: false,
                in_combat: true,
                auto_attacking: true,
                target: Some(WOLF),
                class: Some(ClassChoice {
                    class: mmorpg_core::PlayerClass::Arcanist,
                    sex: mmorpg_core::Sex::Female,
                }),
                resource: Some(ResourceView {
                    kind: ResourceKind::Mana,
                    value: 121,
                    max: 176,
                }),
                cast: Some(CastView {
                    ability: FIREBOLT,
                    elapsed: 20,
                    total: 60,
                    channel: false,
                }),
                global_cooldown: 25,
                // Level 4's 6–9 plus half of 6 intellect.
                damage: [9, 12],
            },
            cooldowns: vec![
                Cooldown {
                    ability: FROST_NOVA,
                    remaining: 412,
                },
                Cooldown {
                    ability: ARCANE_BARRIER,
                    remaining: 700,
                },
            ],
            auras: vec![AuraView {
                ability: ARCANE_BARRIER,
                kind: AuraKind::Absorb,
                remaining: 500,
                amount: 31,
            }],
            target_of_target: Some(VIEWER),
            target_detail: TargetDetail {
                cast: Some(CastView {
                    ability: MUCK_BOLT,
                    elapsed: 10,
                    total: 45,
                    channel: false,
                }),
                auras: vec![AuraView {
                    ability: FROST_NOVA,
                    kind: AuraKind::Root,
                    remaining: 150,
                    amount: 0,
                }],
            },
            events: vec![
                ZoneEvent::DamageDealt {
                    source: VIEWER,
                    target: WOLF,
                    amount: 7,
                    critical: true,
                },
                ZoneEvent::DamageTaken {
                    source: WOLF,
                    target: VIEWER,
                    amount: 3,
                    critical: false,
                },
                ZoneEvent::Miss {
                    source: VIEWER,
                    target: WOLF,
                },
                ZoneEvent::Evade {
                    source: VIEWER,
                    target: WOLF,
                },
                ZoneEvent::Died {
                    entity: WOLF,
                    killer: Some(VIEWER),
                },
                ZoneEvent::Error {
                    code: ErrorCode::OutOfRange,
                    target: Some(WOLF),
                },
                ZoneEvent::Error {
                    code: ErrorCode::NotEnoughResource,
                    target: None,
                },
                ZoneEvent::CastStarted {
                    source: WOLF,
                    target: Some(VIEWER),
                    ability: MUCK_BOLT,
                    ticks: 45,
                },
                ZoneEvent::AbilityUsed {
                    source: VIEWER,
                    target: None,
                    ability: FROST_NOVA,
                },
                ZoneEvent::Healed {
                    source: VIEWER,
                    target: VIEWER,
                    ability: AbilityId::new(3),
                    amount: 2,
                },
                ZoneEvent::AuraApplied {
                    source: VIEWER,
                    target: WOLF,
                    ability: FROST_NOVA,
                    ticks: 180,
                },
                ZoneEvent::AuraRemoved {
                    source: VIEWER,
                    target: VIEWER,
                    ability: ARCANE_BARRIER,
                },
                ZoneEvent::Interrupted {
                    source: None,
                    target: VIEWER,
                    ability: FIREBOLT,
                },
                ZoneEvent::Absorbed {
                    source: WOLF,
                    target: VIEWER,
                    amount: 5,
                },
                ZoneEvent::QuestProgress {
                    quest: mmorpg_core::QuestId::new(2),
                    objective: 0,
                    count: 3,
                },
                ZoneEvent::QuestCompleted {
                    quest: mmorpg_core::QuestId::new(1),
                },
            ],
            chat: vec![
                mmorpg_core::ChatLine {
                    speaker: 7,
                    message: mmorpg_core::ChatMessage::Say(
                        mmorpg_core::ChatText::new("Hail, Greyhaven!").unwrap(),
                    ),
                },
                mmorpg_core::ChatLine {
                    speaker: 4_000_000_000,
                    message: mmorpg_core::ChatMessage::Yell(
                        mmorpg_core::ChatText::new("Grüße!").unwrap(),
                    ),
                },
                mmorpg_core::ChatLine {
                    speaker: 9,
                    message: mmorpg_core::ChatMessage::Emote(mmorpg_core::Emote::Cheer),
                },
            ],
            entities: vec![
                EntitySnapshot {
                    appearance: 5,
                    health_percent: 40,
                    flags: EntityFlags {
                        in_combat: true,
                        ..EntityFlags::default()
                    },
                    ..player(7, [10, 90, -30], [1, -2, 3], 16_384)
                },
                EntitySnapshot {
                    kind: EntityKind::Creature,
                    id: 108,
                    appearance: 5,
                    position: [-845, 45, 2_500],
                    velocity: [-13, 0, i8::MIN],
                    facing: 49_152,
                    level: 2,
                    health_percent: 43,
                    flags: EntityFlags {
                        lootable: false,
                        dead: false,
                        in_combat: true,
                        hostile: true,
                        attackable: true,
                        tapped_by_other: false,
                        evading: false,
                        targets_viewer: true,
                    },
                },
                EntitySnapshot {
                    kind: EntityKind::Npc,
                    id: 5,
                    appearance: 5,
                    position: [-1_650, 90, 4_550],
                    velocity: [0; 3],
                    facing: 49_152,
                    level: 10,
                    health_percent: 100,
                    flags: EntityFlags::default(),
                },
                EntitySnapshot {
                    kind: EntityKind::Creature,
                    id: 109,
                    appearance: 1,
                    position: [-900, 45, 2_600],
                    velocity: [0; 3],
                    facing: 0,
                    level: 1,
                    health_percent: 0,
                    flags: EntityFlags {
                        dead: true,
                        hostile: true,
                        tapped_by_other: true,
                        ..EntityFlags::default()
                    },
                },
            ],
        }
    }

    /// Quest 1 turned in, quest 2 at 3 of its first objective, `!` over
    /// NPC 1 and grey `?` over NPC 2: 4 + 1 + 4 + 1 + 2 × 3 bytes.
    fn quest_sheet() -> mmorpg_core::QuestSheet {
        mmorpg_core::QuestSheet {
            completed: 0b1,
            entries: vec![mmorpg_core::QuestEntry {
                quest: mmorpg_core::QuestId::new(2),
                progress: [3, 0, 0],
            }],
            markers: vec![
                mmorpg_core::NpcMarker {
                    npc: NpcId::new(1),
                    marker: mmorpg_core::QuestMarker::Available,
                },
                mmorpg_core::NpcMarker {
                    npc: NpcId::new(2),
                    marker: mmorpg_core::QuestMarker::InProgress,
                },
            ],
        }
    }

    /// Ten active quests with full progress bytes and eight markers.
    fn full_quest_sheet() -> mmorpg_core::QuestSheet {
        mmorpg_core::QuestSheet {
            completed: 0xffff_fc00,
            entries: (1..=10)
                .map(|quest| mmorpg_core::QuestEntry {
                    quest: mmorpg_core::QuestId::new(quest),
                    progress: [u8::MAX; 3],
                })
                .collect(),
            markers: (1..=8)
                .map(|npc| mmorpg_core::NpcMarker {
                    npc: NpcId::new(npc * 1_000),
                    marker: mmorpg_core::QuestMarker::Complete,
                })
                .collect(),
        }
    }

    /// Equipment holding each catalog item in its own slot.
    fn equipment(items: &[u16]) -> mmorpg_core::Equipment {
        let mut slots = [None; mmorpg_core::EQUIPMENT_SLOTS];
        for &item in items {
            let id = mmorpg_core::ItemId::new(item);
            let slot = mmorpg_core::item_template(id).unwrap().slot.unwrap();
            slots[usize::from(slot.index())] = Some(id);
        }
        mmorpg_core::Equipment::from_slots(slots).unwrap()
    }

    fn hex(fixture: &str) -> Vec<u8> {
        let fixture = fixture.trim();
        (0..fixture.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&fixture[offset..offset + 2], 16).unwrap())
            .collect()
    }

    fn fixture_bytes() -> Vec<u8> {
        hex(include_str!(
            "../../../fixtures/protocol/player-snapshot-v14.hex"
        ))
    }

    /// Byte offsets of the fixture's sections.
    const CLASS: usize = 59;
    const CAST: usize = CLASS + 8;
    const DAMAGE: usize = CAST + CAST_RECORD_BYTES;
    const COOLDOWNS: usize = DAMAGE + 4;
    const AURAS: usize = COOLDOWNS + 1 + 2 * COOLDOWN_RECORD_BYTES;
    const TARGET_OF_TARGET: usize = AURAS + 1 + AURA_RECORD_BYTES;
    const TARGET_CAST: usize = TARGET_OF_TARGET + ENTITY_REF_BYTES;
    const TARGET_AURAS: usize = TARGET_CAST + CAST_RECORD_BYTES;
    const INVENTORY: usize = TARGET_AURAS + 1 + AURA_RECORD_BYTES;
    const EVENT_COUNT: usize = 16;
    const EQUIPMENT: usize = INVENTORY + 9 + INVENTORY_RECORD_BYTES;
    const STATS: usize = EQUIPMENT + EQUIPMENT_RECORD_BYTES;
    const QUESTS: usize = STATS + 8;
    /// The fixture's quest sheet.
    const QUEST_BYTES: usize = 4 + 1 + 4 + 1 + 2 * 3;
    const EVENTS: usize = INVENTORY + 8 + 1 + SELF_SHEET_BYTES + QUEST_BYTES + 1;
    /// The fixture's chat records: 6 + 16 and 6 + 8 bytes, then a 6-byte emote.
    const CHAT_BYTES: usize = 22 + 14 + 6;
    const CHAT: usize = EVENTS + 1 + EVENT_COUNT * EVENT_RECORD_BYTES;
    const ENTITY_COUNT: usize = CHAT + 1 + CHAT_BYTES;
    const FIRST_ENTITY: usize = ENTITY_COUNT + 2;
    /// Cooldown and aura records of the fixture.
    const LIST_BYTES: usize = 2 * COOLDOWN_RECORD_BYTES + 2 * AURA_RECORD_BYTES;

    #[test]
    fn player_snapshot_matches_the_shared_golden_fixture() {
        let snapshot = fixture_snapshot();
        let encoded = encode_snapshot(&snapshot).unwrap();
        if std::env::var_os("MMORPG_PRINT_FIXTURE").is_some() {
            println!(
                "{}",
                encoded
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
        }
        assert_eq!(
            encoded.len(),
            PLAYER_SNAPSHOT_FIXED_BYTES
                + LIST_BYTES
                + SELF_SHEET_BYTES
                + QUEST_BYTES
                + EVENT_COUNT * EVENT_RECORD_BYTES
                + CHAT_BYTES
                + 4 * ENTITY_RECORD_BYTES
        );
        assert_eq!(encoded, fixture_bytes());
        assert_eq!(
            encoded[CHAT], 3,
            "two chat lines and an emote follow the events"
        );
        assert_eq!(decode_snapshot(&encoded).unwrap(), snapshot);
        assert_eq!(encoded[CLASS], 5, "Arcanist, female");
        assert_eq!(encoded[EVENTS], 16);
        assert_eq!(
            encoded[QUESTS..EVENTS - 1],
            [0, 0, 0, 1, 1, 2, 3, 0, 0, 2, 0, 1, 1, 0, 2, 2],
            "the quest sheet ends the self sheet"
        );
        assert_eq!(encoded[FIRST_ENTITY], 1, "the viewer's record leads");
        // The previous version's fixture is rejected, never reinterpreted.
        let legacy = hex(include_str!(
            "../../../fixtures/protocol/player-snapshot-v13.hex"
        ));
        assert_eq!(
            decode_snapshot(&legacy).unwrap_err().to_string(),
            "unsupported snapshot wire version"
        );
    }

    #[test]
    fn complete_corpse_sheet_matches_shared_bytes_and_rejects_inconsistent_claims() {
        let mut snapshot = fixture_snapshot();
        snapshot.viewer.auto_attacking = false;
        snapshot.target_of_target = None;
        let corpse = &mut snapshot.entities[1];
        corpse.position = [10, 45, -180];
        corpse.velocity = [0; 3];
        corpse.health_percent = 0;
        corpse.flags = EntityFlags {
            dead: true,
            hostile: true,
            lootable: true,
            ..EntityFlags::default()
        };
        snapshot.loot = Some(mmorpg_core::LootView {
            claim: mmorpg_core::LootClaim {
                creature: CreatureId::new(108),
                died_at: 98,
            },
            rewards: mmorpg_core::LootRewards {
                money: 2,
                item: Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(1), 2).unwrap()),
                quest_item: Some(
                    mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(10), 1).unwrap(),
                ),
            },
        });
        // A dead target has no target detail.
        snapshot.target_detail = TargetDetail::default();
        let expected = hex(include_str!(
            "../../../fixtures/protocol/player-loot-v14.hex"
        ));
        if std::env::var_os("MMORPG_PRINT_FIXTURE").is_some() {
            let bytes = encode_snapshot(&snapshot).unwrap();
            println!(
                "loot {}",
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
        }
        let bytes = encode_snapshot(&snapshot).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(decode_snapshot(&bytes).unwrap(), snapshot);
        // Without target detail auras the sheet sits one aura record earlier.
        let presence = EVENTS - 1 - AURA_RECORD_BYTES;
        for length in 0..bytes.len() {
            assert!(decode_snapshot(&bytes[..length]).is_err());
        }
        for (offset, value) in [
            (presence, 2),
            (presence + 4, 109),
            (presence + 12, 100),
            (presence + 17, 2),
            (presence + 19, 9),
            (presence + 21, 0),
            (presence + 22, 2),
            (presence + 24, 11),
            (presence + 26, 0),
            // A quest drop of two units.
            (presence + 26, 2),
        ] {
            let mut invalid = bytes.clone();
            invalid[offset] = value;
            assert!(decode_snapshot(&invalid).is_err(), "offset {offset}");
        }
        for changed in 0..4 {
            let mut invalid = snapshot.clone();
            match changed {
                0 => {
                    invalid.loot.as_mut().unwrap().rewards = mmorpg_core::LootRewards {
                        money: 0,
                        item: None,
                        quest_item: None,
                    }
                }
                1 => invalid.loot.as_mut().unwrap().claim.died_at = snapshot.tick + 1,
                2 => invalid.viewer.target = None,
                3 => invalid.entities[1].flags.lootable = false,
                _ => unreachable!(),
            }
            assert!(decode_snapshot(&encode_snapshot(&invalid).unwrap()).is_err());
        }
        for legacy in [
            include_str!("../../../fixtures/protocol/player-snapshot-v7.hex"),
            include_str!("../../../fixtures/protocol/player-loot-v8.hex"),
            include_str!("../../../fixtures/protocol/player-loot-v9.hex"),
            include_str!("../../../fixtures/protocol/player-loot-v10.hex"),
            include_str!("../../../fixtures/protocol/player-loot-v11.hex"),
            include_str!("../../../fixtures/protocol/player-loot-v12.hex"),
            include_str!("../../../fixtures/protocol/player-loot-v13.hex"),
        ] {
            assert!(decode_snapshot(&hex(legacy)).is_err());
        }
    }

    #[test]
    fn player_snapshot_decoder_is_strict() {
        let encoded = fixture_bytes();
        for length in 0..encoded.len() {
            assert!(decode_snapshot(&encoded[..length]).is_err(), "{length}");
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert_eq!(
            decode_snapshot(&trailing).unwrap_err().to_string(),
            "snapshot payload contains trailing bytes"
        );
        let wolf_record = FIRST_ENTITY + ENTITY_RECORD_BYTES;
        for (offset, value, message) in [
            (0, 4, "unsupported snapshot wire version"),
            (1, 1, "unexpected snapshot scope"),
            (3, 4, "unsupported core snapshot schema version"),
            (53, 0b1000, "reserved flag bits are set"),
            (53, 0b111, "viewer state is inconsistent"),
            (54, 9, "unknown entity kind"),
            (CLASS, 7, "viewer ability state is inconsistent"),
            (CLASS, 1, "viewer resource is inconsistent"),
            (CLASS + 1, 1, "viewer resource is inconsistent"),
            (CLASS + 4, 0xff, "viewer resource is inconsistent"),
            (CLASS + 7, 46, "viewer ability state is inconsistent"),
            (CAST, 1, "cast state is inconsistent"),
            (CAST + 1, 1, "cast state is inconsistent"),
            (CAST + 1, 2, "reserved flag bits are set"),
            (CAST + 3, 60, "cast state is inconsistent"),
            (DAMAGE + 1, 13, "viewer damage range is inverted"),
            (COOLDOWNS, 5, "snapshot has too many cooldowns"),
            (COOLDOWNS + 1, 11, "cooldown record is inconsistent"),
            (COOLDOWNS + 2, 9, "cooldown record is inconsistent"),
            (AURAS, 9, "snapshot has too many auras"),
            (AURAS + 1, 9, "aura record is inconsistent"),
            (AURAS + 2, 4, "aura record is inconsistent"),
            (AURAS + 3, 9, "aura record is inconsistent"),
            (TARGET_OF_TARGET, 0, "an absent entity must have ID 0"),
            (TARGET_CAST, 0, "cast state is inconsistent"),
            (TARGET_AURAS + 4, 0, "aura record is inconsistent"),
            (EVENTS, 17, "snapshot exceeds the per-player event capacity"),
            (EVENTS + 1, 16, "unknown event kind"),
            (EVENTS + 2, 2, "event flags are invalid"),
            (
                EVENTS + 1 + 2 * EVENT_RECORD_BYTES + 1,
                1,
                "event flags are invalid",
            ),
            (
                EVENTS + 1 + 6 * EVENT_RECORD_BYTES + 13,
                99,
                "unknown error code",
            ),
            (
                EVENTS + 1 + 7 * EVENT_RECORD_BYTES + 1,
                15,
                "unknown ability",
            ),
            (
                EVENTS + 1 + 7 * EVENT_RECORD_BYTES + 1,
                0,
                "unknown ability",
            ),
            (
                EVENTS + 1 + 7 * EVENT_RECORD_BYTES,
                16,
                "unknown event kind",
            ),
            (
                EVENTS + 1 + 8 * EVENT_RECORD_BYTES + 13,
                1,
                "malformed event record",
            ),
            (
                EVENTS + 1 + 13 * EVENT_RECORD_BYTES + 1,
                1,
                "event flags are invalid",
            ),
            (
                EVENTS + 1 + 14 * EVENT_RECORD_BYTES + 1,
                0,
                "quest id is out of range",
            ),
            (
                EVENTS + 1 + 14 * EVENT_RECORD_BYTES + 12,
                3,
                "malformed event record",
            ),
            (
                EVENTS + 1 + 14 * EVENT_RECORD_BYTES + 13,
                0,
                "malformed event record",
            ),
            (
                EVENTS + 1 + 15 * EVENT_RECORD_BYTES + 1,
                33,
                "quest id is out of range",
            ),
            (
                EVENTS + 1 + 15 * EVENT_RECORD_BYTES + 2,
                1,
                "malformed event record",
            ),
            (QUESTS + 4, 11, "quest log exceeds its capacity"),
            (QUESTS + 5, 1, "an active quest is also turned in"),
            (QUESTS + 9, 9, "quest sheet has too many markers"),
            (QUESTS + 12, 4, "unknown quest marker"),
            (QUESTS + 14, 1, "quest markers are not ordered"),
            (FIRST_ENTITY, 0, "unknown entity kind"),
            (FIRST_ENTITY, 4, "unknown entity kind"),
            (wolf_record + 19, 101, "health percent exceeds 100"),
            (
                wolf_record + 20,
                0x80,
                "lootable entity flags are inconsistent",
            ),
            (31, 9, "player projection must start with the viewer"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
        let mut swapped = encoded.clone();
        swapped[FIRST_ENTITY..].rotate_left(ENTITY_RECORD_BYTES);
        assert_eq!(
            decode_snapshot(&swapped).unwrap_err().to_string(),
            "player projection must start with the viewer"
        );
        let mut fewer = encoded.clone();
        fewer[ENTITY_COUNT..FIRST_ENTITY].copy_from_slice(&3_u16.to_be_bytes());
        assert!(decode_snapshot(&fewer).is_err(), "count must match records");
        let mut more = encoded;
        more[ENTITY_COUNT..FIRST_ENTITY].copy_from_slice(&5_u16.to_be_bytes());
        assert!(decode_snapshot(&more).is_err(), "count must match records");
    }

    #[test]
    fn progression_decoder_rejects_invalid_self_state_and_legacy_bytes() {
        let legacy = include_str!("../../../fixtures/protocol/player-snapshot-v5.hex");
        let bytes: Vec<_> = legacy
            .trim()
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        assert!(decode_snapshot(&bytes).is_err());
        let v6 = include_str!("../../../fixtures/protocol/player-snapshot-v6.hex");
        let bytes = v6
            .trim()
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        assert!(decode_snapshot(&bytes).is_err());
        for (level, experience, threshold) in [
            (1, 100, 100),
            (1, 0, 0),
            (10, 1, 0),
            (10, 0, 100),
            (11, 0, 0),
        ] {
            let mut snapshot = fixture_snapshot();
            snapshot.viewer.level = level;
            snapshot.viewer.experience = experience;
            snapshot.viewer.experience_to_next_level = threshold;
            assert!(decode_snapshot(&pack_snapshot(&snapshot).unwrap().payload).is_err());
        }
    }

    #[test]
    fn inventory_decoder_rejects_corrupt_slots_and_missing_revision() {
        let encoded = fixture_bytes();
        let mut zero_revision = encoded.clone();
        zero_revision[INVENTORY..INVENTORY + 8].fill(0);
        assert!(decode_snapshot(&zero_revision).is_err());
        let mut invalid_flag = encoded.clone();
        invalid_flag[INVENTORY + 8] = 2;
        assert!(decode_snapshot(&invalid_flag).is_err());
        for (slot, item, quantity) in [
            (0, 0_u16, 3_u16),
            (0, 11, 1),
            (0, 1, 0),
            (0, 1, 21),
            (1, 2, 2),
        ] {
            let mut invalid = encoded.clone();
            let offset = INVENTORY + 9 + slot * 4;
            invalid[offset..offset + 2].copy_from_slice(&item.to_be_bytes());
            invalid[offset + 2..offset + 4].copy_from_slice(&quantity.to_be_bytes());
            assert!(decode_snapshot(&invalid).is_err());
        }
        let mut omitted = fixture_snapshot();
        omitted.inventory = None;
        assert_eq!(
            encode_snapshot(&omitted).unwrap_err().to_string(),
            "the self sheet needs the bag, the equipment and the quests"
        );
        omitted.equipment = None;
        omitted.quests = None;
        assert_eq!(
            decode_snapshot(&encode_snapshot(&omitted).unwrap()).unwrap(),
            omitted
        );
    }

    #[test]
    fn equipment_decoder_rejects_unknown_misplaced_items_and_wrong_totals() {
        let encoded = fixture_bytes();
        assert_eq!(encoded[EQUIPMENT..EQUIPMENT + 2], [0, 4], "the wand");
        // Slot 0 holds the wand; slot 1 (off hand) is empty.
        for (offset, item, message) in [
            (EQUIPMENT, 11_u16, "unknown item"),
            (EQUIPMENT, 1, "item cannot be equipped there"),
            (EQUIPMENT + 2, 6, "item cannot be equipped there"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset..offset + 2].copy_from_slice(&item.to_be_bytes());
            assert_eq!(
                decode_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "offset {offset} = item {item}"
            );
        }
        // Removing the wand leaves the intellect total stale.
        let mut stale = encoded.clone();
        stale[EQUIPMENT..EQUIPMENT + 2].fill(0);
        assert_eq!(
            decode_snapshot(&stale).unwrap_err().to_string(),
            "stat totals do not match the equipment"
        );
        assert_eq!(encoded[STATS..STATS + 8], [0, 3, 0, 0, 0, 0, 0, 6]);
        for stat in 0..4 {
            let mut invalid = encoded.clone();
            invalid[STATS + 2 * stat + 1] ^= 1;
            assert_eq!(
                decode_snapshot(&invalid).unwrap_err().to_string(),
                "stat totals do not match the equipment",
                "stat {stat}"
            );
        }
    }

    #[test]
    fn snapshot_decoder_rejects_counts_and_sizes_above_the_budget_before_allocation() {
        let mut encoded = encode_snapshot(&ZoneSnapshot {
            events: Vec::new(),
            entities: vec![fixture_snapshot().entities[0].clone()],
            ..fixture_snapshot()
        })
        .unwrap();
        let excessive_count = u16::try_from(MAX_WIRE_ENTITIES + 1).unwrap();
        encoded[EVENTS + 1..EVENTS + 3].copy_from_slice(&excessive_count.to_be_bytes());
        assert_eq!(
            decode_snapshot(&encoded).unwrap_err().to_string(),
            "snapshot exceeds configured visible entity capacity"
        );
        let oversized = vec![0; MAX_PLAYER_PROJECTION_BYTES + 1];
        assert_eq!(
            decode_snapshot(&oversized).unwrap_err().to_string(),
            "snapshot exceeds the player projection byte budget"
        );
    }

    /// Every section at its extreme: 16 events and the whole relevance cap
    /// with extreme field values. Packing keeps the viewer, its target and as
    /// many nearest units as fit, within one datagram.
    #[test]
    fn the_largest_projection_packs_into_the_datagram_budget() {
        assert_eq!(MAX_PLAYER_PROJECTION_BYTES, 1_161 - 20 - 64);
        assert_eq!(
            (
                PLAYER_SNAPSHOT_FIXED_BYTES,
                EVENT_RECORD_BYTES,
                ENTITY_RECORD_BYTES
            ),
            (105, 14, 21)
        );
        assert_eq!(MAX_WIRE_ENTITIES, 46);
        let viewer_id = u32::MAX;
        let target = EntityRef::Creature(CreatureId::new(u32::MAX));
        let entities: Vec<_> = (0..MAX_VISIBLE_ENTITIES)
            .map(|index| {
                let (low, high) = if index % 2 == 0 {
                    (i16::MIN, i16::MAX)
                } else {
                    (i16::MAX, i16::MIN)
                };
                let (kind, id) = match index {
                    0 => (EntityKind::Player, viewer_id),
                    1 => (EntityKind::Creature, u32::MAX),
                    _ => (EntityKind::Npc, u32::try_from(index).unwrap()),
                };
                EntitySnapshot {
                    kind,
                    id,
                    appearance: u16::MAX,
                    position: [low, high, low].map(i32::from),
                    velocity: [i8::MIN, i8::MAX, i8::MIN],
                    facing: u16::MAX,
                    level: u8::MAX,
                    health_percent: 100,
                    flags: EntityFlags {
                        lootable: false,
                        dead: false,
                        in_combat: true,
                        hostile: true,
                        attackable: true,
                        tapped_by_other: true,
                        evading: true,
                        targets_viewer: true,
                    },
                }
            })
            .collect();
        let event = ZoneEvent::DamageTaken {
            source: target,
            target: EntityRef::Player(viewer_id),
            amount: u16::MAX,
            critical: true,
        };
        let snapshot = ZoneSnapshot {
            chat: Vec::new(),
            loot: None,
            content_revision: u64::MAX,
            acknowledged_sequence: u32::MAX,
            viewer_id,
            inventory_revision: 1,
            inventory: None,
            equipment: None,
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            zone_id: ZoneId::new(u32::MAX),
            tick: u64::MAX,
            viewer: ViewerState {
                copper: 0,
                health: u32::MAX,
                max_health: u32::MAX,
                level: 9,
                experience: 899,
                experience_to_next_level: 900,
                dead: false,
                in_combat: true,
                auto_attacking: true,
                target: Some(target),
                class: None,
                resource: None,
                cast: None,
                global_cooldown: 0,
                damage: [u16::MAX; 2],
            },
            cooldowns: Vec::new(),
            auras: Vec::new(),
            target_of_target: Some(EntityRef::Npc(NpcId::new(u32::MAX))),
            target_detail: TargetDetail::default(),
            events: vec![event; MAX_EVENTS_PER_PLAYER],
            entities,
            quests: None,
        };
        let packed = pack_snapshot(&snapshot).unwrap();
        // (1,077 − 105 − 16 × 14) / 21 = 35 records fit beside a full event section.
        assert_eq!(packed.packed_entities, 35);
        assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
        assert!(
            packed.payload.len() + SESSION_SNAPSHOT_HEADER_BYTES + DATAGRAM_SAFETY_MARGIN_BYTES
                <= MEASURED_MIN_DATAGRAM_BYTES
        );
        let decoded = decode_snapshot(&packed.payload).unwrap();
        assert_eq!(decoded.entities[..], snapshot.entities[..35]);
        assert_eq!(
            decoded.entities[1].entity(),
            target,
            "the target always fits"
        );
        assert_eq!(decoded.events, snapshot.events);
        assert_eq!(
            encode_snapshot(&snapshot).unwrap_err().to_string(),
            "player projection exceeds the byte budget"
        );
        // Without events or a bag sheet the budget holds 46 records.
        let quiet = ZoneSnapshot {
            events: Vec::new(),
            ..snapshot
        };
        let packed = pack_snapshot(&quiet).unwrap();
        assert_eq!(packed.packed_entities, MAX_WIRE_ENTITIES);
        assert_eq!(
            packed.payload.len(),
            PLAYER_SNAPSHOT_FIXED_BYTES + MAX_WIRE_ENTITIES * ENTITY_RECORD_BYTES
        );
        let full_bag = mmorpg_core::Inventory::from_slots(
            [Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(1), 20).unwrap()); 16],
        );
        let full_kit = equipment(&[3, 5, 6, 7, 8, 9]);
        let mut maximum_bytes = 0;
        for events in 0..=MAX_EVENTS_PER_PLAYER {
            let mut with_sheet = quiet.clone();
            with_sheet.inventory_revision = u64::MAX;
            with_sheet.inventory = Some(full_bag.clone());
            with_sheet.equipment = Some(full_kit);
            with_sheet.quests = Some(full_quest_sheet());
            with_sheet.events = vec![event; events];
            let packed = pack_snapshot(&with_sheet).unwrap();
            maximum_bytes = maximum_bytes.max(packed.payload.len());
            let decoded = decode_snapshot(&packed.payload).unwrap();
            assert_eq!(decoded.inventory, with_sheet.inventory);
            assert_eq!(decoded.equipment, with_sheet.equipment);
            assert_eq!(decoded.quests, with_sheet.quests);
            assert_eq!(decoded.inventory_revision, u64::MAX);
            assert_eq!(decoded.events, with_sheet.events);
            assert_eq!(decoded.entities[1].entity(), target);
            assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
            // With a full quest part the sheet is 84 + 70 = 154 bytes:
            // (1,077 − 105 − 154) / 21 = 38 and (1,077 − 105 − 154 − 224) / 21 = 28.
            if events == 0 {
                assert_eq!(packed.packed_entities, 38);
            }
            if events == 16 {
                assert_eq!(packed.packed_entities, 28);
            }
        }
        // Whole records only: 105 + 154 + 16 × 14 + 28 × 21 = 1,071 is the largest.
        assert_eq!(maximum_bytes, 1_071);
        // Four full chat lines still leave room for the viewer, its target and more.
        let mut chatty = quiet.clone();
        chatty.events = vec![event; MAX_EVENTS_PER_PLAYER];
        chatty.chat = vec![
            mmorpg_core::ChatLine {
                speaker: u32::MAX,
                message: mmorpg_core::ChatMessage::Yell(
                    mmorpg_core::ChatText::new(&"€".repeat(26)).unwrap(),
                ),
            };
            MAX_CHAT_PER_TICK
        ];
        let packed = pack_snapshot(&chatty).unwrap();
        // (1,077 − 105 − 224 − 4 × 84) / 21 = 19 records.
        assert_eq!(packed.packed_entities, 19);
        assert_eq!(decode_snapshot(&packed.payload).unwrap().chat, chatty.chat);
        for item in [
            None,
            Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(1), 20).unwrap()),
        ] {
            for events in 0..=MAX_EVENTS_PER_PLAYER {
                let mut with_loot = quiet.clone();
                with_loot.inventory = Some(full_bag.clone());
                with_loot.equipment = Some(full_kit);
                with_loot.quests = Some(full_quest_sheet());
                with_loot.viewer.copper = u32::MAX;
                with_loot.loot = Some(mmorpg_core::LootView {
                    claim: mmorpg_core::LootClaim {
                        creature: CreatureId::new(u32::MAX),
                        died_at: u64::MAX,
                    },
                    rewards: mmorpg_core::LootRewards {
                        money: u32::MAX,
                        item,
                        // A quest drop is one unit; its record is as large as any stack's.
                        quest_item: item.map(|_| {
                            mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(10), 1).unwrap()
                        }),
                    },
                });
                with_loot.target_of_target = None;
                with_loot.entities[1].health_percent = 0;
                with_loot.entities[1].flags = EntityFlags {
                    dead: true,
                    lootable: true,
                    ..EntityFlags::default()
                };
                with_loot.events = vec![event; events];
                let packed = pack_snapshot(&with_loot).unwrap();
                maximum_bytes = maximum_bytes.max(packed.payload.len());
                assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
                let decoded = decode_snapshot(&packed.payload).unwrap();
                assert_eq!(decoded.loot, with_loot.loot);
                assert_eq!(decoded.viewer.copper, u32::MAX);
                assert_eq!(decoded.inventory, with_loot.inventory);
                assert_eq!(decoded.entities[1].entity(), target);
                // (1,077 − 105 − 154 − 26 − 224) / 21 = 27.
                if events == 16 && item.is_some() {
                    assert_eq!(packed.packed_entities, 27);
                }
            }
        }
        // With a corpse sheet, whole records reach 1,076 bytes.
        assert_eq!(maximum_bytes, 1_076);
        // Full cooldown and aura lists for the viewer and its target, both
        // sheets and every event still leave the viewer and its target.
        let aura = |ability: u8| {
            let ability = AbilityId::new(ability);
            let spec = ability_by_id(ability).unwrap().aura().unwrap();
            AuraView {
                ability,
                kind: spec.kind,
                remaining: spec.duration,
                amount: u16::MAX,
            }
        };
        let full_auras: Vec<_> = [2, 3, 6, 7, 8, 10, 11, 3].into_iter().map(aura).collect();
        let mut loaded = quiet.clone();
        loaded.inventory = Some(full_bag);
        loaded.equipment = Some(full_kit);
        loaded.quests = Some(full_quest_sheet());
        loaded.events = vec![event; MAX_EVENTS_PER_PLAYER];
        loaded.viewer.class = Some(ClassChoice {
            class: mmorpg_core::PlayerClass::Warden,
            sex: mmorpg_core::Sex::Male,
        });
        loaded.viewer.resource = Some(ResourceView {
            kind: ResourceKind::Rage,
            value: 100,
            max: 100,
        });
        loaded.viewer.global_cooldown = GLOBAL_COOLDOWN_TICKS;
        loaded.viewer.cast = Some(CastView {
            ability: AbilityId::new(12),
            elapsed: 179,
            total: 180,
            channel: true,
        });
        loaded.cooldowns = [2, 3, 4, 5]
            .into_iter()
            .map(|ability| Cooldown {
                ability: AbilityId::new(ability),
                remaining: ability_by_id(AbilityId::new(ability)).unwrap().cooldown,
            })
            .collect();
        loaded.auras = full_auras.clone();
        loaded.target_detail = TargetDetail {
            cast: Some(CastView {
                ability: AbilityId::new(14),
                elapsed: 89,
                total: 90,
                channel: false,
            }),
            auras: full_auras,
        };
        let packed = pack_snapshot(&loaded).unwrap();
        // (1,077 − 105 − 108 − 154 − 16 × 14) / 21 = 23 records.
        assert_eq!(packed.packed_entities, 23);
        assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
        let decoded = decode_snapshot(&packed.payload).unwrap();
        assert_eq!(decoded.entities[1].entity(), target);
        assert_eq!(
            (decoded.cooldowns, decoded.auras, decoded.target_detail),
            (loaded.cooldowns, loaded.auras, loaded.target_detail)
        );
    }

    #[test]
    fn snapshot_encoding_fails_closed_on_missing_viewer_wide_positions_or_excess_events() {
        let mut no_viewer = fixture_snapshot();
        no_viewer.entities.clear();
        let mut viewer_second = fixture_snapshot();
        viewer_second.entities.swap(0, 1);
        for invalid in [no_viewer, viewer_second] {
            assert_eq!(
                encode_snapshot(&invalid).unwrap_err().to_string(),
                "player projection must start with the viewer"
            );
        }
        for component in [i32::from(i16::MAX) + 1, i32::from(i16::MIN) - 1] {
            let mut wide = fixture_snapshot();
            wide.entities[1].position[2] = component;
            assert_eq!(
                encode_snapshot(&wide).unwrap_err().to_string(),
                "entity position is outside the compact wire range"
            );
        }
        let mut chatty = fixture_snapshot();
        chatty.events = vec![chatty.events[0]; MAX_EVENTS_PER_PLAYER + 1];
        assert_eq!(
            encode_snapshot(&chatty).unwrap_err().to_string(),
            "player projection has too many events"
        );
    }
}
