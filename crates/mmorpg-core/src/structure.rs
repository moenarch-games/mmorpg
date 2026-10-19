//! Destructible structures (#305): a designated site's building is a graph of authored pieces
//! (walls, posts, decks, roofs), each with health and the pieces it rests on.
//!
//! - **Damage**: blasts (Fire Blast, powder barrels) hurt every standing piece within reach that
//!   the centre can see, with the blast's falloff; projectiles that hit a piece chip it by
//!   [`PROJECTILE_PIECE_DAMAGE`]. Health passes through the damage stages of [`PieceStage`].
//! - **Collapse**: a piece at zero health is destroyed: its collider leaves the physical world
//!   (or becomes its authored rubble blocker). A piece resting on others collapses as soon as
//!   none of them stands, in piece-ID order until nothing more falls.
//! - **Regrowth**: a piece rebuilds to full health its site's [`StructureSite::regrowth_ticks`]
//!   after its last change, once nothing stands in its box and something it rests on stands.
//! - **Repair** (owner decision 2026-10-19, Atlas 5.2.80): free; a player standing within
//!   [`REPAIR_REACH_UNITS`] of a piece and repairing it counts [`REPAIR_SPEEDUP_TICKS`] extra
//!   ticks toward its regrowth each tick. Moving stops it.
//!
//! The zone owns piece health and state; canonical snapshots keep both, and projections carry
//! only the pieces that are not intact ([`PieceView`]). Debris is presentation only.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::physics::{BodyId, RigidBody};
use crate::zone::{physics_error, vector};
use crate::{ZoneError, ZoneSimulation};

/// Pieces a zone's content may author: every one may be projected not intact, and the
/// projection budget leaves room for six (#305).
pub const MAX_PIECES: usize = 6;
/// Sites a zone's content may author.
pub const MAX_SITES: usize = 8;
/// What a projectile that hits a piece takes off its health.
pub const PROJECTILE_PIECE_DAMAGE: u32 = 3;
/// A player repairs a piece within this horizontal distance (units) of its box (2.5 m).
pub const REPAIR_REACH_UNITS: i32 = 250;
/// Each tick a player repairs a piece counts this many extra ticks toward its regrowth, free
/// (owner decision 2026-10-19, Atlas 5.2.80).
pub const REPAIR_SPEEDUP_TICKS: u32 = 4;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PieceId(u16);

impl PieceId {
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A designated destructible site and its restoration policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructureSite {
    pub id: u8,
    pub name: String,
    /// Ticks after a piece's last change before it rebuilds.
    pub regrowth_ticks: u32,
}

/// An authored box: centre and half extents, in units.
pub type PieceBox = ([i32; 3], [i32; 3]);

/// One authored piece.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructurePiece {
    pub id: PieceId,
    pub site: u8,
    /// Its physics body, in the static collider namespace.
    pub body: u32,
    pub position: [i32; 3],
    pub half_extents: [i32; 3],
    pub health: u32,
    /// The pieces it rests on; empty for a piece on the ground.
    pub supports: Vec<PieceId>,
    /// The server-side rubble blocker it leaves when destroyed, if gameplay needs one.
    pub rubble: Option<PieceBox>,
}

impl StructurePiece {
    /// Whether feet at `feet` (units) are within [`REPAIR_REACH_UNITS`] of its box, horizontally.
    #[must_use]
    pub fn within_repair_reach(&self, feet: [i32; 3]) -> bool {
        let outside = |axis: usize| {
            (i64::from(feet[axis]) - i64::from(self.position[axis])).abs()
                - i64::from(self.half_extents[axis])
        };
        let (dx, dz) = (outside(0).max(0), outside(2).max(0));
        dx * dx + dz * dz <= i64::from(REPAIR_REACH_UNITS).pow(2)
    }
}

/// How damaged a piece is.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PieceStage {
    Intact,
    /// Below two thirds of its health.
    Damaged,
    /// Below one third.
    Crumbling,
    /// No health: its collider is gone (or rubble).
    Destroyed,
}

impl PieceStage {
    #[must_use]
    pub fn of(health: u32, max: u32) -> Self {
        let (health, max) = (u64::from(health), u64::from(max.max(1)));
        if health == 0 {
            Self::Destroyed
        } else if health * 3 < max {
            Self::Crumbling
        } else if health * 3 < 2 * max {
            Self::Damaged
        } else {
            Self::Intact
        }
    }

    /// Stable wire code.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Intact => 0,
            Self::Damaged => 1,
            Self::Crumbling => 2,
            Self::Destroyed => 3,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Intact),
            1 => Some(Self::Damaged),
            2 => Some(Self::Crumbling),
            3 => Some(Self::Destroyed),
            _ => None,
        }
    }
}

