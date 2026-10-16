//! Creature state: life cycle, threat table and spawning. Decisions live in
//! `ai`, combat in `combat`.

use std::sync::Arc;

use physics_engine::{RigidBody, Vec3i};

use crate::entity::body_id;
use crate::snapshot::{CreatureAi, ThreatEntry};
use crate::zone::{physics_error, vector};
use crate::{CreatureId, CreatureSpawn, CreatureTemplate, EntityRef, ZoneError, ZoneSimulation};

/// Creatures chase at 19 units per tick (5.7 m/s), slower than a running player.
pub const CREATURE_RUN_SPEED_UNITS_PER_TICK: i32 = 19;
/// Wandering creatures walk at 40 % of their run speed, rounded.
pub const CREATURE_WALK_SPEED_UNITS_PER_TICK: i32 = 8;
/// Evading creatures run home at 1.25 × their run speed, rounded up.
pub const CREATURE_EVADE_SPEED_UNITS_PER_TICK: i32 = 24;
/// Idle creatures wait 5–15 s between wander walks.
pub const WANDER_WAIT_TICKS: [u16; 2] = [150, 450];
/// A wander walk that has not arrived after 10 s gives up.
pub const WANDER_WALK_TIMEOUT_TICKS: u16 = 300;
/// Aggro radius against a player of the same level (10 m) …
pub const AGGRO_RADIUS_UNITS: i32 = 1_000;
/// … changed by 1 m per level the creature is above (or below) the player …
pub const AGGRO_PER_LEVEL_UNITS: i32 = 100;
/// … and clamped to 5–20 m.
pub const AGGRO_MIN_RADIUS_UNITS: i32 = 500;
pub const AGGRO_MAX_RADIUS_UNITS: i32 = 2_000;
/// Idle creatures of the same family this close to an engaging one assist.
pub const ASSIST_RADIUS_UNITS: i32 = 800;
/// Beyond this XZ distance from its spawn point a creature evades.
pub const LEASH_RADIUS_UNITS: i32 = 4_000;
/// An evading creature still away from home after 10 s is moved home.
pub const EVADE_TIMEOUT_TICKS: u16 = 300;
/// A creature within this XZ distance of its destination has arrived.
pub const ARRIVAL_RADIUS_UNITS: i32 = 20;
/// Threat table capacity; later attackers are not added when it is full.
pub const MAX_THREAT_ENTRIES: usize = 64;

/// Where a creature is in its life cycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Life {
    /// Present with a physics body.
    Alive,
    /// A visible corpse without a body until `CORPSE_TICKS` after death.
    Corpse { died_at: u64, position: [i32; 3] },
    /// Gone until its template's respawn time after death.
    Despawned { died_at: u64 },
}

/// Authoritative state of one creature; template and spawn come from content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CreatureState {
    pub(crate) level: u8,
    pub(crate) health: u32,
    pub(crate) facing: u16,
    pub(crate) life: Life,
    pub(crate) ai: CreatureAi,
    /// Ordered by entity; at most [`MAX_THREAT_ENTRIES`].
    pub(crate) threat: Vec<ThreatEntry>,
    pub(crate) swing_timer: u16,
    pub(crate) combat_timer: u16,
    /// The first player who damaged it since it last reset.
    pub(crate) tapped_by: Option<crate::PlayerId>,
    pub(crate) loot: Option<crate::LootRewards>,
    /// Ticks until its content-bound ability may be used again.
    pub(crate) ability_timer: u16,
    pub(crate) cast: Option<crate::CastState>,
    /// Auras in slot order.
    pub(crate) auras: Vec<crate::Aura>,
}

impl CreatureState {
    pub(crate) const fn is_alive(&self) -> bool {
        matches!(self.life, Life::Alive)
    }

    /// The unit that currently leads the threat table: highest threat, ties
    /// to the lowest `EntityRef`.
    pub(crate) fn top_threat(&self) -> Option<EntityRef> {
        top_threat(&self.threat)
    }

    /// Adds `amount` threat for `entity`, inserting it when there is room.
    pub(crate) fn add_threat(&mut self, entity: EntityRef, amount: u32) {
        match self
            .threat
            .binary_search_by_key(&entity, |entry| entry.entity)
        {
            Ok(index) => {
                self.threat[index].threat = self.threat[index].threat.saturating_add(amount);
            }
            Err(index) if self.threat.len() < MAX_THREAT_ENTRIES => {
                self.threat.insert(
                    index,
                    ThreatEntry {
                        entity,
                        threat: amount,
                    },
                );
            }
            Err(_) => {}
        }
    }

    /// Removes a unit from the threat table (death, leaving the zone).
    pub(crate) fn forget(&mut self, entity: EntityRef) {
        self.threat.retain(|entry| entry.entity != entity);
    }

