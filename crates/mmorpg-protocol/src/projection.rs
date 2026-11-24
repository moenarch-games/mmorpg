//! Player-visible scope: one player's projection, packed into a single
//! datagram by fixed section priority (header, self with its class,
//! resource and cast, the target's target and cast, the sheets the client
//! lacks (#544; doors, pieces, the raid signal and waystones travel in sheets too, #882), the
//! nearby summoned terrain and traps, events, chat, then entities by relevance until the budget
//! is used).

use std::collections::BTreeSet;

use mmorpg_core::{
    CastView, ClassChoice, EntityKind, EntitySnapshot, GLOBAL_COOLDOWN_TICKS, GuardView,
    MAX_CHAT_PER_TICK, MAX_EVENTS_PER_PLAYER, MAX_VISIBLE_ENTITIES, PlayerProjection, ResourceKind,
    ResourceView, TargetDetail, ViewerState, ZoneEvent, ZoneSnapshot,
};

use crate::codec::Codec;
use crate::records::{CastRecord, DebrisRecord, EntityRecord, LoadRecord, ProjectileRecord};
use crate::sheet::{
    Abilities, PublishStamps, SHEET_COUNT, Sheet, SheetEncodingCache, SheetRevisions,
    encode_sheets, revision,
};
use crate::wire::{
    COMMON_HEADER_BYTES, ENTITY_REF_BYTES, PLAYER_SNAPSHOT_SCOPE, decode_common_header,
    decode_entity_ref, decode_u8_count, decode_u16_count, encode_common_header, encode_entity_ref,
    encode_u8_count, ensure_fully_consumed, flag_byte, read_bool, read_flags, read_u8, take,
};
use crate::wire::{EventRecord, SummonedRecord, TrapRecord};
use crate::{MAX_PLAYER_PROJECTION_BYTES, ProtocolError};

/// Content revision, acknowledged sequence and viewer ID after the prefix.
const HEADER_BYTES: usize = COMMON_HEADER_BYTES + 8 + 4 + 4;
/// Health, maximum health, level, flags and the viewer's target.
const SELF_BYTES: usize = 4 + 4 + 1 + 8 + 4 + 1 + ENTITY_REF_BYTES;
/// A [`CastRecord`]: ability ID (`u32`), flags (bit 0 channel), elapsed and total ticks;
/// ability 0 means no cast.
pub const CAST_RECORD_BYTES: usize = CastRecord::BYTES;
/// Class code, resource kind/value/max, global cooldown, cast, melee damage
/// range, the guard (lane code, protecting flag, break ticks), the melee
/// stance and combo phase codes, breath ticks (#274), the harm stored against the target
/// (#936) and the sword mastery's level and progress (#915).
const SELF_ABILITY_BYTES: usize =
    1 + 1 + 2 + 2 + 2 + CAST_RECORD_BYTES + 2 + 2 + 1 + 1 + 2 + 1 + 1 + 2 + 2 + 1 + 2;
/// The target's own target and the target's cast.
const TARGET_BYTES: usize = ENTITY_REF_BYTES + CAST_RECORD_BYTES;
/// Every section's fixed part: header, self, target, sheet mask (`u16`, #544, #897), summoned
/// count (`u8`), trap count (`u8`), event count (`u8`), chat count (`u8`), entity count (`u16`),
/// projectile count (`u8`) and debris count (`u8`).
pub const PLAYER_SNAPSHOT_FIXED_BYTES: usize =
    HEADER_BYTES + SELF_BYTES + SELF_ABILITY_BYTES + TARGET_BYTES + 2 + 1 + 1 + 1 + 1 + 2 + 1 + 1;
/// A [`SummonedRecord`] (#293): `u32` ID, kind, `3 × i32` centre, `u16` facing, health percent.
pub const SUMMONED_RECORD_BYTES: usize = SummonedRecord::BYTES;
/// A [`TrapRecord`] (#300): `u32` ID and `u16` age.
pub const TRAP_RECORD_BYTES: usize = TrapRecord::BYTES;
/// A [`DebrisRecord`] (#288): `u16` ID, prop kind, piece index, `3 × i32` position, `4 × i16`
/// rotation quaternion and age.
pub const DEBRIS_RECORD_BYTES: usize = DebrisRecord::BYTES;
/// At most this many pieces of debris, nearest first, take what budget the units leave.
pub const MAX_WIRE_DEBRIS: usize = 16;
/// A [`LoadRecord`] (#1077): kind and ownership, `3 × i32` base and `u16` facing.
pub const LOAD_RECORD_BYTES: usize = LoadRecord::BYTES;
/// At most this many set-down loads, nearest first (#1077), before the debris takes the rest.
pub const MAX_WIRE_LOADS: usize = mmorpg_core::MAX_VISIBLE_LOADS;
/// The debris count byte's high bits count the set-down loads after the debris (#1077), so
/// a projection without loads does not grow.
const LOAD_COUNT_SHIFT: u32 = 5;
const DEBRIS_COUNT_MASK: u8 = (1 << LOAD_COUNT_SHIFT) - 1;
const _: () = assert!(MAX_WIRE_DEBRIS <= DEBRIS_COUNT_MASK as usize);
const _: () = assert!(MAX_WIRE_LOADS < 1 << (8 - LOAD_COUNT_SHIFT));
/// A [`ProjectileRecord`] (#286): `u16` ID, kind, `3 × i32` position, `u16` yaw, `i16` pitch
/// and state.
pub const PROJECTILE_RECORD_BYTES: usize = ProjectileRecord::BYTES;
/// At most this many projectiles, nearest first, share a projection's budget.
pub const MAX_WIRE_PROJECTILES: usize = 8;
/// An [`EventRecord`]: kind, `u32` flags or ID, source and target references, `u16` amount.
pub const EVENT_RECORD_BYTES: usize = EventRecord::BYTES;
/// An [`EntityRecord`]: kind, ID, `u32` appearance, `3 × i32` position, `3 × i8` velocity,
/// `u16` facing, level, health percent, flags and the player's weapon (#1088).
pub const ENTITY_RECORD_BYTES: usize = EntityRecord::BYTES;
/// Records that fit the budget without events or sheets: (1,077 − 117) / 30 = 32.
pub const MAX_WIRE_ENTITIES: usize =
    (MAX_PLAYER_PROJECTION_BYTES - PLAYER_SNAPSHOT_FIXED_BYTES) / ENTITY_RECORD_BYTES;
const MAX_EVENT_SECTION_BYTES: usize = MAX_EVENTS_PER_PLAYER * EVENT_RECORD_BYTES;
const MAX_CHAT_SECTION_BYTES: usize = MAX_CHAT_PER_TICK * crate::chat::MAX_CHAT_RECORD_BYTES;
/// Every per-tick section at its largest, with the viewer and its target.
const MAX_PER_TICK_BYTES: usize = PLAYER_SNAPSHOT_FIXED_BYTES
    + MAX_EVENT_SECTION_BYTES
    + MAX_CHAT_SECTION_BYTES
    + mmorpg_core::MAX_VISIBLE_SUMMONED * SUMMONED_RECORD_BYTES
    + mmorpg_core::MAX_VISIBLE_TRAPS * TRAP_RECORD_BYTES
    + 2 * ENTITY_RECORD_BYTES;

/// Every per-tick section at its largest on a tick without events or chat.
pub(crate) const MAX_QUIET_TICK_BYTES: usize =
    MAX_PER_TICK_BYTES - MAX_EVENT_SECTION_BYTES - MAX_CHAT_SECTION_BYTES;

// With every list full, a full event section and four 80-byte chat lines, the viewer and its
// target always fit: 111 + 16 × 17 + 4 × 86 + summoned + traps + 2 × 29 ≤ 1,077 bytes.
// Sheets (#544) take what that leaves and wait for a later tick when they do not fit.
const _: () = assert!(MAX_PER_TICK_BYTES <= MAX_PLAYER_PROJECTION_BYTES);
// A tick without events or chat carries the bag, loot, ability and boss loot sheets at their
// largest at once; the party sheet and the quest, world and waystone sheets, bounded by the
// budget alone, each fit such a tick by itself (#862, #882).
const _: () = assert!(
    MAX_QUIET_TICK_BYTES + crate::sheet::MAX_OTHER_SHEETS_BYTES <= MAX_PLAYER_PROJECTION_BYTES
);
// Budget packing, not the relevance cap, bounds the entity count.
const _: () = assert!(MAX_WIRE_ENTITIES <= MAX_VISIBLE_ENTITIES);

/// A player projection packed within [`MAX_PLAYER_PROJECTION_BYTES`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedSnapshot {
    pub payload: Vec<u8>,
    /// Leading entities of the projection that fit; the rest were omitted.
    pub packed_entities: usize,
    /// Leading projectiles that fit; the rest were omitted.
    pub packed_projectiles: usize,
    /// Leading pieces of debris that fit; the rest were omitted (#288).
    pub packed_debris: usize,
    /// Leading set-down loads that fit; the rest were omitted (#1077).
    pub packed_loads: usize,
    /// Whether a sheet the client lacks waits for a later tick because it did not fit (#544).
    pub deferred_sheets: bool,
}

/// [`pack_snapshot_acknowledged`] for a client that holds no sheet: every sheet goes along
/// that fits.
pub fn pack_snapshot(snapshot: &ZoneSnapshot) -> Result<PackedSnapshot, ProtocolError> {
    pack_snapshot_acknowledged(snapshot, SheetRevisions::default())
}

/// Budget-driven packing: writes the header, the viewer's own state, its
/// target's target, the sheets whose revision differs from the client's
/// `acknowledged` one (#544) as far as they fit beside this tick's sections,
/// this tick's events, then entities in the projection's priority order until
/// the next record would exceed [`MAX_PLAYER_PROJECTION_BYTES`]. The viewer
/// and its target lead the entities and always fit. Hosts publish this;
/// positions travel as full `i32` units.
pub fn pack_snapshot_acknowledged(
    snapshot: &ZoneSnapshot,
    acknowledged: SheetRevisions,
) -> Result<PackedSnapshot, ProtocolError> {
    pack_snapshot_cached(snapshot, acknowledged, &mut SheetEncodingCache::default())
}

/// [`pack_snapshot_acknowledged`] reusing the viewer's unchanged inventory sheet encoding from
/// `cache` (#1007); the bytes are identical to a cold pack's.
pub fn pack_snapshot_cached(
    snapshot: &ZoneSnapshot,
    acknowledged: SheetRevisions,
    cache: &mut SheetEncodingCache,
) -> Result<PackedSnapshot, ProtocolError> {
    pack(snapshot, None, acknowledged, cache)
}