/// A piece's state as every player sees it: only pieces that are not intact are projected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PieceView {
    pub piece: PieceId,
    pub stage: PieceStage,
}

/// A piece's canonical state: health, the tick it last changed (0: never) and the regrowth
/// ticks players' repair has added since.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalPiece {
    pub piece: PieceId,
    pub health: u32,
    pub changed_tick: u64,
    pub repaired: u32,
}

pub(crate) fn validate(
    sites: &[StructureSite],
    pieces: &[StructurePiece],
) -> Result<(), ZoneError> {
    if sites.len() > MAX_SITES || pieces.len() > MAX_PIECES {
        return Err(ZoneError::new("structure content capacity reached"));
    }
    if sites.windows(2).any(|pair| pair[0].id >= pair[1].id)
        || pieces.windows(2).any(|pair| pair[0].id >= pair[1].id)
    {
        return Err(ZoneError::new(
            "structure sites and pieces must be in ascending ID order",
        ));
    }
    let in_range = |(centre, half): PieceBox| {
        half.iter().all(|half| *half > 0)
            && (0..3).all(|axis| {
                (i64::from(centre[axis]) - i64::from(half[axis])).abs()
                    <= i64::from(crate::MAX_CONTENT_COORDINATE_UNITS)
                    && (i64::from(centre[axis]) + i64::from(half[axis])).abs()
                        <= i64::from(crate::MAX_CONTENT_COORDINATE_UNITS)
            })
    };
    for piece in pieces {
        let supports_known = piece
            .supports
            .iter()
            .all(|support| *support < piece.id && pieces.iter().any(|other| other.id == *support));
        if piece.health == 0
            || !sites.iter().any(|site| site.id == piece.site)
            || !supports_known
            || !in_range((piece.position, piece.half_extents))
            || piece.rubble.is_some_and(|rubble| !in_range(rubble))
            || sites.iter().any(|site| site.regrowth_ticks == 0)
        {
            return Err(ZoneError::new("structure piece is invalid"));
        }
    }
    Ok(())
}

/// A piece's live state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PieceState {
    pub health: u32,
    pub changed_tick: u64,
    /// Regrowth ticks repair added since the last change.
    pub repaired: u32,
}

impl ZoneSimulation {
    fn piece_stands(&self, id: PieceId) -> bool {
        self.pieces.get(&id).is_some_and(|state| state.health > 0)
    }

    /// The piece whose collider is `body`, if it stands.
    pub(crate) fn piece_of_body(&self, body: BodyId) -> Option<PieceId> {
        self.content
            .pieces()
            .iter()
            .find(|piece| u64::from(piece.body) == body.0)
            .map(|piece| piece.id)
            .filter(|id| self.piece_stands(*id))
    }

    /// Adds every piece's collider, all intact: building a zone.
    pub(crate) fn build_pieces(
        world: &mut crate::physics::World,
        content: &crate::ZoneContent,
    ) -> Result<BTreeMap<PieceId, PieceState>, ZoneError> {
        let mut states = BTreeMap::new();
        for piece in content.pieces() {
            world
                .add_body(RigidBody::fixed(
                    BodyId(u64::from(piece.body)),
                    vector(piece.position),
                    vector(piece.half_extents),
                ))
                .map_err(physics_error)?;
            states.insert(
                piece.id,
                PieceState {
                    health: piece.health,
                    changed_tick: 0,
                    repaired: 0,
                },
            );
        }
        Ok(states)
    }

    /// Puts `piece`'s collider (or rubble, or nothing) where its health says.
    fn place_piece(&mut self, piece: &StructurePiece, standing: bool) -> Result<(), ZoneError> {
        let body = BodyId(u64::from(piece.body));
        let shape = if standing {
            Some((piece.position, piece.half_extents))
        } else {
            piece.rubble
        };
        self.world.remove_body(body);
        if let Some((centre, half)) = shape {
            self.world
                .add_body(RigidBody::fixed(body, vector(centre), vector(half)))
                .map_err(physics_error)?;
        }
        Ok(())
    }

    /// Takes `amount` off a standing piece at tick `now`; a piece that reaches zero is
    /// destroyed and whatever rested only on it collapses.
    pub(crate) fn damage_piece(
        &mut self,
        id: PieceId,
        amount: u32,
        now: u64,
    ) -> Result<(), ZoneError> {
        let Some(state) = self.pieces.get_mut(&id).filter(|state| state.health > 0) else {
            return Ok(());
        };
        state.health = state.health.saturating_sub(amount.max(1));
        state.changed_tick = now;
        state.repaired = 0;
        if state.health > 0 {
            return Ok(());
        }
        let content = Arc::clone(&self.content);
        if let Some(piece) = content.pieces().iter().find(|piece| piece.id == id) {
            self.place_piece(piece, false)?;
        }
        self.collapse_unsupported(now)
    }

