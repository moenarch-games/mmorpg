//! The durable character record (#30): the player facts that outlive a
//! session and a zone, captured from and admitted into a zone simulation.
//!
//! A zone keys live players by session [`PlayerId`]; a [`CharacterId`] names
//! the durable character independently of any session, process or zone. The
//! record holds only durable facts. Session and tick-local state (held
//! movement, queued intents, events, chat, target, casts, auras, guard and the
//! zone-local inventory revision) is not part of it and starts fresh on
//! admission. Health, the class resource and running cooldowns are kept, so
//! leaving and re-entering neither heals nor resets anything.
//!
//! Capture and admission are pure and deterministic: admitting a captured
//! record and capturing it again yields the same record. Who may hold a
//! character live is decided by the character store's lease, not here.

use std::fmt;

use crate::physics::{RigidBody, Vec3i};

use crate::entity::body_id;
use crate::snapshot::{CanonicalPlayerAbilities, validate_player_abilities};
use crate::unit::player_max_health;
use crate::zone::{PLAYER_HALF_EXTENTS, PlayerState, vector, within_world_limits};
use crate::{
    ClassChoice, Cooldown, EntityRef, Equipment, Inventory, MAX_PLAYERS_PER_ZONE, PlayerId,
    QuestLog, ZoneError, ZoneId, ZoneSimulation,
};

/// Version of the [`CharacterRecord`] schema. Bump it with every change to
/// the record's fields or their meaning; persisted records of another version
/// are rejected rather than reinterpreted.
pub const CHARACTER_RECORD_SCHEMA_VERSION: u16 = 1;

/// Longest [`CharacterId`] in bytes.
pub const MAX_CHARACTER_ID_BYTES: usize = 64;

/// Stable identity of a durable character: 1–64 bytes of lowercase ASCII
/// letters, digits and `-`, starting with a letter or digit. It is safe as a
/// file name and a storage key, and never derived from a session.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CharacterId(String);

impl CharacterId {
    pub fn new(value: impl Into<String>) -> Result<Self, ZoneError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_CHARACTER_ID_BYTES
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !value.starts_with('-');
        if valid {
            Ok(Self(value))
        } else {
            Err(ZoneError::new("character ID is invalid"))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CharacterId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Where a character was when its record was captured: the zone and the
/// body centre and facing in that zone's coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterLocation {
    pub zone_id: ZoneId,
    pub position: [i32; 3],
    pub facing: u16,
}

/// The durable facts of one character.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterRecord {
    pub character_id: CharacterId,
    /// Class and sex: chosen once, at creation.
    pub class: ClassChoice,
    pub level: u8,
    pub experience: u32,
    pub copper: u32,
    pub inventory: Inventory,
    pub equipment: Equipment,
    pub quests: QuestLog,
    /// Zero means dead (a corpse or a released spirit).
    pub health: u32,
    pub resource: u16,
    pub resource_ticks: u16,
    pub mana_delay: u16,
    pub global_cooldown: u16,
    /// Running cooldowns in ability order.
    pub cooldowns: Vec<Cooldown>,
    pub location: CharacterLocation,
}

impl ZoneSimulation {
    /// The durable record of an admitted player who has chosen a class.
    pub fn capture_character(
        &self,
        player_id: PlayerId,
        character_id: CharacterId,
    ) -> Result<CharacterRecord, ZoneError> {
        let player = self
            .players
            .get(&player_id)
            .ok_or_else(|| ZoneError::new("unknown player"))?;
        let class = player
            .class
            .ok_or_else(|| ZoneError::new("a character record needs a chosen class"))?;
        let position = self.player_position(player_id)?;
        Ok(CharacterRecord {
            character_id,
            class,
            level: player.level,
            experience: player.experience,
            copper: player.copper,
            inventory: player.inventory.clone(),
            equipment: player.equipment,
            quests: player.quests.clone(),
            health: player.health,
            resource: player.resource.value,
            resource_ticks: player.resource.ticks,
            mana_delay: player.resource.delay,
            global_cooldown: player.global_cooldown,
            cooldowns: player.cooldowns.clone(),
            location: CharacterLocation {
                zone_id: self.zone_id,
                position: [position.x, position.y, position.z],
                facing: player.facing,
            },
        })
    }