/// Publishes a projection from [`mmorpg_core::ZoneSimulation::project_for_publish`], asked with
/// `cache`'s [`SheetEncodingCache::held`] stamps (#1056): a stamped sheet the core left out or
/// that did not change is sent from `cache` without encoding or hashing it again. The bytes are
/// identical to a cold [`pack_snapshot_acknowledged`] of the complete projection.
pub fn pack_projection_cached(
    projection: &PlayerProjection,
    acknowledged: SheetRevisions,
    cache: &mut SheetEncodingCache,
) -> Result<PackedSnapshot, ProtocolError> {
    let stamps = PublishStamps {
        current: &projection.sheets,
        held: &projection.held,
    };
    pack(&projection.snapshot, Some(stamps), acknowledged, cache)
}

fn pack(
    snapshot: &ZoneSnapshot,
    stamps: Option<PublishStamps<'_>>,
    acknowledged: SheetRevisions,
    cache: &mut SheetEncodingCache,
) -> Result<PackedSnapshot, ProtocolError> {
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
        snapshot.zone_id,
        snapshot.tick,
    );
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
    payload.push(flag_byte(&[viewer.dead, viewer.in_combat]));
    encode_entity_ref(&mut payload, viewer.target);
    // Class appearances fit seven bits. The high bit carries the distinct
    // drawn/sheathed choice without growing the projection datagram (#802).
    payload.push(
        viewer
            .class
            .map_or(0, |choice| u8::try_from(choice.appearance()).unwrap_or(0))
            | (u8::from(viewer.weapon_drawn) << 7),
    );
    match viewer.resource {
        Some(resource) => {
            payload.push(resource_code(resource.kind));
            payload.extend_from_slice(&resource.value.to_be_bytes());
            payload.extend_from_slice(&resource.max.to_be_bytes());
        }
        None => payload.extend_from_slice(&[0; 5]),
    }
    // The global cooldown's length and ticks left share its two bytes (#885): neither exceeds
    // [`GLOBAL_COOLDOWN_TICKS`], which fits a byte.
    const _: () = assert!(GLOBAL_COOLDOWN_TICKS <= u8::MAX as u16);
    let byte = |ticks: u16| u8::try_from(ticks).unwrap_or(u8::MAX);
    payload.extend_from_slice(&[
        byte(viewer.global_cooldown_total),
        byte(viewer.global_cooldown),
    ]);
    viewer.cast.encode(&mut payload)?;
    for value in viewer.damage {
        payload.extend_from_slice(&value.to_be_bytes());
    }
    payload.push(crate::wire::encode_lane(viewer.guard.raised));
    payload.push(u8::from(viewer.guard.protecting));
    payload.extend_from_slice(&viewer.guard.broken.to_be_bytes());
    payload.push(viewer.stance.map_or(0, mmorpg_core::MeleeStance::code));
    payload.push(viewer.combo.map_or(0, mmorpg_core::ComboPhase::code));
    payload.extend_from_slice(&viewer.breath.to_be_bytes());
    payload.extend_from_slice(&viewer.reckoning.to_be_bytes());
    payload.push(viewer.mastery.level);
    payload.extend_from_slice(&viewer.mastery.progress.to_be_bytes());
    encode_entity_ref(&mut payload, snapshot.target_of_target);
    snapshot.target_detail.cast.encode(&mut payload)?;

    let mut world_payload = Vec::new();
    crate::wire::encode_summoned_views(&mut world_payload, &snapshot.summoned)?;
    crate::wire::encode_trap_views(&mut world_payload, &snapshot.traps)?;
    let mut chat_payload = Vec::new();
    chat_payload.push(encode_u8_count(
        snapshot.chat.len(),
        MAX_CHAT_PER_TICK,
        "player projection has too many chat lines",
    )?);
    for line in &snapshot.chat {
        crate::chat::encode_line(&mut chat_payload, line);
    }
    // Sheets the client lacks take what this tick's sections, the viewer and its target leave;
    // the rest wait for a later tick (#544).
    let mut room = MAX_PLAYER_PROJECTION_BYTES.saturating_sub(
        payload.len()
            + 2
            + world_payload.len()
            + 1
            + usize::from(event_count) * EVENT_RECORD_BYTES
            + chat_payload.len()
            + 2
            + 2 * ENTITY_RECORD_BYTES
            + 1
            + 1,
    );
    let mask_offset = payload.len();
    payload.extend_from_slice(&[0, 0]);
    let mut mask = 0_u16;
    let mut deferred_sheets = false;
    for (sheet, encoded) in Sheet::ALL
        .into_iter()
        .zip(encode_sheets(snapshot, stamps, cache)?)
    {
        let Some((bytes, hash)) = encoded else {
            continue;
        };
        if acknowledged.get(sheet) == hash {
            continue;
        }
        if bytes.len() > room {
            deferred_sheets = true;
            continue;
        }
        room -= bytes.len();
        mask |= 1 << sheet.index();
        payload.extend_from_slice(&bytes);
    }
    payload[mask_offset..mask_offset + 2].copy_from_slice(&mask.to_be_bytes());
    payload.extend_from_slice(&world_payload);
    // Plan a conservative entity prefix once, before removing cosmetic records.
    // Freed event bytes do not grow that prefix and cannot cause a packing cycle.
    let reserved =
        payload.len() + 1 + usize::from(event_count) * EVENT_RECORD_BYTES + chat_payload.len() + 2;
    // Projectiles (#286) come after the viewer and its target but before other entities.
    let projectiles = (MAX_PLAYER_PROJECTION_BYTES
        .saturating_sub(reserved + 1 + 2 * ENTITY_RECORD_BYTES)
        / PROJECTILE_RECORD_BYTES)
        .min(MAX_WIRE_PROJECTILES)
        .min(snapshot.projectiles.len());
    // The projectile count and records, then the debris count byte.
    let reserved = reserved + 1 + projectiles * PROJECTILE_RECORD_BYTES + 1;
    let planned = (MAX_PLAYER_PROJECTION_BYTES.saturating_sub(reserved) / ENTITY_RECORD_BYTES)
        .min(MAX_WIRE_ENTITIES)
        .min(snapshot.entities.len());
    let visible: BTreeSet<_> = snapshot.entities[..planned]
        .iter()
        .map(EntitySnapshot::entity)
        .collect();
    let events: Vec<_> = snapshot
        .events
        .iter()
        .filter(|event| {
            event
                .observation_refs(snapshot.viewer_id)
                .is_none_or(|refs| {
                    refs.into_iter()
                        .flatten()
                        .all(|entity| visible.contains(&entity))
                })
        })
        .collect();
    payload.push(encode_u8_count(
        events.len(),
        MAX_EVENTS_PER_PLAYER,
        "too many projection events",
    )?);
    for event in events {
        event.encode(&mut payload)?;
    }
    payload.extend_from_slice(&chat_payload);

    let count_offset = payload.len();
    payload.extend_from_slice(&0_u16.to_be_bytes());
    let mut packed_entities = 0_u16;
    // Projectiles and the debris count follow the entities.
    let projectile_bytes = 1 + projectiles * PROJECTILE_RECORD_BYTES + 1;
    for entity in snapshot.entities.iter().take(planned) {
        if payload.len() + ENTITY_RECORD_BYTES + projectile_bytes > MAX_PLAYER_PROJECTION_BYTES {
            break;
        }
        entity.encode(&mut payload)?;
        packed_entities += 1;
    }
    payload[count_offset..count_offset + 2].copy_from_slice(&packed_entities.to_be_bytes());
    payload.push(u8::try_from(projectiles).unwrap_or(u8::MAX));
    for projectile in &snapshot.projectiles[..projectiles] {
        projectile.encode(&mut payload)?;
    }
    // Set-down loads (#1077), then debris (#288), take what the units left; one byte counts
    // both.
    let room = MAX_PLAYER_PROJECTION_BYTES.saturating_sub(payload.len() + 1);
    let loads = (room / LOAD_RECORD_BYTES)
        .min(MAX_WIRE_LOADS)
        .min(snapshot.loads.len());
    let debris = ((room - loads * LOAD_RECORD_BYTES) / DEBRIS_RECORD_BYTES)
        .min(MAX_WIRE_DEBRIS)
        .min(snapshot.debris.len());
    payload.push(u8::try_from(debris | loads << LOAD_COUNT_SHIFT).unwrap_or(u8::MAX));
    for piece in &snapshot.debris[..debris] {
        piece.encode(&mut payload)?;
    }
    for load in &snapshot.loads[..loads] {
        load.encode(&mut payload)?;
    }
    Ok(PackedSnapshot {
        payload,
        packed_entities: usize::from(packed_entities),
        packed_projectiles: projectiles,
        packed_debris: debris,
        packed_loads: loads,
        deferred_sheets,
    })
}

/// Encodes a complete player projection. Fails closed, never truncating
/// silently, when any entity would not fit [`MAX_PLAYER_PROJECTION_BYTES`].
/// Fixtures and tests use it; hosts publish with [`pack_snapshot`].
pub fn encode_snapshot(snapshot: &ZoneSnapshot) -> Result<Vec<u8>, ProtocolError> {
    let packed = pack_snapshot(snapshot)?;
    if packed.packed_entities < snapshot.entities.len()
        || packed.packed_projectiles < snapshot.projectiles.len()
        || packed.packed_debris < snapshot.debris.len()
        || packed.packed_loads < snapshot.loads.len()
        || packed.deferred_sheets
    {
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
        ResourceKind::Faith => 4,
    }
}

/// Decodes one projection without any sheet received before: sheets it does not carry are
/// absent (no bag, quests or loot, no cooldowns or auras).
pub fn decode_snapshot(payload: &[u8]) -> Result<ZoneSnapshot, ProtocolError> {
    SnapshotDecoder::new().decode(payload)
}

/// A sheet as a client received it (#544).
#[derive(Clone, Debug, Eq, PartialEq)]
struct ReceivedSheet {
    revision: u32,
    /// The projection tick it arrived with; its timers run from there.
    tick: u64,
    bytes: Vec<u8>,
}

/// A client's projection decoder (#544): it keeps the newest copy of every sheet, so each
/// decoded projection is complete, and reports the revisions it holds for the client to
/// echo with [`crate::encode_acknowledged_command`]. One per session; a sheet a projection
/// does not carry keeps its last copy, timers running down.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SnapshotDecoder {
    sheets: [Option<ReceivedSheet>; SHEET_COUNT],
}

impl SnapshotDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The revisions of the sheets held, `0` for none.
    #[must_use]
    pub fn revisions(&self) -> SheetRevisions {
        let mut revisions = SheetRevisions::default();
        for (sheet, received) in Sheet::ALL.into_iter().zip(&self.sheets) {
            revisions.set(
                sheet,
                received.as_ref().map_or(0, |received| received.revision),
            );
        }
        revisions
    }

    /// Decodes a projection strictly; a malformed one leaves the held sheets as they were.
    pub fn decode(&mut self, payload: &[u8]) -> Result<ZoneSnapshot, ProtocolError> {
        let (snapshot, sheets) = decode_projection(payload, &self.sheets)?;
        self.sheets = sheets;
        Ok(snapshot)
    }
}

