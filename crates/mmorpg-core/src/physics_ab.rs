//! Probes for the physics backend A/B comparison (physics-engine #330), compiled only with the
//! test/bench `physics-ab` feature. They read the zone's physical world and ask the same
//! gameplay queries the rules ask (line of sight, door occupancy, projectile contact, ground
//! height), so the `physics_ab` test can compare decisions across backends. They change no
//! state.

use crate::entity::{body_id, entity_of};
use crate::physics::{RayHit, RigidBody, World};
use crate::{DoorId, EntityRef, NpcId, PlayerId, ZoneError, ZoneSimulation};

/// A unit's moving body as the physics world holds it, and whether it stands on ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitBody {
    pub unit: EntityRef,
    pub body: RigidBody,
    pub grounded: bool,
}

/// What a projectile launched from one unit toward another meets first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotProbe {
    /// The unit it met, or `None` for static geometry (content, terrain, doors).
    pub unit: Option<EntityRef>,
    pub hit: RayHit,
}

impl ZoneSimulation {
    /// The zone's physical world.
    #[must_use]
    pub const fn physics(&self) -> &World {
        &self.world
    }

    /// Every player and living, moving creature body, players first, each by ID.
    #[must_use]
    pub fn physics_units(&self) -> Vec<UnitBody> {
        self.players
            .keys()
            .map(|&id| EntityRef::Player(id))
            .chain(self.creatures.keys().map(|&id| EntityRef::Creature(id)))
            .filter_map(|unit| {
                let body = *self.world.body(body_id(unit))?;
                body.is_moving().then(|| UnitBody {
                    unit,
                    body,
                    grounded: self.world.grounded(body.id()),
                })
            })
            .collect()
    }

    /// Every standing breakable prop's fixed body (#288), by creature ID.
    #[must_use]
    pub fn prop_bodies(&self) -> Vec<RigidBody> {
        self.creatures
            .keys()
            .filter_map(|&id| self.world.body(body_id(EntityRef::Creature(id))))
            .filter(|body| !body.is_moving())
            .copied()
            .collect()
    }

    /// The line-of-sight rule between two units' body centres.
    pub fn line_of_sight_probe(&self, from: EntityRef, to: EntityRef) -> Result<bool, ZoneError> {
        self.line_of_sight(from, to)
    }

    /// Whether a unit stands where `door`'s leaf would go when opened (`open`) or closed.
    pub fn door_blocked_probe(&self, door: DoorId, open: bool) -> Result<bool, ZoneError> {
        let door = *self
            .content
            .door(door)
            .ok_or_else(|| ZoneError::new("unknown door"))?;
        self.leaf_blocked(&door, open)
    }

    /// Whether `player` sees `npc` for its services (the head-height sight line).
    pub fn npc_sight_probe(&self, player: PlayerId, npc: NpcId) -> Result<bool, ZoneError> {
        self.npc_in_sight(player, npc)
    }

    /// The first thing a straight shot from `owner`'s launch point to `target`'s centre meets,
    /// under the projectile rule's contact filter.
    pub fn shot_probe(
        &self,
        owner: EntityRef,
        target: EntityRef,
    ) -> Result<Option<ShotProbe>, ZoneError> {
        let from = self.launch_point(owner)?;
        let to = self
            .unit_centre(target)
            .ok_or_else(|| ZoneError::new("target physics body is missing"))?;
        let segment = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        Ok(self
            .world
            .cast_ray(from, segment, |body| self.projectile_meets(owner, body))
            .map(|hit| ShotProbe {
                unit: entity_of(hit.body),
                hit,
            }))
    }

    /// Whether `from`'s body fits along a charge at `to` (#57): the charge-path rule's shape cast.
    pub fn charge_path_probe(&self, from: EntityRef, to: EntityRef) -> Result<bool, ZoneError> {
        self.charge_path_clear(from, to)
    }

    /// The ground height gameplay reads at `(x, z)` (whole units, 0 on flat content).
    #[must_use]
    pub fn ground_probe(&self, x: i32, z: i32) -> i32 {
        self.ground_y(x, z)
    }
}
