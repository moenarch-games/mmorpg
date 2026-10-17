//! Doors (#158): content-authored physical leaves the zone opens and closes for every player.
//!
//! A door is one fixed physics body whose box is either its closed or its open box. A living
//! player within [`DOOR_REACH_UNITS`] of the closed leaf asks for a state with
//! [`crate::ZoneCommand::UseDoor`]; the zone moves the leaf on the next tick unless a unit's
//! body overlaps the box the leaf would take ([`crate::ErrorCode::DoorBlocked`]). Asking for
//! the state a door already has changes nothing, so two players cannot toggle it against each
//! other. Door states belong to canonical state and every projection.

use crate::{EntityRef, ErrorCode, PlayerId, ZoneError, ZoneSimulation};

/// A player reaches a door within this horizontal distance of its closed leaf's centre (2.5 m).
pub const DOOR_REACH_UNITS: i32 = 250;
/// NPC service sight lines run between heads, this far above the feet (1.6 m).
pub const NPC_SIGHT_HEIGHT_UNITS: i32 = 160;
/// Doors a zone's content may author.
pub const MAX_DOORS: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DoorId(u8);

impl DoorId {
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// An axis-aligned box as centre and half extents, in units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DoorBox {
    pub position: [i32; 3],
    pub half_extents: [i32; 3],
}

/// One authored door: its physics body ID (in the static collider namespace) and both leaf boxes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Door {
    pub id: DoorId,
    pub body: u32,
    pub closed: DoorBox,
    pub open: DoorBox,
}

impl Door {
    #[must_use]
    pub const fn leaf(&self, open: bool) -> DoorBox {
        if open { self.open } else { self.closed }
    }

    /// Whether feet at `position` (units) are within [`DOOR_REACH_UNITS`] of the closed leaf.
    #[must_use]
    pub fn within_reach(&self, position: [i32; 3]) -> bool {
        let [dx, dz] = [
            i64::from(position[0]) - i64::from(self.closed.position[0]),
            i64::from(position[2]) - i64::from(self.closed.position[2]),
        ];
        dx * dx + dz * dz <= i64::from(DOOR_REACH_UNITS).pow(2)
    }
}

/// A door's state as every player sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DoorView {
    pub door: DoorId,
    pub open: bool,
}

impl ZoneSimulation {
    /// Every content door's state, in door-ID order.
    #[must_use]
    pub fn door_views(&self) -> Vec<DoorView> {
        self.content
            .doors()
            .iter()
            .map(|door| DoorView {
                door: door.id,
                open: self.doors.get(&door.id).copied().unwrap_or(false),
            })
            .collect()
    }

    /// Opens or closes `door` for `player_id`; a refusal names why.
    pub(crate) fn use_door(
        &mut self,
        player_id: PlayerId,
        id: DoorId,
        open: bool,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        let Some(door) = self.content.door(id).copied() else {
            return Ok(Some((ErrorCode::InvalidTarget, None)));
        };
        if self.doors.get(&id).copied().unwrap_or(false) == open {
            return Ok(None);
        }
        let from = self.player_position(player_id)?;
        if !door.within_reach([from.x, from.y, from.z]) {
            return Ok(Some((ErrorCode::OutOfRange, None)));
        }
        let leaf = door.leaf(open);
        let probe = physics_engine::Aabb::new(
            crate::zone::vector(leaf.position),
            crate::zone::vector(leaf.half_extents),
        );
        let blocked = self
            .world
            .overlap_query(probe)
            .map_err(|error| ZoneError::new(format!("door overlap query failed: {error}")))?
            .into_iter()
            .any(crate::entity::is_unit_body);
        if blocked {
            return Ok(Some((ErrorCode::DoorBlocked, None)));
        }
        self.set_door(&door, open)?;
        Ok(None)
    }

    /// Moves `door`'s leaf body to its `open` or closed box.
    pub(crate) fn set_door(&mut self, door: &Door, open: bool) -> Result<(), ZoneError> {
        let leaf = door.leaf(open);
        self.world
            .replace_body(physics_engine::RigidBody::fixed(
                physics_engine::BodyId(u64::from(door.body)),
                crate::zone::vector(leaf.position),
                crate::zone::vector(leaf.half_extents),
            ))
            .map_err(|error| ZoneError::new(format!("door body update failed: {error}")))?;
        self.doors.insert(door.id, open);
        Ok(())
    }

    /// Whether no content collider or closed door lies on the sight line between the player's
    /// and `npc`'s heads ([`NPC_SIGHT_HEIGHT_UNITS`] above their feet): NPC services are never
    /// reached through walls, but a bar counter between them does not block it (#158).
    pub(crate) fn npc_in_sight(
        &self,
        player_id: PlayerId,
        npc: crate::NpcId,
    ) -> Result<bool, ZoneError> {
        let Some(feet) = self.content.npc(npc).map(|npc| npc.position) else {
            return Ok(false);
        };
        let centre = self.player_position(player_id)?;
        let eye = centre.y - crate::PLAYER_HALF_EXTENTS_UNITS[1] + NPC_SIGHT_HEIGHT_UNITS;
        let ray = physics_engine::Ray::new(
            physics_engine::Vec3i::new(centre.x, eye, centre.z),
            physics_engine::Vec3i::new(
                feet[0] - centre.x,
                NPC_SIGHT_HEIGHT_UNITS - eye,
                feet[1] - centre.z,
            ),
        );
        let hits = self
            .world
            .ray_cast(ray, 1)
            .map_err(|error| ZoneError::new(format!("npc sight query failed: {error}")))?;
        Ok(!hits
            .iter()
            .any(|hit| hit.body.0 < crate::entity::PLAYER_BODY_BASE))
    }
}