fn decode_projection(
    payload: &[u8],
    held: &[Option<ReceivedSheet>; SHEET_COUNT],
) -> Result<(ZoneSnapshot, [Option<ReceivedSheet>; SHEET_COUNT]), ProtocolError> {
    if payload.len() > MAX_PLAYER_PROJECTION_BYTES {
        return Err(ProtocolError::new(
            "snapshot exceeds the player projection byte budget",
        ));
    }
    let mut offset = 0;
    let (zone_id, tick) = decode_common_header(payload, &mut offset, PLAYER_SNAPSHOT_SCOPE)?;
    let content_revision = u64::from_be_bytes(take(payload, &mut offset)?);
    let acknowledged_sequence = u32::from_be_bytes(take(payload, &mut offset)?);
    let viewer_id = u32::from_be_bytes(take(payload, &mut offset)?);

    let health = u32::from_be_bytes(take(payload, &mut offset)?);
    let max_health = u32::from_be_bytes(take(payload, &mut offset)?);
    let level = read_u8(payload, &mut offset)?;
    let experience = u32::from_be_bytes(take(payload, &mut offset)?);
    let experience_to_next_level = u32::from_be_bytes(take(payload, &mut offset)?);
    let copper = u32::from_be_bytes(take(payload, &mut offset)?);
    let [dead, in_combat] = read_flags(read_u8(payload, &mut offset)?)?;
    let target = decode_entity_ref(payload, &mut offset)?;
    if !(1..=10).contains(&level)
        || (level == 10 && (experience != 0 || experience_to_next_level != 0))
        || (level < 10 && (experience_to_next_level == 0 || experience >= experience_to_next_level))
        || health > max_health
        || dead != (health == 0)
    {
        return Err(ProtocolError::new("viewer state is inconsistent"));
    }
    let encoded_class = read_u8(payload, &mut offset)?;
    let weapon_drawn = encoded_class & 0x80 != 0;
    let class_code = encoded_class & 0x7f;
    let class = ClassChoice::from_appearance(u32::from(class_code));
    if class_code != 0 && class.is_none() {
        return Err(ProtocolError::new("viewer ability state is inconsistent"));
    }
    let resource_kind = read_u8(payload, &mut offset)?;
    let resource_value = u16::from_be_bytes(take(payload, &mut offset)?);
    let resource_max = u16::from_be_bytes(take(payload, &mut offset)?);
    let [global_cooldown_total, global_cooldown] = take::<2>(payload, &mut offset)?.map(u16::from);
    let cast = Option::<CastView>::decode(payload, &mut offset)?;
    let damage = [
        u16::from_be_bytes(take(payload, &mut offset)?),
        u16::from_be_bytes(take(payload, &mut offset)?),
    ];
    if damage[0] > damage[1] {
        return Err(ProtocolError::new("viewer damage range is inverted"));
    }
    let guard = GuardView {
        raised: crate::wire::decode_lane(read_u8(payload, &mut offset)?)?,
        protecting: read_bool(payload, &mut offset)?,
        broken: u16::from_be_bytes(take(payload, &mut offset)?),
    };
    let warden = class.is_some_and(|choice| choice.class == mmorpg_core::PlayerClass::Warden);
    if guard.broken > mmorpg_core::GUARD_BREAK_TICKS
        || (dead && guard != GuardView::default())
        || (guard.protecting && (guard.raised.is_none() || guard.broken > 0 || !warden))
    {
        return Err(ProtocolError::new("viewer guard state is inconsistent"));
    }
    let stance = match read_u8(payload, &mut offset)? {
        0 => None,
        code => Some(
            mmorpg_core::MeleeStance::from_code(code)
                .filter(|_| warden && !dead)
                .ok_or_else(|| ProtocolError::new("viewer melee stance is invalid"))?,
        ),
    };
    let combo = match read_u8(payload, &mut offset)? {
        0 => None,
        code => Some(
            mmorpg_core::ComboPhase::from_code(code)
                .filter(|_| warden && !dead)
                .ok_or_else(|| ProtocolError::new("viewer combo phase is invalid"))?,
        ),
    };
    let breath = u16::from_be_bytes(take(payload, &mut offset)?);
    if breath > mmorpg_core::water::BREATH_MAX_TICKS {
        return Err(ProtocolError::new("viewer breath is out of range"));
    }
    // The harm a Divine Reckoning would draw on against the target (#936).
    let reckoning = u16::from_be_bytes(take(payload, &mut offset)?);
    // The viewer's sword mastery (#915).
    let mastery = mmorpg_core::Mastery {
        level: read_u8(payload, &mut offset)?,
        progress: u16::from_be_bytes(take(payload, &mut offset)?),
    };
    if !mastery.is_valid() {
        return Err(ProtocolError::new("viewer weapon mastery is out of range"));
    }
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
    if global_cooldown > global_cooldown_total
        || global_cooldown_total > GLOBAL_COOLDOWN_TICKS
        || (class.is_none() && (global_cooldown_total != 0 || cast.is_some()))
        || (dead && cast.is_some())
    {
        return Err(ProtocolError::new("viewer ability state is inconsistent"));
    }
    let target_of_target = decode_entity_ref(payload, &mut offset)?;
    let target_cast = Option::<CastView>::decode(payload, &mut offset)?;
    if target.is_none() && target_cast.is_some() {
        return Err(ProtocolError::new("target detail needs a target"));
    }

    let mask = u16::from_be_bytes(take(payload, &mut offset)?);
    if u32::from(mask) >> SHEET_COUNT != 0 {
        return Err(ProtocolError::new("unknown sheet in the sheet mask"));
    }
    let mut fresh: [Option<ReceivedSheet>; SHEET_COUNT] = Default::default();
    for sheet in Sheet::ALL {
        if mask & (1 << sheet.index()) != 0 {
            let bytes = crate::sheet::read_sheet(sheet, payload, &mut offset, tick)?;
            fresh[sheet.index()] = Some(ReceivedSheet {
                revision: revision(bytes),
                tick,
                bytes: bytes.to_vec(),
            });
        }
    }
    let summoned = crate::wire::decode_summoned_views(payload, &mut offset)?;
    let traps = crate::wire::decode_trap_views(payload, &mut offset)?;
    let event_count = decode_u8_count(
        payload,
        &mut offset,
        MAX_EVENTS_PER_PLAYER,
        "snapshot exceeds the per-player event capacity",
    )?;
    let mut events = Vec::with_capacity(event_count);
    for _ in 0..event_count {
        events.push(ZoneEvent::decode(payload, &mut offset)?);
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
        entities.push(EntitySnapshot::decode(payload, &mut offset)?);
    }
    let count = decode_u8_count(
        payload,
        &mut offset,
        MAX_WIRE_PROJECTILES,
        "snapshot has too many projectiles",
    )?;
    let mut projectiles = Vec::with_capacity(count);
    for _ in 0..count {
        projectiles.push(mmorpg_core::ProjectileView::decode(payload, &mut offset)?);
    }
    // One byte counts the debris (low bits) and the set-down loads after it (#1077).
    let [counts] = take(payload, &mut offset)?;
    let count = usize::from(counts & DEBRIS_COUNT_MASK);
    if count > MAX_WIRE_DEBRIS {
        return Err(ProtocolError::new("snapshot has too much debris"));
    }
    let mut debris = Vec::with_capacity(count);
    for _ in 0..count {
        debris.push(mmorpg_core::DebrisView::decode(payload, &mut offset)?);
    }
    let count = usize::from(counts >> LOAD_COUNT_SHIFT);
    if count > MAX_WIRE_LOADS {
        return Err(ProtocolError::new("snapshot has too many loads"));
    }
    let mut loads = Vec::with_capacity(count);
    for _ in 0..count {
        loads.push(mmorpg_core::LoadView::decode(payload, &mut offset)?);
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

    // The newest copy of each sheet: this projection's, unless a later one arrived first.
    let mut sheets = held.clone();
    let mut current = [false; SHEET_COUNT];
    for (index, received) in fresh.into_iter().enumerate() {
        if let Some(received) = received
            && sheets[index].as_ref().is_none_or(|kept| kept.tick <= tick)
        {
            sheets[index] = Some(received);
            current[index] = true;
        }
    }
    let sheet_bytes = |sheet: Sheet| sheets[sheet.index()].as_ref();
    let (inventory_revision, inventory, equipment) = match sheet_bytes(Sheet::Inventory) {
        Some(received) => {
            let (revision, inventory, equipment) =
                crate::sheet::decode_inventory_sheet(&received.bytes)?;
            (revision, Some(inventory), Some(equipment))
        }
        None => (0, None, None),
    };
    let quests = sheet_bytes(Sheet::Quests)
        .map(|received| crate::sheet::decode_quest_sheet(&received.bytes))
        .transpose()?;
    let loot = match sheet_bytes(Sheet::Loot) {
        Some(received) => crate::sheet::decode_loot_sheet(&received.bytes)?,
        None => None,
    };
    let loot = match loot {
        Some(view) if !loot_matches(view, dead, target, tick, &entities) => {
            if current[Sheet::Loot.index()] {
                return Err(ProtocolError::new("corpse loot sheet is inconsistent"));
            }
            // A held sheet the projection outgrew: the client has no current corpse sheet.
            None
        }
        loot => loot,
    };
    let abilities = match sheet_bytes(Sheet::Abilities) {
        Some(received) => crate::sheet::decode_ability_sheet(&received.bytes, received.tick, tick)?,
        None => Abilities {
            target,
            ..Abilities::default()
        },
    };
    let inconsistent = (class.is_none()
        && (!abilities.cooldowns.is_empty() || !abilities.auras.is_empty()))
        || (dead && !abilities.auras.is_empty())
        || abilities.target != target;
    let (doors, pieces, raid, depleted_veins) = sheet_bytes(Sheet::World)
        .map(|received| crate::sheet::decode_world_sheet(&received.bytes))
        .transpose()?
        .unwrap_or_default();
    let (waystones, discoveries) = sheet_bytes(Sheet::Waystones)
        .map(|received| crate::sheet::decode_waystone_sheet(&received.bytes))
        .transpose()?
        .unwrap_or_default();
    let depths_loot = sheet_bytes(Sheet::DepthsLoot)
        .map(|received| crate::depths::decode_loot_sheet(&received.bytes))
        .transpose()?;
    let party = sheet_bytes(Sheet::Party)
        .map(|received| crate::party::decode_party_state(&received.bytes))
        .transpose()?;
    let abilities = if !inconsistent {
        abilities
    } else if current[Sheet::Abilities.index()] {
        return Err(ProtocolError::new("viewer ability state is inconsistent"));
    } else {
        // A held sheet from before a death, class choice or target change shows only what
        // still applies until its successor arrives.
        Abilities {
            target,
            cooldowns: if class.is_none() {
                Vec::new()
            } else {
                abilities.cooldowns
            },
            auras: if class.is_none() || dead {
                Vec::new()
            } else {
                abilities.auras
            },
            target_auras: if abilities.target == target {
                abilities.target_auras
            } else {
                Vec::new()
            },
        }
    };
    let snapshot = ZoneSnapshot {
        content_revision,
        acknowledged_sequence,
        viewer_id,
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
            weapon_drawn,
            target,
            class,
            resource,
            cast,
            global_cooldown,
            global_cooldown_total,
            damage,
            guard,
            stance,
            combo,
            breath,
            reckoning,
            mastery,
            waystones,
            discoveries,
        },
        cooldowns: abilities.cooldowns,
        auras: abilities.auras,
        target_of_target,
        target_detail: TargetDetail {
            cast: target_cast,
            auras: abilities.target_auras,
        },
        inventory_revision,
        inventory,
        equipment,
        loot,
        events,
        chat,
        entities,
        doors,
        projectiles,
        debris,
        loads,
        pieces,
        depleted_veins,
        summoned,
        traps,
        raid,
        quests,
        party,
        depths_loot,
    };
    Ok((snapshot, sheets))
}

