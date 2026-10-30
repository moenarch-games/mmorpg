//! Interaction-light handoff corridors around cross-zone seams (#145).
//!
//! A seam between two logical zones is an explicit content invariant. Each
//! side authors a [`HandoffCorridor`]: a narrow XZ rectangle around the seam
//! in which movement, facing and presentation continue but nothing
//! irreversible or economically meaningful begins. Inside a corridor a player
//! cannot claim loot, trade with a vendor or accept, turn in or abandon a
//! quest ([`crate::ErrorCode::HandoffCorridor`]), and creatures evade the
//! player's attacks, so no kill, tap or reward starts there. Creatures turn
//! back before a corridor: one whose target stands in it, or whose body comes
//! within [`CORRIDOR_TURN_BACK_UNITS`] of it, leashes home. Content
//! validation keeps every creature's wander area, every NPC, routine path,
//! graveyard and player spawn slot clear of corridors, so no unit of one zone
//! ever crosses into another.
//!
//! The corridor's crossing strip marks where authority moves: a living player
//! whose body centre stands in it is due to transfer to the neighbouring zone
//! ([`crate::ZoneSimulation::seam_crossings`]). The transfer itself is the
//! same fenced protocol a teleport uses ([`crate::CharacterTransfer`]); a
//! host reads both kinds from [`crate::ZoneSimulation::departures`].

use crate::{PlayerId, XzBounds, ZoneError, ZoneId, ZoneSimulation};

/// Upper bound on corridors per content revision.
pub const MAX_HANDOFF_CORRIDORS: usize = 16;

/// How far before a corridor a creature turns back, measured from its body's
/// edge in units.
pub const CORRIDOR_TURN_BACK_UNITS: i32 = 200;

/// The rectangle grown by `margin` on every side.
pub(crate) const fn grown(bounds: XzBounds, margin: i32) -> XzBounds {
    XzBounds {
        min: [
            bounds.min[0].saturating_sub(margin),
            bounds.min[1].saturating_sub(margin),
        ],
        max: [
            bounds.max[0].saturating_add(margin),
            bounds.max[1].saturating_add(margin),
        ],
    }
}

/// Whether the box of `half` extents around `centre`, grown by `margin`,
/// overlaps `bounds`; touching does not count.
pub(crate) const fn overlaps_box(
    bounds: XzBounds,
    centre: [i32; 2],
    half: [i32; 2],
    margin: i32,
) -> bool {
    let reach = [
        half[0].saturating_add(margin),
        half[1].saturating_add(margin),
    ];
    bounds.overlaps(XzBounds {
        min: [
            centre[0].saturating_sub(reach[0]),
            centre[1].saturating_sub(reach[1]),
        ],
        max: [
            centre[0].saturating_add(reach[0]),
            centre[1].saturating_add(reach[1]),
        ],
    })
}

const fn encloses(outer: XzBounds, inner: XzBounds) -> bool {
    inner.min[0] >= outer.min[0]
        && inner.min[1] >= outer.min[1]
        && inner.max[0] <= outer.max[0]
        && inner.max[1] <= outer.max[1]
}

const fn nonempty(bounds: XzBounds) -> bool {
    bounds.min[0] < bounds.max[0] && bounds.min[1] < bounds.max[1]
}

/// One side of a cross-zone seam.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandoffCorridor {
    /// The logical zone on the far side of the seam.
    pub neighbor: ZoneId,
    /// The interaction-light region around the seam.
    pub corridor: XzBounds,
    /// The strip inside `corridor` where a player hands off to `neighbor`.
    pub crossing: XzBounds,
    /// Added to a position in this zone, it names the same place in the
    /// neighbour's coordinates.
    pub offset: [i32; 3],
}

impl HandoffCorridor {
    pub(crate) fn validate(&self) -> Result<(), ZoneError> {
        let in_range = |bounds: XzBounds| {
            bounds
                .min
                .iter()
                .chain(&bounds.max)
                .all(|value| value.abs() <= crate::MAX_CONTENT_COORDINATE_UNITS)
        };
        if !nonempty(self.corridor)
            || !nonempty(self.crossing)
            || !in_range(self.corridor)
            || !encloses(self.corridor, self.crossing)
            || self
                .offset
                .iter()
                .any(|value| value.abs() > 2 * crate::MAX_CONTENT_COORDINATE_UNITS)
        {
            return Err(ZoneError::new(
                "a handoff corridor needs valid bounds enclosing its crossing",
            ));
        }
        Ok(())
    }
}