    /// Destroys every standing piece none of whose supports stands, in ID order, until
    /// nothing more falls.
    fn collapse_unsupported(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        loop {
            let falling = content.pieces().iter().find(|piece| {
                self.piece_stands(piece.id)
                    && !piece.supports.is_empty()
                    && !piece
                        .supports
                        .iter()
                        .any(|support| self.piece_stands(*support))
            });
            let Some(piece) = falling else {
                return Ok(());
            };
            if let Some(state) = self.pieces.get_mut(&piece.id) {
                state.health = 0;
                state.changed_tick = now;
                state.repaired = 0;
            }
            self.place_piece(piece, false)?;
        }
    }

    /// A blast (#287) at `centre` hurts every standing piece whose box comes within `radius`
    /// that the centre can see: `amount` with the blast's linear falloff to half at the edge.
    pub(crate) fn blast_pieces(
        &mut self,
        centre: [i32; 3],
        radius: i32,
        amount: u32,
        now: u64,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let radius = i64::from(radius.max(1));
        for piece in content.pieces() {
            if !self.piece_stands(piece.id) {
                continue;
            }
            // The box's point nearest the centre.
            let near: [i32; 3] = std::array::from_fn(|axis| {
                centre[axis].clamp(
                    piece.position[axis] - piece.half_extents[axis],
                    piece.position[axis] + piece.half_extents[axis],
                )
            });
            let delta: [i64; 3] =
                std::array::from_fn(|axis| i64::from(near[axis]) - i64::from(centre[axis]));
            let distance = delta.iter().map(|d| d * d).sum::<i64>().isqrt();
            if distance > radius {
                continue;
            }
            // Another collider between the centre and the box shields it.
            let body = BodyId(u64::from(piece.body));
            #[allow(clippy::cast_precision_loss)]
            let hit = self.world.cast_ray(
                centre.map(|value| value as f32),
                delta.map(|value| value as f32),
                |other| other.0 < crate::entity::PLAYER_BODY_BASE,
            );
            if hit.is_some_and(|hit| hit.body != body && hit.fraction < 0.999) {
                continue;
            }
            let share = 1_000 - 500 * distance / radius;
            let damage = u32::try_from(i64::from(amount) * share / 1_000).unwrap_or(0);
            self.damage_piece(piece.id, damage, now)?;
        }
        Ok(())
    }

    /// The `Repair` intent (#305): starts repairing a known piece within reach, or stops.
    pub(crate) fn start_repair(
        &mut self,
        player_id: crate::PlayerId,
        piece: Option<PieceId>,
    ) -> Result<Option<(crate::ErrorCode, Option<crate::EntityRef>)>, ZoneError> {
        let Some(piece) = piece else {
            self.update_player(player_id, |player| player.repairing = None);
            return Ok(None);
        };
        if !self.content.pieces().iter().any(|known| known.id == piece) {
            return Ok(Some((crate::ErrorCode::InvalidTarget, None)));
        }
        if !self.within_repair_reach(player_id, piece)? {
            return Ok(Some((crate::ErrorCode::OutOfRange, None)));
        }
        self.update_player(player_id, |player| player.repairing = Some(piece));
        Ok(None)
    }

    fn within_repair_reach(
        &self,
        player_id: crate::PlayerId,
        piece: PieceId,
    ) -> Result<bool, ZoneError> {
        let Some(piece) = self.content.pieces().iter().find(|known| known.id == piece) else {
            return Ok(false);
        };
        let feet = self.player_position(player_id)?;
        Ok(piece.within_repair_reach([feet.x, feet.y, feet.z]))
    }

    /// Players repairing a piece within reach bring its regrowth closer, in player-ID order; a
    /// repair whose player died or left reach, or whose piece is whole, stops.
    fn advance_repairs(&mut self) -> Result<(), ZoneError> {
        let repairs: Vec<_> = self
            .players
            .iter()
            .filter_map(|(&player_id, player)| {
                Some((player_id, player.repairing?, player.is_alive()))
            })
            .collect();
        for (player_id, piece, alive) in repairs {
            let whole = self
                .pieces
                .get(&piece)
                .zip(self.content.pieces().iter().find(|known| known.id == piece))
                .is_none_or(|(state, known)| state.health >= known.health);
            if !alive || whole || !self.within_repair_reach(player_id, piece)? {
                self.update_player(player_id, |player| player.repairing = None);
                continue;
            }
            if let Some(state) = self.pieces.get_mut(&piece) {
                state.repaired = state.repaired.saturating_add(REPAIR_SPEEDUP_TICKS);
            }
        }
        Ok(())
    }