/// Whether a corpse sheet belongs to the viewer's living selection of a lootable corpse.
fn loot_matches(
    view: mmorpg_core::LootView,
    dead: bool,
    target: Option<mmorpg_core::EntityRef>,
    tick: u64,
    entities: &[EntitySnapshot],
) -> bool {
    let corpse = entities
        .iter()
        .find(|entity| entity.entity() == mmorpg_core::EntityRef::Creature(view.claim.creature));
    !(dead
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
        }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::inventory::{EQUIPMENT_RECORD_BYTES, INVENTORY_RECORD_BYTES};
    use crate::{
        DATAGRAM_SAFETY_MARGIN_BYTES, MEASURED_MIN_DATAGRAM_BYTES, SESSION_SNAPSHOT_HEADER_BYTES,
    };
    use mmorpg_core::{AbilityId, EntityFlags, ability_by_id};
    use mmorpg_core::{AuraKind, AuraView, Cooldown, MAX_COOLDOWNS};
    use mmorpg_core::{
        CreatureId, EntityRef, ErrorCode, NpcId, PlayerId, ZoneCommand, ZoneEvent, ZoneId,
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

    #[test]
    fn unchanged_inventory_sheets_reuse_their_encoding_byte_for_byte() {
        let mut snapshot = fixture_snapshot();
        let mut cache = SheetEncodingCache::default();
        let mut decoder = SnapshotDecoder::new();
        // Each publication matches the stateless reference, held sheets or not.
        let mut publish = |snapshot: &ZoneSnapshot, cache: &mut SheetEncodingCache| {
            let acknowledged = decoder.revisions();
            let packed = pack_snapshot_cached(snapshot, acknowledged, cache).unwrap();
            assert_eq!(
                packed,
                pack_snapshot_acknowledged(snapshot, acknowledged).unwrap()
            );
            decoder.decode(&packed.payload).unwrap();
            packed.payload
        };
        let first = publish(&snapshot, &mut cache);
        publish(&snapshot, &mut cache);
        let acknowledged = publish(&snapshot, &mut cache);
        assert!(acknowledged.len() < first.len());
        assert_eq!(cache.inventory_encodes, 1);
        assert_eq!(publish(&snapshot, &mut cache), acknowledged);
        assert_eq!(cache.inventory_encodes, 1);

        // A revision bump re-encodes once, then reuses again.
        snapshot.inventory_revision += 1;
        publish(&snapshot, &mut cache);
        publish(&snapshot, &mut cache);
        assert_eq!(cache.inventory_encodes, 2);

        // A replaced or restored character may reuse the revision for another bag.
        snapshot.inventory = Some(mmorpg_core::Inventory::default());
        publish(&snapshot, &mut cache);
        assert_eq!(cache.inventory_encodes, 3);

        // A cold cache packs the same bytes.
        let mut cold = SheetEncodingCache::default();
        assert_eq!(
            pack_snapshot_cached(&snapshot, SheetRevisions::default(), &mut cold).unwrap(),
            pack_snapshot(&snapshot).unwrap()
        );
    }

    /// Publishing through `project_for_publish` (#1056) leaves unchanged sheets unbuilt and
    /// unencoded, builds the zone-wide views once per tick, and still packs exactly the bytes
    /// of a cold pack of the complete projection.
    #[test]
    fn published_projections_skip_unchanged_sheets_byte_for_byte() {
        let content = mmorpg_core::greyhaven_vale::content();
        let mut zone = mmorpg_core::ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
        let players: Vec<PlayerId> = (1..=4).collect();
        for &player in &players {
            zone.add_player(player).unwrap();
        }
        let mut caches: Vec<SheetEncodingCache> = players
            .iter()
            .map(|_| SheetEncodingCache::default())
            .collect();
        let mut decoders: Vec<SnapshotDecoder> =
            players.iter().map(|_| SnapshotDecoder::new()).collect();
        // One publication of every player: (sheets built, stamped sheets encoded) in total.
        let mut publish = |zone: &mmorpg_core::ZoneSimulation| {
            let views = zone.zone_view_builds();
            let mut work = (0, 0);
            for (index, &player) in players.iter().enumerate() {
                let (cache, decoder) = (&mut caches[index], &mut decoders[index]);
                let acknowledged = decoder.revisions();
                let encodes = cache.stamped_encodes;
                let projection = zone.project_for_publish(player, &cache.held()).unwrap();
                let packed = pack_projection_cached(&projection, acknowledged, cache).unwrap();
                let complete = zone.snapshot_for_player(player).unwrap();
                assert_eq!(
                    packed,
                    pack_snapshot_acknowledged(&complete, acknowledged).unwrap()
                );
                assert!(
                    projection.work.entity_records_built
                        <= projection.stats.relevant.min(MAX_VISIBLE_ENTITIES)
                );
                decoder.decode(&packed.payload).unwrap();
                work.0 += projection.work.sheets_built;
                work.1 += cache.stamped_encodes - encodes;
            }
            assert!(zone.zone_view_builds() - views <= 1, "one build per tick");
            work
        };

        // A cold publication builds and encodes every stamped sheet of every viewer.
        assert_eq!(publish(&zone), (16, 16));
        // Warm, idle viewers build and encode none, tick after tick.
        for _ in 0..3 {
            zone.advance_tick().unwrap();
            assert_eq!(publish(&zone), (0, 0));
        }
        // A bag change rebuilds the mover's bag and quest sheets only.
        zone.apply_command(
            1,
            1,
            ZoneCommand::MoveItem {
                source: 0,
                destination: 5,
                quantity: 1,
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
        assert_eq!(publish(&zone), (2, 2));
        zone.advance_tick().unwrap();
        assert_eq!(publish(&zone), (0, 0));
    }
    /// The shared Rust/browser golden projection: a level-4 Arcanist casting
    /// Firebolt behind an Arcane Barrier at a rooted Mirefin Lurker that
    /// casts Muck Bolt, next to an NPC and a corpse tapped by another player,
    /// wearing a wand, hood and tunic (3 stamina, 6 intellect), with one event
    /// of every kind.
    pub(crate) fn fixture_snapshot() -> ZoneSnapshot {
        let mut slots = [None; mmorpg_core::INVENTORY_SLOTS];
        slots[0] = Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(1), 3).unwrap());
        slots[1] = Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(2), 1).unwrap());
        slots[15] = Some(mmorpg_core::ItemStack::new(mmorpg_core::ItemId::new(1), 20).unwrap());
        ZoneSnapshot {
            debris: Vec::new(),
            loads: Vec::new(),
            party: None,
            depths_loot: None,
            raid: Some(mmorpg_core::RaidView {
                phase: 2,
                integrity: 63,
                aid: BTreeSet::new(),
                signs: false,
                warned: true,
                braced: BTreeSet::from([1]),
                scouted: false,
                standing: None,
            }),
            pieces: vec![
                mmorpg_core::PieceView {
                    piece: mmorpg_core::PieceId::new(2),
                    stage: mmorpg_core::PieceStage::Damaged,
                },
                mmorpg_core::PieceView {
                    piece: mmorpg_core::PieceId::new(0x0105),
                    stage: mmorpg_core::PieceStage::Destroyed,
                },
            ],
            depleted_veins: vec![1],
            traps: vec![mmorpg_core::TrapView {
                id: mmorpg_core::TrapId::new(1),
                age: 30,
            }],
            summoned: vec![
                mmorpg_core::SummonedView {
                    id: 3,
                    kind: mmorpg_core::SummonedTerrainKind::IceWall,
                    centre: [-2_000, 140, 310],
                    facing: 0x4000,
                    health_percent: 80,
                },
                mmorpg_core::SummonedView {
                    id: 7,
                    kind: mmorpg_core::SummonedTerrainKind::EarthPillar,
                    centre: [12, 180, -40],
                    facing: 9,
                    health_percent: 100,
                },
            ],
            doors: vec![
                mmorpg_core::DoorView {
                    door: mmorpg_core::DoorId::new(1),
                    open: true,
                },
                mmorpg_core::DoorView {
                    door: mmorpg_core::DoorId::new(2),
                    open: false,
                },
            ],
            projectiles: vec![
                mmorpg_core::ProjectileView {
                    id: 3,
                    kind: mmorpg_core::ProjectileKind::Arrow,
                    position: [120, 110, -340],
                    yaw: 16_384,
                    pitch: -900,
                    stuck: false,
                    burst: false,
                },
                mmorpg_core::ProjectileView {
                    id: 65_535,
                    kind: mmorpg_core::ProjectileKind::Bolt,
                    position: [-32_768, 0, 32_767],
                    yaw: 0,
                    pitch: 16_384,
                    stuck: true,
                    burst: false,
                },
                mmorpg_core::ProjectileView {
                    id: 7,
                    kind: mmorpg_core::ProjectileKind::Missile,
                    position: [60, 130, -200],
                    yaw: 49_152,
                    pitch: 0,
                    stuck: false,
                    burst: false,
                },
                mmorpg_core::ProjectileView {
                    id: 9,
                    kind: mmorpg_core::ProjectileKind::Bomb,
                    position: [-40, 12, 75],
                    yaw: 0,
                    pitch: 0,
                    stuck: false,
                    burst: true,
                },
            ],
            quests: Some(quest_sheet()),
            loot: None,
            content_revision: 4,
            acknowledged_sequence: 81,
            viewer_id: 7,
            inventory_revision: 9,
            inventory: Some(mmorpg_core::Inventory::from_slots(slots)),
            equipment: Some(equipment(&[4, 6, 7])),
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
                weapon_drawn: false,
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
                    elapsed: mmorpg_core::golden::t30(20) as _,
                    total: mmorpg_core::golden::t30(60) as _,
                    channel: false,
                }),
                global_cooldown: 25,
                global_cooldown_total: 60,
                // Level 4's 6–9 plus half of 6 intellect.
                damage: [9, 12],
                // A held guard recovering from a break.
                guard: GuardView {
                    raised: Some(mmorpg_core::StrikeHeight::High),
                    protecting: false,
                    broken: 30,
                },
                // Only a Warden has a melee stance or combo.
                stance: None,
                combo: None,
                breath: mmorpg_core::water::BREATH_MAX_TICKS,
                // Harm stored against the target (#936).
                reckoning: 123,
                mastery: mmorpg_core::Mastery {
                    level: 1,
                    progress: 42,
                },
                waystones: mmorpg_core::WaystoneSet::EMPTY,
                discoveries: std::collections::BTreeSet::new(),
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
                    elapsed: mmorpg_core::golden::t30(10) as _,
                    total: mmorpg_core::golden::t30(45) as _,
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
                ZoneEvent::LostInterest { source: WOLF },
                ZoneEvent::Died {
                    entity: WOLF,
                    killer: Some(VIEWER),
                },
                ZoneEvent::Error {
                    code: ErrorCode::OutOfRange,
                    target: Some(WOLF),
                },
                ZoneEvent::StrikeSwept {
                    source: VIEWER,
                    target: Some(WOLF),
                    ability: AbilityId::new(1),
                    aim: mmorpg_core::StrikeAim::new(
                        mmorpg_core::StrikeSide::Left,
                        mmorpg_core::StrikeHeight::High,
                    ),
                },
                ZoneEvent::CastStarted {
                    source: WOLF,
                    target: Some(VIEWER),
                    ability: MUCK_BOLT,
                    ticks: mmorpg_core::golden::t30(45) as _,
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
                    ticks: mmorpg_core::golden::t30(180) as _,
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
                        weapon_drawn: false,
                        weapon: None,
                        named_weapon: None,
                        great_weapon: None,
                        shapeshifted: false,
                        load: None,
                        hostile: true,
                        attackable: true,
                        tapped_by_other: false,
                        evading: false,
                        targets_viewer: true,
                        traversal: mmorpg_core::TraversalView::Free,
                        posture: mmorpg_core::PostureView::Free,
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
            completed: [mmorpg_core::QuestId::new(1)].into(),
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

    /// Twenty-two turned-in quests, ten active ones with full progress bytes and eight markers.
    fn full_quest_sheet() -> mmorpg_core::QuestSheet {
        mmorpg_core::QuestSheet {
            completed: (11..=32).map(mmorpg_core::QuestId::new).collect(),
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
    fn equipment(items: &[u32]) -> mmorpg_core::Equipment {
        let mut slots = [None; mmorpg_core::EQUIPMENT_SLOTS];
        for &item in items {
            let id = mmorpg_core::ItemId::new(item);
            let slot = mmorpg_core::item_template(id).unwrap().slot.unwrap();
            slots[usize::from(slot.index())] = Some(id);
        }
        mmorpg_core::Equipment::from_slots(slots).unwrap()
    }

    /// The shared observer-cue golden projection (#27): another player attacking, damaging and
    /// swinging at a creature, as the viewer sees it.
    pub(crate) fn observation_fixture_snapshot() -> ZoneSnapshot {
        let mut zone = mmorpg_core::ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        let mut view = zone.snapshot_for_player(1).unwrap();
        view.content_revision = 1;
        // No bag or quest sheet to send, as to a client that holds them (#544).
        view.inventory_revision = 0;
        view.inventory = None;
        view.equipment = None;
        view.quests = None;
        view.entities[0].position = [0, 90, 0];
        let mut other = view.entities[0].clone();
        other.id = 2;
        other.position = [100, 90, 0];
        view.entities.push(other);
        let mut creature = view.entities[0].clone();
        creature.kind = mmorpg_core::EntityKind::Creature;
        creature.flags.weapon = None;
        creature.id = 101;
        creature.appearance = 1;
        creature.position = [200, 45, 0];
        creature.flags.hostile = true;
        creature.flags.attackable = true;
        view.entities.push(creature);
        let source = EntityRef::Player(2);
        let target = EntityRef::Creature(CreatureId::new(101));
        view.events = vec![
            ZoneEvent::AttackPerformed {
                source,
                target: Some(target),
            },
            ZoneEvent::DamageObserved {
                source,
                target,
                amount: 7,
                critical: true,
            },
            ZoneEvent::SwingBegan {
                source,
                ability: AbilityId::new(1),
                facing: 16384,
                aim: mmorpg_core::StrikeAim::new(
                    mmorpg_core::StrikeSide::Centre,
                    mmorpg_core::StrikeHeight::Middle,
                ),
                startup: 18,
                active: 9,
            },
        ];
        view
    }

    #[test]
    fn observation_records_round_trip_through_the_shared_fixture() {
        let view = observation_fixture_snapshot();
        let bytes = encode_snapshot(&view).unwrap();
        let expected = crate::fixture::golden_wire_hex(
            mmorpg_core::golden::workspace_root()
                .join("fixtures/protocol/presentation-observation.hex"),
            &bytes,
        );
        assert_eq!(bytes, expected);
        assert_eq!(decode_snapshot(&bytes).unwrap(), view);
        let packed = pack_snapshot(&view).unwrap();
        assert!(packed.payload.len() <= crate::MAX_PLAYER_PROJECTION_BYTES);
        assert_eq!(
            decode_snapshot(&packed.payload).unwrap().events,
            view.events
        );
        let mut zone = mmorpg_core::ZoneSimulation::new(ZoneId::new(1));
        zone.add_player(1).unwrap();
        let mut state = zone.snapshot().unwrap();
        state.players[0].combat.events = view.events.clone();
        assert_eq!(
            crate::decode_canonical_snapshot(&crate::encode_canonical_snapshot(&state).unwrap())
                .unwrap(),
            state
        );
    }

    /// The checked-in player snapshot fixture (`MMORPG_UPDATE_FIXTURES=1` rewrites it).
    fn fixture_bytes() -> Vec<u8> {
        crate::fixture::golden_wire_hex(
            mmorpg_core::golden::workspace_root().join("fixtures/protocol/player-snapshot.hex"),
            &encode_snapshot(&fixture_snapshot()).unwrap(),
        )
    }

    /// Byte offsets of the fixture's sections.
    const CLASS: usize = 59;
    const CAST: usize = CLASS + 8;
    const DAMAGE: usize = CAST + CAST_RECORD_BYTES;
    const GUARD: usize = DAMAGE + 4;
    const STANCE: usize = GUARD + 4;
    const COMBO: usize = STANCE + 1;
    const BREATH: usize = COMBO + 1;
    /// The harm stored against the target (#936).
    const RECKONING: usize = BREATH + 2;
    /// The sword mastery's level and progress (#915).
    const MASTERY: usize = RECKONING + 2;
    const TARGET_OF_TARGET: usize = MASTERY + 3;
    const TARGET_CAST: usize = TARGET_OF_TARGET + ENTITY_REF_BYTES;
    const SHEET_MASK: usize = TARGET_CAST + CAST_RECORD_BYTES;
    /// The `u16` sheet mask (#897).
    fn sheet_mask(payload: &[u8]) -> u16 {
        u16::from_be_bytes([payload[SHEET_MASK], payload[SHEET_MASK + 1]])
    }
    /// The bag sheet (#544): inventory revision, bag, equipment and stat totals.
    const INVENTORY: usize = SHEET_MASK + 2;
    const EVENT_COUNT: usize = 16;
    const EQUIPMENT: usize = INVENTORY + 8 + INVENTORY_RECORD_BYTES;
    const STATS: usize = EQUIPMENT + EQUIPMENT_RECORD_BYTES;
    const QUESTS: usize = STATS + 8;
    /// The fixture's quest sheet: one turned-in quest, one entry and two markers.
    const QUEST_BYTES: usize = 2 + 4 + 1 + crate::quest::QUEST_ENTRY_BYTES + 2 + 2 * 5;
    /// The loot sheet: no corpse sheet.
    const LOOT: usize = QUESTS + QUEST_BYTES;
    /// The ability sheet: its target, then the cooldown, aura and target aura lists.
    const ABILITIES: usize = LOOT + 1;
    const COOLDOWNS: usize = ABILITIES + ENTITY_REF_BYTES;
    const AURAS: usize = COOLDOWNS + 1 + 2 * 6;
    const TARGET_AURAS: usize = AURAS + 1 + 9;
    /// The world sheet (#882): two doors, two pieces, the raid signal with one braced spot and
    /// one spent vein (#917).
    const WORLD: usize = TARGET_AURAS + 1 + 9;
    const WORLD_BYTES: usize = (2 + 2 * 5) + (2 + 2 * 5) + (4 + 2 + 2 + 4) + (2 + 4);
    /// The waystone sheet (#882): none unlocked and no area discovered (#983).
    const WAYSTONES: usize = WORLD + WORLD_BYTES;
    const SHEETS_END: usize = WAYSTONES + 2 + 2;
    const SUMMONED: usize = 1 + 2 * SUMMONED_RECORD_BYTES;
    const TRAPS: usize = 1 + TRAP_RECORD_BYTES;
    const EVENTS: usize = SHEETS_END + SUMMONED + TRAPS;
    /// The fixture's chat records: 6 + 16 and 6 + 8 bytes, then a 6-byte emote.
    const CHAT_BYTES: usize = 22 + 14 + 6;
    const CHAT: usize = EVENTS + 1 + EVENT_COUNT * EVENT_RECORD_BYTES;
    const ENTITY_COUNT: usize = CHAT + 1 + CHAT_BYTES;
    const FIRST_ENTITY: usize = ENTITY_COUNT + 2;
    /// The fixture's sheets: bag, quests, no corpse sheet, then the ability sheet with two
    /// cooldowns, an aura and a target aura.
    const SHEET_BYTES: usize = 8
        + INVENTORY_RECORD_BYTES
        + EQUIPMENT_RECORD_BYTES
        + 8
        + QUEST_BYTES
        + 1
        + ENTITY_REF_BYTES
        + 1
        + 2 * 6
        + 2 * (1 + 9)
        + WORLD_BYTES
        + 2
        + 2;

    #[test]
    fn player_snapshot_matches_the_shared_golden_fixture() {
        let snapshot = fixture_snapshot();
        let encoded = encode_snapshot(&snapshot).unwrap();
        assert_eq!(
            encoded.len(),
            PLAYER_SNAPSHOT_FIXED_BYTES
                + SHEET_BYTES
                + EVENT_COUNT * EVENT_RECORD_BYTES
                + CHAT_BYTES
                + 2 * SUMMONED_RECORD_BYTES
                + TRAP_RECORD_BYTES
                + 4 * ENTITY_RECORD_BYTES
                + 4 * PROJECTILE_RECORD_BYTES
        );
        assert_eq!(encoded, fixture_bytes());
        assert_eq!(
            encoded[CHAT], 3,
            "two chat lines and an emote follow the events"
        );
        assert_eq!(decode_snapshot(&encoded).unwrap(), snapshot);
        assert_eq!(encoded[CLASS], 5, "Arcanist, female");
        assert_eq!(
            encoded[RECKONING..RECKONING + 2],
            123u16.to_be_bytes(),
            "stored harm"
        );
        assert_eq!(encoded[EVENTS], 16);
        assert_eq!(
            encoded[QUESTS..LOOT],
            [
                0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 2, 3, 0, 0, 0, 2, 0, 0, 0, 1, 1, 0, 0, 0, 2, 2
            ],
            "the quest sheet follows the bag sheet"
        );
        assert_eq!(encoded[FIRST_ENTITY], 1, "the viewer's record leads");
        // Any other protocol ID is rejected, never reinterpreted.
        for at in 0..3 {
            let mut other = encoded.clone();
            other[at] ^= 1;
            assert_eq!(
                decode_snapshot(&other).unwrap_err().to_string(),
                "unsupported snapshot protocol id"
            );
        }
    }

    #[test]
    fn debris_ends_the_projection_and_round_trips() {
        let mut snapshot = fixture_snapshot();
        snapshot.debris = vec![
            mmorpg_core::DebrisView {
                id: 9,
                kind: mmorpg_core::PropKind::ExplosiveBarrel,
                piece: 7,
                position: [-300, 12, 9_000],
                rotation: [0, 23_170, 0, 23_170],
                age: 4,
            },
            mmorpg_core::DebrisView {
                id: 65_535,
                kind: mmorpg_core::PropKind::Crate,
                piece: 0,
                position: [32_767, -32_768, 0],
                rotation: [-32_767, 0, 0, 0],
                age: 255,
            },
        ];
        let encoded = encode_snapshot(&snapshot).unwrap();
        assert_eq!(
            encoded.len(),
            fixture_bytes().len() + 2 * DEBRIS_RECORD_BYTES
        );
        assert_eq!(decode_snapshot(&encoded).unwrap(), snapshot);
        let debris = encoded.len() - 2 * DEBRIS_RECORD_BYTES;
        assert_eq!(encoded[debris - 1], 2);
        for (offset, value, message) in [
            (debris + 2, 9, "unknown debris kind"),
            (debris + 3, 8, "unknown debris piece"),
            (debris - 1, 17, "snapshot has too much debris"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
    }

    /// Set-down loads (#1077) follow the debris, counted in the debris count byte's high bits, so
    /// a projection without loads keeps its bytes.
    #[test]
    fn set_down_loads_follow_the_debris_and_round_trip() {
        let mut snapshot = fixture_snapshot();
        snapshot.debris = vec![mmorpg_core::DebrisView {
            id: 9,
            kind: mmorpg_core::PropKind::Crate,
            piece: 0,
            position: [-300, 12, 9_000],
            rotation: [0, 23_170, 0, 23_170],
            age: 4,
        }];
        snapshot.loads = vec![
            mmorpg_core::LoadView {
                kind: mmorpg_core::LoadKind::Sack,
                own: true,
                at: [120, 0, -40],
                facing: 16_384,
            },
            mmorpg_core::LoadView {
                kind: mmorpg_core::LoadKind::WaterJug,
                own: false,
                at: [-6_000, 1_040, 1_100],
                facing: 0,
            },
        ];
        let encoded = encode_snapshot(&snapshot).unwrap();
        assert_eq!(
            encoded.len(),
            fixture_bytes().len() + DEBRIS_RECORD_BYTES + 2 * LOAD_RECORD_BYTES
        );
        assert_eq!(decode_snapshot(&encoded).unwrap(), snapshot);
        let loads = encoded.len() - 2 * LOAD_RECORD_BYTES;
        let counts = loads - DEBRIS_RECORD_BYTES - 1;
        assert_eq!(encoded[counts], 1 | 2 << LOAD_COUNT_SHIFT);
        assert_eq!(encoded[loads], 0x80 | mmorpg_core::LoadKind::Sack.code());
        for (offset, value, message) in [
            (loads, 6, "unknown load kind"),
            (
                counts,
                1 | 5 << LOAD_COUNT_SHIFT,
                "snapshot has too many loads",
            ),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
    }

    /// The shared loot golden projection: [`fixture_snapshot`] looking at its tapped corpse.
    pub(crate) fn loot_fixture_snapshot() -> ZoneSnapshot {
        let mut snapshot = fixture_snapshot();
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
        snapshot
    }

    #[test]
    fn complete_corpse_sheet_matches_shared_bytes_and_rejects_inconsistent_claims() {
        let snapshot = loot_fixture_snapshot();
        let bytes = encode_snapshot(&snapshot).unwrap();
        let expected = crate::fixture::golden_wire_hex(
            mmorpg_core::golden::workspace_root().join("fixtures/protocol/player-loot.hex"),
            &bytes,
        );
        assert_eq!(bytes, expected);
        assert_eq!(decode_snapshot(&bytes).unwrap(), snapshot);
        let presence = LOOT;
        for length in 0..bytes.len() {
            assert!(decode_snapshot(&bytes[..length]).is_err());
        }
        for (offset, value) in [
            (presence, 2),
            (presence + 4, 109),
            (presence + 12, 100),
            (presence + 17, 2),
            (presence + 21, 9),
            (presence + 23, 0),
            (presence + 24, 2),
            (presence + 28, 99),
            (presence + 30, 0),
            // A quest drop of two units.
            (presence + 30, 2),
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
            (0, encoded[0] ^ 1, "unsupported snapshot protocol id"),
            (3, 1, "unexpected snapshot scope"),
            (53, 0b11, "viewer state is inconsistent"),
            (54, 9, "unknown entity kind"),
            (CLASS, 15, "viewer ability state is inconsistent"),
            (CLASS, 1, "viewer resource is inconsistent"),
            (CLASS + 1, 1, "viewer resource is inconsistent"),
            (CLASS + 4, 0xff, "viewer resource is inconsistent"),
            (
                CLASS + 7,
                u8::try_from(GLOBAL_COOLDOWN_TICKS + 1).unwrap(),
                "viewer ability state is inconsistent",
            ),
            (CAST, 1, "cast state is inconsistent"),
            (CAST + 1, 1, "cast state is inconsistent"),
            (CAST + 4, 2, "reserved flag bits are set"),
            // Elapsed reaches the 2 s total.
            (
                CAST + 6,
                u8::try_from(mmorpg_core::ticks_from_ms(2_000)).unwrap(),
                "cast state is inconsistent",
            ),
            (DAMAGE + 1, 13, "viewer damage range is inverted"),
            (
                COOLDOWNS,
                MAX_COOLDOWNS as u8 + 1,
                "snapshot has too many cooldowns",
            ),
            (COOLDOWNS + 4, 11, "cooldown record is inconsistent"),
            (COOLDOWNS + 5, 9, "cooldown record is inconsistent"),
            (AURAS, 9, "snapshot has too many auras"),
            (AURAS + 4, 9, "aura record is inconsistent"),
            (AURAS + 5, 4, "aura record is inconsistent"),
            (AURAS + 6, 9, "aura record is inconsistent"),
            (TARGET_OF_TARGET, 0, "an absent entity must have ID 0"),
            (TARGET_CAST + 3, 0, "cast state is inconsistent"),
            (TARGET_AURAS + 7, 0, "aura record is inconsistent"),
            (EVENTS, 17, "snapshot exceeds the per-player event capacity"),
            (EVENTS + 1, 255, "unknown event kind"),
            (EVENTS + 5, 2, "event flags are invalid"),
            (
                EVENTS + 1 + 2 * EVENT_RECORD_BYTES + 4,
                1,
                "event flags are invalid",
            ),
            (
                EVENTS + 1 + 5 * EVENT_RECORD_BYTES + 16,
                99,
                "unknown error code",
            ),
            (
                EVENTS + 1 + 6 * EVENT_RECORD_BYTES + 16,
                3,
                "unknown strike aim",
            ),
            (
                EVENTS + 1 + 7 * EVENT_RECORD_BYTES + 4,
                mmorpg_core::ABILITY_CATALOG.len() as u8 + 1,
                "unknown ability",
            ),
            (
                EVENTS + 1 + 7 * EVENT_RECORD_BYTES + 4,
                0,
                "unknown ability",
            ),
            (
                EVENTS + 1 + 7 * EVENT_RECORD_BYTES,
                255,
                "unknown event kind",
            ),
            (
                EVENTS + 1 + 8 * EVENT_RECORD_BYTES + 16,
                1,
                "malformed event record",
            ),
            (
                EVENTS + 1 + 13 * EVENT_RECORD_BYTES + 4,
                1,
                "event flags are invalid",
            ),
            (
                EVENTS + 1 + 14 * EVENT_RECORD_BYTES + 4,
                0,
                "quest id is out of range",
            ),
            (
                EVENTS + 1 + 14 * EVENT_RECORD_BYTES + 15,
                3,
                "malformed event record",
            ),
            (
                EVENTS + 1 + 14 * EVENT_RECORD_BYTES + 16,
                0,
                "malformed event record",
            ),
            (
                EVENTS + 1 + 15 * EVENT_RECORD_BYTES + 4,
                0,
                "quest id is out of range",
            ),
            (
                EVENTS + 1 + 15 * EVENT_RECORD_BYTES + 5,
                1,
                "malformed event record",
            ),
            (QUESTS + 5, 0, "quest id is out of range"),
            (QUESTS + 6, 11, "quest log exceeds its capacity"),
            (QUESTS + 10, 1, "an active quest is also turned in"),
            (QUESTS + 20, 4, "unknown quest marker"),
            (QUESTS + 24, 1, "quest markers are not ordered"),
            (FIRST_ENTITY, 0, "unknown entity kind"),
            (FIRST_ENTITY, 5, "unknown entity kind"),
            (wolf_record + 27, 101, "health percent exceeds 100"),
            (
                wolf_record + 28,
                0x80,
                "lootable entity flags are inconsistent",
            ),
            (31, 9, "player projection must start with the viewer"),
            // The fixture's Arcanist has no melee stance.
            (STANCE, 2, "viewer melee stance is invalid"),
            (COMBO, 2, "viewer combo phase is invalid"),
            (MASTERY, 9, "viewer weapon mastery is out of range"),
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
        swapped[FIRST_ENTITY..FIRST_ENTITY + 4 * ENTITY_RECORD_BYTES]
            .rotate_left(ENTITY_RECORD_BYTES);
        assert_eq!(
            decode_snapshot(&swapped).unwrap_err().to_string(),
            "player projection must start with the viewer"
        );
        let mut fewer = encoded.clone();
        fewer[ENTITY_COUNT..FIRST_ENTITY].copy_from_slice(&3_u16.to_be_bytes());
        assert!(decode_snapshot(&fewer).is_err(), "count must match records");
        let mut more = encoded.clone();
        more[ENTITY_COUNT..FIRST_ENTITY].copy_from_slice(&5_u16.to_be_bytes());
        assert!(decode_snapshot(&more).is_err(), "count must match records");
        // The four projectile records (arrow, bolt, missile, bursting bomb) end the projection
        // (#286, #289, #290). The empty debris section (#288) follows them.
        let projectiles = encoded.len() - 1 - 4 * PROJECTILE_RECORD_BYTES;
        assert_eq!(encoded[projectiles - 1], 4);
        for (offset, value, message) in [
            (projectiles + 2, 10, "unknown projectile kind"),
            (projectiles + 19, 3, "unknown projectile state"),
            (projectiles - 1, 9, "snapshot has too many projectiles"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset] = value;
            assert_eq!(
                decode_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "byte {offset} = {value}"
            );
        }
    }

    #[test]
    fn progression_decoder_rejects_invalid_self_state() {
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
        for (slot, item, quantity) in [
            (0, 0_u32, 3_u16),
            (0, 99, 1),
            (0, 1, 0),
            (0, 1, 21),
            (1, 2, 2),
        ] {
            let mut invalid = encoded.clone();
            let offset = INVENTORY + 8 + slot * 6;
            invalid[offset..offset + 4].copy_from_slice(&item.to_be_bytes());
            invalid[offset + 4..offset + 6].copy_from_slice(&quantity.to_be_bytes());
            assert!(decode_snapshot(&invalid).is_err());
        }
        let mut omitted = fixture_snapshot();
        omitted.inventory = None;
        assert_eq!(
            encode_snapshot(&omitted).unwrap_err().to_string(),
            "the bag sheet needs both the bag and the equipment"
        );
        // A projection without a bag or quests to send carries neither sheet.
        omitted.equipment = None;
        omitted.quests = None;
        omitted.inventory_revision = 0;
        assert_eq!(
            decode_snapshot(&encode_snapshot(&omitted).unwrap()).unwrap(),
            omitted
        );
    }

    /// A client that acknowledged a sheet stops receiving it; its decoder keeps the copy, with
    /// timers running down, until the sheet changes (#544).
    #[test]
    fn acknowledged_sheets_stay_home_and_the_decoder_keeps_them() {
        let snapshot = fixture_snapshot();
        let mut decoder = SnapshotDecoder::new();
        assert_eq!(decoder.revisions(), SheetRevisions::default());
        let full = pack_snapshot(&snapshot).unwrap();
        assert_eq!(sheet_mask(&full.payload), 0b1100_1111);
        assert_eq!(decoder.decode(&full.payload).unwrap(), snapshot);
        let held = decoder.revisions();
        assert!(
            Sheet::ALL
                .into_iter()
                .filter(|sheet| *sheet != Sheet::Party && *sheet != Sheet::DepthsLoot)
                .all(|sheet| held.get(sheet) != 0)
        );

        // Ten ticks on, nothing changed: no sheet travels, and the timers ran down.
        let mut later = snapshot.clone();
        later.tick += 10;
        for cooldown in &mut later.cooldowns {
            cooldown.remaining -= 10;
        }
        for aura in later.auras.iter_mut().chain(&mut later.target_detail.auras) {
            aura.remaining -= 10;
        }
        let quiet = pack_snapshot_acknowledged(&later, held).unwrap();
        assert_eq!(sheet_mask(&quiet.payload), 0);
        assert_eq!(full.payload.len() - quiet.payload.len(), SHEET_BYTES);
        assert_eq!(decoder.decode(&quiet.payload).unwrap(), later);
        assert_eq!(decoder.revisions(), held);
        // A decoder that never saw the sheets has none.
        let bare = decode_snapshot(&quiet.payload).unwrap();
        assert_eq!(
            (bare.inventory, bare.quests, bare.inventory_revision),
            (None, None, 0)
        );
        assert!(bare.cooldowns.is_empty() && bare.auras.is_empty());

        // A changed bag travels alone; the others stay acknowledged.
        let mut changed = later.clone();
        changed.inventory_revision += 1;
        changed.inventory.as_mut().unwrap().remove(0, 1).unwrap();
        let update = pack_snapshot_acknowledged(&changed, held).unwrap();
        assert_eq!(sheet_mask(&update.payload), 0b0001);
        assert_eq!(decoder.decode(&update.payload).unwrap(), changed);
        assert_ne!(
            decoder.revisions().get(Sheet::Inventory),
            held.get(Sheet::Inventory)
        );
        assert_eq!(
            decoder.revisions().get(Sheet::Quests),
            held.get(Sheet::Quests)
        );

        // `snapshot` without any sheet, as to a client that holds all of its sheets.
        let sheetless = |snapshot: &ZoneSnapshot| {
            let mut own = SnapshotDecoder::new();
            own.decode(&pack_snapshot(snapshot).unwrap().payload)
                .unwrap();
            let packed = pack_snapshot_acknowledged(snapshot, own.revisions()).unwrap();
            assert_eq!(sheet_mask(&packed.payload), 0);
            packed.payload
        };

        // A held timer that ran out is gone; the others keep running down.
        let shortest = changed
            .cooldowns
            .iter()
            .map(|cooldown| cooldown.remaining)
            .min()
            .unwrap();
        let mut expired = changed.clone();
        expired.tick += u64::from(shortest);
        expired.cooldowns.retain_mut(|cooldown| {
            cooldown.remaining -= shortest;
            cooldown.remaining > 0
        });
        for auras in [&mut expired.auras, &mut expired.target_detail.auras] {
            auras.retain_mut(|aura| {
                aura.remaining = aura.remaining.saturating_sub(shortest);
                aura.remaining > 0
            });
        }
        assert_eq!(expired.cooldowns.len(), changed.cooldowns.len() - 1);
        assert_eq!(
            decoder.clone().decode(&sheetless(&expired)).unwrap(),
            expired
        );

        // A held ability sheet of another target keeps only the viewer's own lists until the
        // new one arrives; a corpse sheet of a creature no longer selected is dropped.
        let mut retargeted = changed.clone();
        retargeted.viewer.target = None;
        retargeted.target_of_target = None;
        retargeted.target_detail = TargetDetail::default();
        let stale = decoder.clone().decode(&sheetless(&retargeted)).unwrap();
        assert!(stale.target_detail.auras.is_empty());
        assert_eq!(stale, retargeted);
    }

    #[test]
    fn acknowledged_commands_carry_the_held_revisions() {
        // The party sheet is not held, so its revision stays out of the held mask (#897).
        let mut revisions = SheetRevisions::default();
        for (sheet, revision) in Sheet::ALL
            .into_iter()
            .zip([1, 2, 3, 0xdead_beef, 0, 6, 7, 8])
        {
            revisions.set(sheet, revision);
        }
        let command = mmorpg_core::ZoneCommand::Jump;
        for inner in [
            crate::encode_command(command),
            crate::encode_observed_command(command, 41),
        ] {
            let wrapped = crate::encode_acknowledged_command(revisions, &inner);
            assert_eq!(
                wrapped.len(),
                inner.len() + 1 + crate::SHEET_REVISIONS_BYTES - 4
            );
            let envelope = crate::decode_command_envelope(&wrapped).unwrap();
            assert_eq!(envelope.command, command);
            assert_eq!(envelope.sheets, Some(revisions));
            assert_eq!(
                (envelope.command, envelope.observed),
                crate::decode_observed_command(&inner).unwrap()
            );
            assert!(crate::decode_observed_command(&wrapped).is_err());
            let twice = crate::encode_acknowledged_command(revisions, &wrapped);
            assert!(crate::decode_command_envelope(&twice).is_err());
            assert!(crate::decode_command_envelope(&wrapped[..wrapped.len() - 1]).is_err());
            let mut unknown = wrapped.clone();
            unknown[2] |= 0x80;
            assert!(crate::decode_command_envelope(&unknown).is_err());
        }
        assert_eq!(
            crate::decode_command_envelope(&crate::encode_command(command))
                .unwrap()
                .sheets,
            None
        );
    }

    #[test]
    fn equipment_decoder_rejects_unknown_misplaced_items_and_wrong_totals() {
        let encoded = fixture_bytes();
        assert_eq!(encoded[EQUIPMENT..EQUIPMENT + 4], [0, 0, 0, 4], "the wand");
        // Slot 0 holds the wand; slot 1 (off hand) is empty.
        for (offset, item, message) in [
            (EQUIPMENT, 99_u32, "unknown item"),
            (EQUIPMENT, 1, "item cannot be equipped there"),
            (EQUIPMENT + 4, 6, "item cannot be equipped there"),
        ] {
            let mut invalid = encoded.clone();
            invalid[offset..offset + 4].copy_from_slice(&item.to_be_bytes());
            assert_eq!(
                decode_snapshot(&invalid).unwrap_err().to_string(),
                message,
                "offset {offset} = item {item}"
            );
        }
        // Removing the wand leaves the intellect total stale.
        let mut stale = encoded.clone();
        stale[EQUIPMENT..EQUIPMENT + 4].fill(0);
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
            (117, 17, 30)
        );
        assert_eq!(MAX_WIRE_ENTITIES, 32);
        let viewer_id = u32::MAX;
        let target = EntityRef::Creature(CreatureId::new(u32::MAX));
        let entities: Vec<_> = (0..MAX_VISIBLE_ENTITIES)
            .map(|index| {
                let (low, high) = if index % 2 == 0 {
                    (i32::MIN, i32::MAX)
                } else {
                    (i32::MAX, i32::MIN)
                };
                let (kind, id) = match index {
                    0 => (EntityKind::Player, viewer_id),
                    1 => (EntityKind::Creature, u32::MAX),
                    _ => (EntityKind::Npc, u32::try_from(index).unwrap()),
                };
                EntitySnapshot {
                    kind,
                    id,
                    // A player appearance reserves its high bit for weapon mode (#802).
                    appearance: if kind == EntityKind::Player {
                        1
                    } else {
                        u32::MAX
                    },
                    position: [low, high, low],
                    velocity: [i8::MIN, i8::MAX, i8::MIN],
                    facing: u16::MAX,
                    level: u8::MAX,
                    health_percent: 100,
                    flags: EntityFlags {
                        lootable: false,
                        dead: false,
                        in_combat: true,
                        weapon_drawn: false,
                        weapon: None,
                        named_weapon: None,
                        great_weapon: None,
                        shapeshifted: false,
                        load: None,
                        hostile: true,
                        attackable: true,
                        tapped_by_other: kind == EntityKind::Creature,
                        evading: kind == EntityKind::Creature,
                        targets_viewer: true,
                        posture: mmorpg_core::PostureView::Free,
                        traversal: if kind == EntityKind::Player {
                            mmorpg_core::TraversalView::Rolling
                        } else {
                            mmorpg_core::TraversalView::Free
                        },
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
            party: None,
            depths_loot: None,
            raid: Some(mmorpg_core::RaidView {
                phase: 2,
                integrity: 63,
                aid: BTreeSet::new(),
                signs: false,
                warned: true,
                braced: BTreeSet::from([0, 1, 2, 3]),
                scouted: false,
                standing: None,
            }),
            projectiles: Vec::new(),
            debris: Vec::new(),
            loads: Vec::new(),
            doors: vec![
                mmorpg_core::DoorView {
                    door: mmorpg_core::DoorId::new(1),
                    open: true,
                };
                9
            ]
            .into_iter()
            .enumerate()
            .map(|(index, view)| mmorpg_core::DoorView {
                door: mmorpg_core::DoorId::new(u32::try_from(index + 1).unwrap()),
                ..view
            })
            .collect(),
            chat: Vec::new(),
            loot: None,
            content_revision: u64::MAX,
            acknowledged_sequence: u32::MAX,
            viewer_id,
            inventory_revision: 1,
            inventory: None,
            equipment: None,
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
                weapon_drawn: false,
                target: Some(target),
                class: None,
                resource: None,
                cast: None,
                global_cooldown: 0,
                global_cooldown_total: 0,
                damage: [u16::MAX; 2],
                guard: GuardView::default(),
                stance: None,
                combo: None,
                breath: mmorpg_core::water::BREATH_MAX_TICKS,
                reckoning: 0,
                mastery: mmorpg_core::Mastery::default(),
                waystones: mmorpg_core::WaystoneSet::EMPTY,
                discoveries: std::collections::BTreeSet::new(),
            },
            cooldowns: Vec::new(),
            auras: Vec::new(),
            target_of_target: Some(EntityRef::Npc(NpcId::new(u32::MAX))),
            target_detail: TargetDetail::default(),
            events: vec![event; MAX_EVENTS_PER_PLAYER],
            entities,
            quests: None,
            pieces: Vec::new(),
            depleted_veins: Vec::new(),
            summoned: Vec::new(),
            traps: Vec::new(),
        };
        let packed = pack_snapshot(&snapshot).unwrap();
        // (1,077 − 114 − 9 − 73 − 2 − 16 × 17) / 29 = 20 records fit beside the empty loot and
        // ability sheets, the world sheet with nine doors and a raid signal (#882), the empty
        // waystone sheet and a full event section.
        assert_eq!(packed.packed_entities, 20);
        assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
        assert!(
            packed.payload.len() + SESSION_SNAPSHOT_HEADER_BYTES + DATAGRAM_SAFETY_MARGIN_BYTES
                <= MEASURED_MIN_DATAGRAM_BYTES
        );
        let decoded = decode_snapshot(&packed.payload).unwrap();
        assert_eq!(decoded.entities[..], snapshot.entities[..20]);
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
        // Without events, doors, a raid signal or a bag sheet the budget holds 31 records beside
        // the empty loot (1 byte), ability (8), world (7) and waystone (2) sheets, and all 32
        // once the client holds those sheets.
        let quiet = ZoneSnapshot {
            events: Vec::new(),
            doors: Vec::new(),
            raid: None,
            ..snapshot
        };
        let packed = pack_snapshot(&quiet).unwrap();
        assert_eq!(packed.packed_entities, 31);
        assert_eq!(
            packed.payload.len(),
            PLAYER_SNAPSHOT_FIXED_BYTES + 20 + 31 * ENTITY_RECORD_BYTES
        );
        let mut holder = SnapshotDecoder::new();
        holder.decode(&packed.payload).unwrap();
        let held = pack_snapshot_acknowledged(&quiet, holder.revisions()).unwrap();
        assert_eq!(held.packed_entities, MAX_WIRE_ENTITIES);
        assert_eq!(
            held.payload.len(),
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
            // The full bag and quest sheets (136 + 203 bytes beside the other 16) leave room for
            // 20 whole records, and for 10 beside a full event section.
            if events == 0 {
                assert_eq!(packed.packed_entities, 20);
            }
            if events == 16 {
                assert_eq!(packed.packed_entities, 10);
            }
        }
        // Whole records only, within the budget.
        assert!(maximum_bytes <= 1_077);
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
        // (1,077 − 117 − 16 − 272 − 4 × 84) / 30 = 11 records.
        assert_eq!(packed.packed_entities, 11);
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
                // The bag, quest and corpse sheets beside the other 16 bytes and a full event
                // section leave room for 9 whole records.
                if events == 16 && item.is_some() {
                    assert_eq!(packed.packed_entities, 9);
                }
            }
        }
        // With a corpse sheet, whole records fill the budget to its last record.
        assert!(maximum_bytes > 1_077 - ENTITY_RECORD_BYTES && maximum_bytes <= 1_077);
        // Full cooldown and aura lists for the viewer and its target, both
        // sheets and every event still leave the viewer and its target.
        let aura = |ability: u32| {
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
        loaded.viewer.global_cooldown_total = GLOBAL_COOLDOWN_TICKS;
        loaded.viewer.cast = Some(CastView {
            ability: AbilityId::new(12),
            elapsed: mmorpg_core::golden::t30(179) as _,
            total: mmorpg_core::golden::t30(180) as _,
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
                elapsed: mmorpg_core::golden::t30(89) as _,
                total: mmorpg_core::golden::t30(90) as _,
                channel: false,
            }),
            auras: full_auras,
        };
        let packed = pack_snapshot(&loaded).unwrap();
        // The full ability sheet is 176 bytes beside the bag, quest and other sheets' 347:
        // (1,077 − 111 − 523 − 16 × 17) / 29 = 5 records.
        assert_eq!(packed.packed_entities, 5);
        assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
        let decoded = decode_snapshot(&packed.payload).unwrap();
        assert_eq!(decoded.entities[1].entity(), target);
        assert_eq!(
            (decoded.cooldowns, decoded.auras, decoded.target_detail),
            (loaded.cooldowns, loaded.auras, loaded.target_detail)
        );
    }

    #[test]
    fn snapshot_encoding_fails_closed_on_missing_viewer_or_excess_events() {
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
        // Positions beyond the old `i16` range travel whole (#424).
        for component in [i32::from(i16::MAX) + 1, i32::from(i16::MIN) - 1] {
            let mut wide = fixture_snapshot();
            wide.entities[1].position[2] = component;
            let decoded = decode_snapshot(&encode_snapshot(&wide).unwrap()).unwrap();
            assert_eq!(decoded.entities[1].position[2], component);
        }
        let mut chatty = fixture_snapshot();
        chatty.events = vec![chatty.events[0]; MAX_EVENTS_PER_PLAYER + 1];
        assert_eq!(
            encode_snapshot(&chatty).unwrap_err().to_string(),
            "player projection has too many events"
        );
    }
}

#[cfg(test)]
mod posture_tests {
    use mmorpg_core::{EntityFlags, EntityKind, EntitySnapshot, PostureView};

    use super::{decode_snapshot, encode_snapshot, tests::fixture_snapshot};
    use crate::codec::Codec;

    fn record(kind: EntityKind, posture: PostureView) -> EntitySnapshot {
        EntitySnapshot {
            kind,
            id: 7,
            appearance: 0,
            position: [1, 2, 3],
            velocity: [0; 3],
            facing: 9,
            level: 1,
            health_percent: 100,
            flags: EntityFlags {
                posture,
                ..EntityFlags::default()
            },
        }
    }

    fn round_trip(entity: &EntitySnapshot) -> Result<EntitySnapshot, crate::ProtocolError> {
        let mut bytes = Vec::new();
        entity.encode(&mut bytes)?;
        EntitySnapshot::decode(&bytes, &mut 0)
    }

    #[test]
    fn player_weapon_mode_survives_packed_projection_without_changing_threat() {
        let mut snapshot = fixture_snapshot();
        snapshot.viewer.weapon_drawn = true;
        snapshot.entities[0].flags.weapon_drawn = true;
        snapshot.viewer.in_combat = false;
        snapshot.entities[0].flags.in_combat = false;
        let received = decode_snapshot(&encode_snapshot(&snapshot).unwrap()).unwrap();
        assert!(received.viewer.weapon_drawn);
        assert!(!received.viewer.in_combat);
        assert!(received.entities[0].flags.weapon_drawn);
        assert!(!received.entities[0].flags.in_combat);
        assert!(
            !round_trip(&record(EntityKind::Creature, PostureView::Free))
                .unwrap()
                .flags
                .weapon_drawn
        );
    }

    #[test]
    fn a_player_weapon_family_crosses_the_wire_beside_its_class() {
        for family in mmorpg_core::WeaponFamily::ALL {
            let mut player = record(EntityKind::Player, PostureView::Free);
            player.flags.weapon_drawn = true;
            player.flags.weapon = Some(family);
            player.flags.named_weapon = Some(mmorpg_core::NamedWeapon::GarricksCleaver);
            assert_eq!(round_trip(&player).unwrap(), player);
            player.flags.named_weapon = None;
            for great in (0..4).map(mmorpg_core::GreatWeapon::from_code) {
                player.flags.great_weapon = Some(great.unwrap());
                assert_eq!(round_trip(&player).unwrap(), player);
            }
        }
        let mut creature = record(EntityKind::Creature, PostureView::Free);
        creature.flags.weapon = Some(mmorpg_core::WeaponFamily::Spear);
        assert!(round_trip(&creature).is_err());
    }

    #[test]
    fn a_sitting_player_and_every_npc_posture_cross_the_wire() {
        let sitter = record(EntityKind::Player, PostureView::Sitting);
        assert_eq!(round_trip(&sitter).unwrap(), sitter);
        for posture in [
            PostureView::Free,
            PostureView::Sitting,
            PostureView::Asleep,
            PostureView::Waking,
            PostureView::Hunting,
            PostureView::Carrying,
            PostureView::Hurt,
        ] {
            let npc = record(EntityKind::Npc, posture);
            assert_eq!(round_trip(&npc).unwrap(), npc);
        }
        // A player is never in bed, and a creature never sits.
        assert!(round_trip(&record(EntityKind::Player, PostureView::Asleep)).is_err());
        assert!(round_trip(&record(EntityKind::Creature, PostureView::Sitting)).is_err());
    }
}