/// A player due to leave for another logical zone and where it continues, in
/// that zone's coordinates: through a seam (the same body centre and facing)
/// or by waystone (the stone's arrival).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Departure {
    pub player: PlayerId,
    pub destination: crate::CharacterLocation,
}

impl ZoneSimulation {
    /// The corridor containing the XZ point, if any.
    #[must_use]
    pub fn handoff_corridor_at(&self, x: i32, z: i32) -> Option<&HandoffCorridor> {
        self.content
            .handoff_corridors()
            .iter()
            .find(|corridor| corridor.corridor.contains(x, z))
    }

    /// Whether the player's body centre stands in a handoff corridor.
    pub(crate) fn in_handoff_corridor(&self, player_id: PlayerId) -> bool {
        if self.content.handoff_corridors().is_empty() {
            return false;
        }
        self.player_position(player_id)
            .is_ok_and(|at| self.handoff_corridor_at(at.x, at.z).is_some())
    }

    /// Whether a creature body of `half` extents at `centre` has come within
    /// the turn-back margin of a corridor.
    pub(crate) fn near_handoff_corridor(&self, centre: [i32; 2], half: [i32; 2]) -> bool {
        self.content
            .handoff_corridors()
            .iter()
            .any(|corridor| overlaps_box(corridor.corridor, centre, half, CORRIDOR_TURN_BACK_UNITS))
    }

    /// Living players whose body centre stands in a corridor's crossing strip,
    /// in player-ID order, with where each continues. The host starts a
    /// transfer for each one it is not already moving.
    #[must_use]
    pub fn seam_crossings(&self) -> Vec<Departure> {
        let corridors = self.content.handoff_corridors();
        if corridors.is_empty() {
            return Vec::new();
        }
        self.players
            .iter()
            .filter(|(_, player)| player.is_alive())
            .filter_map(|(&player_id, player)| {
                let at = self.player_position(player_id).ok()?;
                let corridor = corridors
                    .iter()
                    .find(|corridor| corridor.crossing.contains(at.x, at.z))?;
                Some(Departure {
                    player: player_id,
                    destination: crate::CharacterLocation {
                        zone_id: corridor.neighbor,
                        position: [
                            at.x.checked_add(corridor.offset[0])?,
                            at.y.checked_add(corridor.offset[1])?,
                            at.z.checked_add(corridor.offset[2])?,
                        ],
                        facing: player.facing,
                    },
                })
            })
            .collect()
    }

    /// Every player due to leave this zone, in player-ID order: the seam
    /// crossings and the living travellers to a waystone of another zone
    /// ([`crate::ZoneCommand::UseWaystone`]). A host freezes each one with
    /// [`Self::freeze_for_transfer`] toward its destination; one it cannot
    /// move cancels a waystone departure with [`Self::cancel_departure`].
    #[must_use]
    pub fn departures(&self) -> Vec<Departure> {
        let mut departures = self.seam_crossings();
        departures.extend(
            self.transfers
                .waystone_departures
                .iter()
                .filter(|(player_id, _)| {
                    self.players
                        .get(player_id)
                        .is_some_and(crate::zone::PlayerState::is_alive)
                })
                .map(|(&player, &destination)| Departure {
                    player,
                    destination,
                }),
        );
        departures.sort_by_key(|departure| departure.player);
        departures.dedup_by_key(|departure| departure.player);
        departures
    }

    /// The player's departure, if it is due to leave.
    #[must_use]
    pub fn departure(&self, player_id: PlayerId) -> Option<Departure> {
        self.departures()
            .into_iter()
            .find(|departure| departure.player == player_id)
    }

    /// Drops the player's pending waystone departure; it stays where it is.
    pub fn cancel_departure(&mut self, player_id: PlayerId) -> bool {
        self.transfers
            .waystone_departures
            .remove(&player_id)
            .is_some()
    }
}
