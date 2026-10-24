//! Doors (#158): content-authored physical leaves the zone opens and closes for every player.
//!
//! A door is one fixed physics body whose box is either its closed or its open box. A living
//! player within [`DOOR_REACH_UNITS`] of the closed leaf asks for a state with
//! [`crate::ZoneCommand::UseDoor`]; the zone moves the leaf on the next tick unless a unit's
//! body overlaps the box the leaf would take ([`crate::ErrorCode::DoorBlocked`]). Asking for
//! the state a door already has changes nothing, so two players cannot toggle it against each
//! other, and a player cannot move a leaf within [`DOOR_COOLDOWN_TICKS`] of its last change
//! ([`crate::ErrorCode::DoorBusy`]). A routine NPC opens a closed door on its path (#158).
//! Door states and their change ticks belong to canonical state and every projection.

use crate::{EntityRef, ErrorCode, PlayerId, ZoneError, ZoneSimulation};

/// A player reaches a door within this horizontal distance of its closed leaf's centre (2.5 m).
pub const DOOR_REACH_UNITS: i32 = 250;
/// NPC service sight lines run between heads, this far above the feet (1.6 m).
pub const NPC_SIGHT_HEIGHT_UNITS: i32 = 160;
/// Doors a zone's content may author.
pub const MAX_DOORS: usize = 16;
/// Players cannot move a door leaf again within 1.5 s of its last change.
pub const DOOR_COOLDOWN_TICKS: u64 = crate::ticks_from_ms(1_500) as u64;

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

/// A door's canonical state: its view and the tick its leaf last moved (0: never).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalDoor {
    pub door: DoorId,
    pub open: bool,
    pub changed_tick: u64,
}

/// A door's live state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DoorState {
    pub(crate) open: bool,
    pub(crate) changed_tick: u64,
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
                open: self.doors.get(&door.id).is_some_and(|state| state.open),
            })
            .collect()
    }

    /// Every content door's canonical state, in door-ID order.
    #[must_use]
    pub(crate) fn canonical_doors(&self) -> Vec<CanonicalDoor> {
        self.content
            .doors()
            .iter()
            .map(|door| {
                let state = self.doors.get(&door.id).copied().unwrap_or_default();
                CanonicalDoor {
                    door: door.id,
                    open: state.open,
                    changed_tick: state.changed_tick,
                }
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
        let state = self.doors.get(&id).copied().unwrap_or_default();
        if state.open == open {
            return Ok(None);
        }
        if state.changed_tick != 0 && self.tick < state.changed_tick + DOOR_COOLDOWN_TICKS {
            return Ok(Some((ErrorCode::DoorBusy, None)));
        }
        let from = self.player_position(player_id)?;
        if !door.within_reach([from.x, from.y, from.z]) {
            return Ok(Some((ErrorCode::OutOfRange, None)));
        }
        if self.leaf_blocked(&door, open)? {
            return Ok(Some((ErrorCode::DoorBlocked, None)));
        }
        self.set_door(&door, open, self.tick)?;
        Ok(None)
    }

    /// Whether a unit's body overlaps the box `door`'s leaf would take. The probe never sees
    /// terrain (Rapier skips cuboid–heightfield overlaps), which is fine: only units count.
    pub(crate) fn leaf_blocked(&self, door: &Door, open: bool) -> Result<bool, ZoneError> {
        let leaf = door.leaf(open);
        let probe = crate::physics::Aabb::new(
            crate::zone::vector(leaf.position),
            crate::zone::vector(leaf.half_extents),
        );
        Ok(self
            .world
            .overlap_query(probe)
            .map_err(|error| ZoneError::new(format!("door overlap query failed: {error}")))?
            .into_iter()
            .any(crate::entity::is_unit_body))
    }

    /// A routine NPC opens `door` unless a unit stands in the leaf's way; it tries again next
    /// tick while still in reach.
    pub(crate) fn try_open_for_npc(&mut self, door: &Door) -> Result<(), ZoneError> {
        if !self.leaf_blocked(door, true)? {
            self.set_door(door, true, self.tick)?;
        }
        Ok(())
    }

    /// Moves `door`'s leaf body to its `open` or closed box, recording `tick` as its change.
    pub(crate) fn set_door(&mut self, door: &Door, open: bool, tick: u64) -> Result<(), ZoneError> {
        self.invalidate_navigation();
        let leaf = door.leaf(open);
        self.world
            .replace_body(crate::physics::RigidBody::fixed(
                crate::physics::BodyId(u64::from(door.body)),
                crate::zone::vector(leaf.position),
                crate::zone::vector(leaf.half_extents),
            ))
            .map_err(|error| ZoneError::new(format!("door body update failed: {error}")))?;
        self.doors.insert(
            door.id,
            DoorState {
                open,
                changed_tick: tick,
            },
        );
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
        let Some(feet) = self.npc_feet(npc) else {
            return Ok(false);
        };
        let centre = self.player_position(player_id)?;
        let eye = centre.y - crate::PLAYER_HALF_EXTENTS_UNITS[1] + NPC_SIGHT_HEIGHT_UNITS;
        Ok(!self.world.segment_blocked(
            crate::physics::Vec3i::new(centre.x, eye, centre.z),
            crate::physics::Vec3i::new(
                feet[0] - centre.x,
                NPC_SIGHT_HEIGHT_UNITS - eye,
                feet[1] - centre.z,
            ),
            |body| body.0 < crate::entity::PLAYER_BODY_BASE,
        ))
    }
}