    /// Admits `player_id` as a newly created character of `class`: a fresh
    /// player at a spawn slot that has already made its one class choice, so
    /// it can be captured at once.
    pub fn create_character(
        &mut self,
        player_id: PlayerId,
        class: ClassChoice,
    ) -> Result<(), ZoneError> {
        self.add_player(player_id)?;
        if self
            .choose_class(player_id, class.class.code(), class.sex.code())
            .is_some()
        {
            self.remove_player(player_id);
            return Err(ZoneError::new("the zone content offers no classes"));
        }
        Ok(())
    }

    /// Admits `player_id` as the character `record` describes. A record
    /// captured in this zone resumes at its position; one from another zone
    /// enters at a free spawn slot. The record is validated against this
    /// zone's content first, and a refused record changes nothing.
    pub fn admit_character(
        &mut self,
        player_id: PlayerId,
        record: &CharacterRecord,
    ) -> Result<(), ZoneError> {
        if self.players.contains_key(&player_id) {
            return Err(ZoneError::new("player already exists in zone"));
        }
        if self.players.len() >= MAX_PLAYERS_PER_ZONE {
            return Err(ZoneError::new("zone player capacity reached"));
        }
        self.validate_character(record)?;
        let spawn_slot = self
            .available_spawn_slot()
            .ok_or_else(|| ZoneError::new("zone spawn capacity reached"))?;
        let position = if record.location.zone_id == self.zone_id {
            vector(record.location.position)
        } else {
            self.spawn_position(spawn_slot)?
        };
        let entity = EntityRef::Player(player_id);
        self.world
            .add_body(RigidBody::dynamic(
                body_id(entity),
                position,
                Vec3i::ZERO,
                PLAYER_HALF_EXTENTS,
            ))
            .map_err(crate::zone::physics_error)?;
        self.interest.insert(entity, position.x, position.z);
        self.interest_work.bucket_inserts += 1;
        let mut state = PlayerState::new(spawn_slot, self.tick);
        state.facing = if record.location.zone_id == self.zone_id {
            record.location.facing
        } else {
            0
        };
        state.class = Some(record.class);
        state.level = record.level;
        state.experience = record.experience;
        state.copper = record.copper;
        state.inventory = record.inventory.clone();
        state.equipment = record.equipment;
        state.quests = record.quests.clone();
        state.health = record.health;
        state.resource.value = record.resource;
        state.resource.ticks = record.resource_ticks;
        state.resource.delay = record.mana_delay;
        state.global_cooldown = record.global_cooldown;
        state.cooldowns = record.cooldowns.clone();
        self.players.insert(player_id, state);
        Ok(())
    }

    fn validate_character(&self, record: &CharacterRecord) -> Result<(), ZoneError> {
        if self.content.ability_revision() == 0 {
            return Err(ZoneError::new(
                "a class player needs content that binds the ability catalog",
            ));
        }
        let max_health = player_max_health(record.level) + record.equipment.totals().bonus_health();
        if !crate::progression::valid_progression(record.level, record.experience)
            || record.health > max_health
        {
            return Err(ZoneError::new("character unit state is out of range"));
        }
        crate::quest::validate_log(&self.content, &record.quests)?;
        let alive = record.health > 0;
        validate_player_abilities(
            &CanonicalPlayerAbilities {
                class: Some(record.class),
                resource: record.resource,
                resource_ticks: record.resource_ticks,
                mana_delay: record.mana_delay,
                global_cooldown: record.global_cooldown,
                cooldowns: record.cooldowns.clone(),
                ..CanonicalPlayerAbilities::default()
            },
            record.level,
            alive,
        )?;
        if record.location.zone_id == self.zone_id
            && !within_world_limits(vector(record.location.position))
        {
            return Err(ZoneError::new("character position is out of range"));
        }
        Ok(())
    }
}