    /// Back to rest: no cast, auras or pending ability timer. Death, the
    /// start of an evade and its end all reset this way.
    pub(crate) fn reset_abilities(&mut self) {
        self.cast = None;
        self.auras.clear();
        self.ability_timer = 0;
    }
}

pub(crate) fn top_threat(threat: &[ThreatEntry]) -> Option<EntityRef> {
    threat
        .iter()
        .max_by(|left, right| {
            left.threat
                .cmp(&right.threat)
                .then(right.entity.cmp(&left.entity))
        })
        .map(|entry| entry.entity)
}

/// Aggro radius of a creature against a player: 10 m ± 1 m per level of
/// difference, clamped to 5–20 m.
#[must_use]
pub(crate) fn aggro_radius(creature_level: u8, player_level: u8) -> i32 {
    let difference = i32::from(creature_level) - i32::from(player_level);
    (AGGRO_RADIUS_UNITS + AGGRO_PER_LEVEL_UNITS * difference)
        .clamp(AGGRO_MIN_RADIUS_UNITS, AGGRO_MAX_RADIUS_UNITS)
}

impl ZoneSimulation {
    /// The content spawn and template of a creature.
    pub(crate) fn creature_content(
        content: &Arc<crate::ZoneContent>,
        id: CreatureId,
    ) -> Result<(&CreatureSpawn, &CreatureTemplate), ZoneError> {
        let spawn = content
            .creature_spawn(id)
            .ok_or_else(|| ZoneError::new("creature has no spawn in content"))?;
        let template = content
            .creature_template(spawn.template)
            .ok_or_else(|| ZoneError::new("creature spawn names an unknown template"))?;
        Ok((spawn, template))
    }

    /// (Re)spawns a creature at its spawn point: a new level roll and first
    /// wander delay from the zone RNG, full health, no threat or tap.
    pub(crate) fn spawn_creature(&mut self, id: CreatureId) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let (spawn, template) = Self::creature_content(&content, id)?;
        let level = self
            .rng
            .inclusive(u32::from(template.min_level), u32::from(template.max_level));
        let level = u8::try_from(level).unwrap_or(template.max_level);
        let wait = self.rng.inclusive(
            u32::from(WANDER_WAIT_TICKS[0]),
            u32::from(WANDER_WAIT_TICKS[1]),
        );
        let entity = EntityRef::Creature(id);
        let position = Vec3i::new(
            spawn.position[0],
            template.half_extents[1],
            spawn.position[1],
        );
        self.world
            .add_body(RigidBody::dynamic(
                body_id(entity),
                position,
                Vec3i::ZERO,
                vector(template.half_extents),
            ))
            .map_err(physics_error)?;
        self.place_in_interest(entity, position.x, position.z);
        self.creatures.insert(
            id,
            CreatureState {
                level,
                health: template.max_health(level),
                facing: spawn.facing,
                life: Life::Alive,
                ai: CreatureAi::Idle {
                    timer: u16::try_from(wait).unwrap_or(WANDER_WAIT_TICKS[1]),
                    destination: None,
                },
                threat: Vec::new(),
                swing_timer: 0,
                combat_timer: 0,
                tapped_by: None,
                loot: None,
                ability_timer: 0,
                cast: None,
                auras: Vec::new(),
            },
        );
        Ok(())
    }

    /// Where a creature is visible: its body while alive, its corpse, or
    /// nowhere once despawned.
    pub(crate) fn creature_position(
        &self,
        id: CreatureId,
        creature: &CreatureState,
    ) -> Result<Option<[i32; 3]>, ZoneError> {
        Ok(match creature.life {
            Life::Alive => {
                let position = self
                    .world
                    .body(body_id(EntityRef::Creature(id)))
                    .ok_or_else(|| ZoneError::new("creature physics body is missing"))?
                    .position();
                Some([position.x, position.y, position.z])
            }
            Life::Corpse { position, .. } => Some(position),
            Life::Despawned { .. } => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlayerId;

    fn entry(player: PlayerId, threat: u32) -> ThreatEntry {
        ThreatEntry {
            entity: EntityRef::Player(player),
            threat,
        }
    }

    #[test]
    fn highest_threat_leads_and_ties_go_to_the_lowest_entity() {
        assert_eq!(top_threat(&[]), None);
        assert_eq!(
            top_threat(&[entry(1, 5), entry(2, 9), entry(3, 9)]),
            Some(EntityRef::Player(2))
        );
        assert_eq!(
            top_threat(&[entry(1, 0), entry(2, 0)]),
            Some(EntityRef::Player(1))
        );
    }

    #[test]
    fn aggro_radius_scales_with_level_difference_and_clamps() {
        assert_eq!(aggro_radius(1, 1), 1_000);
        assert_eq!(aggro_radius(3, 1), 1_200);
        assert_eq!(aggro_radius(1, 4), 700);
        assert_eq!(aggro_radius(1, 60), 500);
        assert_eq!(aggro_radius(60, 1), 2_000);
    }
}