    /// Step 7: pieces rebuild to full health their site's regrowth time after their last
    /// change, once their box is clear of units and, for a piece resting on others, one of
    /// them stands; in ID order, so a wall regrows before the deck on it.
    pub(crate) fn regrow_pieces(&mut self, now: u64) -> Result<(), ZoneError> {
        self.advance_repairs()?;
        let content = Arc::clone(&self.content);
        for piece in content.pieces() {
            let Some(state) = self.pieces.get(&piece.id).copied() else {
                continue;
            };
            let regrowth = content
                .sites()
                .iter()
                .find(|site| site.id == piece.site)
                .map_or(u32::MAX, |site| site.regrowth_ticks);
            let elapsed = now
                .saturating_sub(state.changed_tick)
                .saturating_add(u64::from(state.repaired));
            if state.health >= piece.health || elapsed < u64::from(regrowth) {
                continue;
            }
            let supported = piece.supports.is_empty()
                || piece
                    .supports
                    .iter()
                    .any(|support| self.piece_stands(*support));
            if !supported {
                continue;
            }
            if state.health == 0 {
                let occupied = self
                    .world
                    .overlap_query(crate::physics::Aabb::new(
                        vector(piece.position),
                        vector(piece.half_extents),
                    ))
                    .map_err(physics_error)?
                    .into_iter()
                    .any(|body| crate::entity::entity_of(body).is_some());
                if occupied {
                    continue;
                }
                self.place_piece(piece, true)?;
            }
            self.pieces.insert(
                piece.id,
                PieceState {
                    health: piece.health,
                    changed_tick: now,
                    repaired: 0,
                },
            );
        }
        Ok(())
    }

    /// Pieces that are not intact, in ID order, for every projection.
    pub(crate) fn piece_views(&self) -> Vec<PieceView> {
        self.content
            .pieces()
            .iter()
            .filter_map(|piece| {
                let state = self.pieces.get(&piece.id)?;
                let stage = PieceStage::of(state.health, piece.health);
                (stage != PieceStage::Intact).then_some(PieceView {
                    piece: piece.id,
                    stage,
                })
            })
            .collect()
    }

    /// Every piece's state in content order, for canonical snapshots.
    pub(crate) fn canonical_pieces(&self) -> Vec<CanonicalPiece> {
        self.content
            .pieces()
            .iter()
            .filter_map(|piece| {
                let state = self.pieces.get(&piece.id)?;
                Some(CanonicalPiece {
                    piece: piece.id,
                    health: state.health,
                    changed_tick: state.changed_tick,
                    repaired: state.repaired,
                })
            })
            .collect()
    }

    pub(crate) fn restore_pieces(
        &mut self,
        pieces: &[CanonicalPiece],
        tick: u64,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        if pieces.len() != content.pieces().len() {
            return Err(ZoneError::new(
                "snapshot pieces do not match the content pieces",
            ));
        }
        for (record, piece) in pieces.iter().zip(content.pieces()) {
            if record.piece != piece.id
                || record.health > piece.health
                || record.changed_tick > tick
                || (record.health == piece.health && record.repaired > 0)
            {
                return Err(ZoneError::new("snapshot piece is invalid"));
            }
            self.pieces.insert(
                piece.id,
                PieceState {
                    health: record.health,
                    changed_tick: record.changed_tick,
                    repaired: record.repaired,
                },
            );
            if record.health == 0 {
                self.place_piece(piece, false)?;
            }
        }
        // A restored world holds no piece left without supports.
        if content.pieces().iter().any(|piece| {
            self.piece_stands(piece.id)
                && !piece.supports.is_empty()
                && !piece
                    .supports
                    .iter()
                    .any(|support| self.piece_stands(*support))
        }) {
            return Err(ZoneError::new("snapshot piece stands without support"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_follow_thirds_of_health() {
        assert_eq!(PieceStage::of(90, 90), PieceStage::Intact);
        assert_eq!(PieceStage::of(60, 90), PieceStage::Intact);
        assert_eq!(PieceStage::of(59, 90), PieceStage::Damaged);
        assert_eq!(PieceStage::of(30, 90), PieceStage::Damaged);
        assert_eq!(PieceStage::of(29, 90), PieceStage::Crumbling);
        assert_eq!(PieceStage::of(1, 90), PieceStage::Crumbling);
        assert_eq!(PieceStage::of(0, 90), PieceStage::Destroyed);
        for stage in [
            PieceStage::Intact,
            PieceStage::Damaged,
            PieceStage::Crumbling,
            PieceStage::Destroyed,
        ] {
            assert_eq!(PieceStage::from_code(stage.code()), Some(stage));
        }
        assert_eq!(PieceStage::from_code(4), None);
    }
}
