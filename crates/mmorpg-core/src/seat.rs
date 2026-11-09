//! Sitting (#619, Atlas 5.4.i619): authored seats with stable IDs and a player's or resident's
//! seat as authoritative state.
//!
//! A [`Seat`] is a chair or one place on a bench. `stand` is where a seated unit's body stands,
//! clear of the furniture's collider; `root` is where the clients draw its feet so the clip's
//! pelvis lands on the seat; `facing` is its seated yaw. A player sits with
//! [`crate::ZoneCommand::Sit`]: on the ground where it stands, or on a seat within
//! [`SEAT_REACH_UNITS`], which moves its body to `stand` and turns it to `facing`. One unit holds
//! a seat at a time, and a seat held by anyone else refuses with
//! [`crate::ErrorCode::SeatTaken`].
//!
//! Moving, jumping, a forced move, taking damage, casting, attacking, guarding, repairing or
//! leaving through a waystone ends a player's sit. A routine NPC is seated as a pure function of
//! the tick and its authored routine, needing no canonical state. When one arrives at a seat a
//! player holds, the player stands up: the authored routine wins and two units never hold one
//! seat. A player's sit is canonical, so replay and recovery keep it. It ends at a cross-zone
//! transfer like other session state (see [`crate::transfer`]), which walking already implies.

use crate::{EntityRef, ErrorCode, NpcId, PlayerId, ZoneError, ZoneEvent, ZoneSimulation};

/// A player reaches a seat within this horizontal distance of its `stand` (2.5 m).
pub const SEAT_REACH_UNITS: i32 = 250;
/// Seats a zone's content may author.
pub const MAX_SEATS: usize = 32;

/// A seat's stable content ID, nonzero.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SeatId(u8);

impl SeatId {
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// One authored seat, in units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Seat {
    pub id: SeatId,
    /// Where a seated unit's body stands (feet XZ), clear of the furniture.
    pub stand: [i32; 2],
    /// Where the clients draw a seated unit's feet.
    pub root: [i32; 2],
    /// A seated unit's yaw.
    pub facing: u16,
}

/// What a player sits on, if anything; canonical.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Sitting {
    #[default]
    No,
    Ground,
    On(SeatId),
}

impl Sitting {
    #[must_use]
    pub const fn is_sitting(self) -> bool {
        !matches!(self, Self::No)
    }
}

/// What a unit shows other players about its pose (#619): a sitter's seat follows from its
/// position ([`crate::ZoneContent::seat_at`]).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PostureView {
    #[default]
    Free,
    Sitting,
    /// A private-home resident in bed (#158).
    Asleep,
    /// A knocked-awake resident getting up (#619).
    Waking,
}

impl PostureView {
    /// Wire code, 0–3.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Free => 0,
            Self::Sitting => 1,
            Self::Asleep => 2,
            Self::Waking => 3,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Free),
            1 => Some(Self::Sitting),
            2 => Some(Self::Asleep),
            3 => Some(Self::Waking),
            _ => None,
        }
    }
}

impl ZoneSimulation {
    /// Resolves [`crate::ZoneCommand::Sit`]: the refusal, if any.
    pub(crate) fn sit(
        &mut self,
        player_id: PlayerId,
        requested: Option<SeatId>,
    ) -> Result<Option<(ErrorCode, Option<EntityRef>)>, ZoneError> {
        use crate::entity::body_id;
        use crate::physics::Vec3i;
        let refuse = |code| Ok(Some((code, None)));
        let body_ref = body_id(EntityRef::Player(player_id));
        let body = self
            .world
            .body(body_ref)
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
        let centre = body.position();
        let grounded = crate::zone::is_grounded(&self.world, body)?;
        let Some(player) = self.players.get(&player_id) else {
            return refuse(ErrorCode::InvalidTarget);
        };
        if !player.is_alive() {
            return refuse(ErrorCode::YouAreDead);
        }
        if player.traversal.holds() || player.roll.rolling().is_some() || !grounded {
            return refuse(ErrorCode::CannotSitHere);
        }
        if player.swimming {
            return refuse(ErrorCode::NotWhileSwimming);
        }
        // Asking again stands back up, by the clip's exit.
        let again = match (requested, player.sitting) {
            (None, sitting) => sitting.is_sitting(),
            (Some(id), sitting) => sitting == Sitting::On(id),
        };
        if again {
            self.update_player(player_id, |player| player.sitting = Sitting::No);
            return Ok(None);
        }
        // Standing exactly on a seat's spot sits on that seat, so the projection's reading of
        // the position always agrees with what is held.
        let seat = requested.or_else(|| {
            self.content
                .seat_at([centre.x, centre.z])
                .map(|seat| seat.id)
        });
        let Some(id) = seat else {
            self.update_player(player_id, |player| player.sitting = Sitting::Ground);
            return Ok(None);
        };
        let Some(seat) = self.content.seat(id).copied() else {
            return refuse(ErrorCode::InvalidTarget);
        };
        let [dx, dz] = [
            i64::from(seat.stand[0]) - i64::from(centre.x),
            i64::from(seat.stand[1]) - i64::from(centre.z),
        ];
        if dx * dx + dz * dz > i64::from(SEAT_REACH_UNITS).pow(2) {
            return refuse(ErrorCode::OutOfRange);
        }
        if self.seat_held(id, Some(player_id)) {
            return refuse(ErrorCode::SeatTaken);
        }
        let position = Vec3i::new(seat.stand[0], centre.y, seat.stand[1]);
        self.world
            .set_position(body_ref, position)
            .map_err(crate::zone::physics_error)?;
        self.world
            .set_velocity(body_ref, Vec3i::ZERO)
            .map_err(crate::zone::physics_error)?;
        self.update_player(player_id, |player| {
            player.sitting = Sitting::On(id);
            player.facing = seat.facing;
        });
        Ok(None)
    }

    /// Whether anyone but `except` holds seat `id`: a player, or a routine NPC seated now.
    pub(crate) fn seat_held(&self, id: SeatId, except: Option<PlayerId>) -> bool {
        self.players
            .iter()
            .any(|(&holder, player)| Some(holder) != except && player.sitting == Sitting::On(id))
            || self
                .content
                .routines()
                .iter()
                .any(|routine| self.npc_seat(routine.npc) == Some(id))
    }

    /// The seat routine NPC `npc` sits on now: standing still at its routine's seat.
    pub(crate) fn npc_seat(&self, npc: NpcId) -> Option<SeatId> {
        let routine = self.content.routine(npc)?;
        let placement = self.npc_placement(npc)?;
        if placement.velocity != [0, 0] {
            return None;
        }
        [routine.home_seat, routine.work_seat]
            .into_iter()
            .flatten()
            .find(|&id| {
                self.content
                    .seat(id)
                    .is_some_and(|seat| seat.stand == placement.position)
            })
    }

    /// What a routine NPC shows other players (#619).
    pub(crate) fn npc_posture(&self, npc: NpcId) -> PostureView {
        if self.waking(npc) {
            PostureView::Waking
        } else if self.npc_seat(npc).is_some() {
            PostureView::Sitting
        } else if self.in_bed(npc) {
            PostureView::Asleep
        } else {
            PostureView::Free
        }
    }

    /// The tick's end of sitting: whoever acted this tick stands, and a seat a routine NPC has
    /// just taken frees up (a player holding it stands).
    pub(crate) fn advance_sitting(&mut self) {
        let taken: Vec<SeatId> = self
            .content
            .routines()
            .iter()
            .filter_map(|routine| self.npc_seat(routine.npc))
            .collect();
        for (&player_id, player) in &mut self.players {
            let me = EntityRef::Player(player_id);
            let acted = player.events.iter().any(|event| match *event {
                ZoneEvent::AttackPerformed { source, .. }
                | ZoneEvent::CastStarted { source, .. }
                | ZoneEvent::AbilityUsed { source, .. }
                | ZoneEvent::StrikeSwept { source, .. } => source == me,
                _ => false,
            });
            let lost_seat = matches!(player.sitting, Sitting::On(seat) if taken.contains(&seat));
            if acted
                || lost_seat
                || player.guard.raised.is_some()
                || player.repairing.is_some()
                || player.cast.is_some()
                || !player.is_alive()
            {
                player.sitting = Sitting::No;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        DAY_TICKS, ErrorCode, PlayerId, Sitting, ZONE_START_TIME, ZoneCommand, ZoneEvent, ZoneId,
        ZoneSimulation, greyhaven_vale,
    };

    use super::{PostureView, SeatId};

    const P: PlayerId = 1;
    const OTHER: PlayerId = 2;

    fn at(time: u32) -> u64 {
        u64::from((time + DAY_TICKS - ZONE_START_TIME) % DAY_TICKS)
    }

    /// A zone at `tick` with players standing on the ground at `spots`, settled.
    fn zone(tick: u64, spots: &[[i32; 2]]) -> ZoneSimulation {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        for player in 1..=u32::try_from(spots.len()).unwrap() {
            zone.add_player(player).unwrap();
        }
        let mut state = zone.snapshot().unwrap();
        state.tick = tick;
        for (player, &[x, z]) in state.players.iter_mut().zip(spots) {
            let y =
                zone.content().definition().ground_at(x, z) + crate::PLAYER_HALF_EXTENTS_UNITS[1];
            player.position = crate::physics::from_units([x, y, z]);
        }
        let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        for _ in 0..3 {
            zone.advance_tick().unwrap();
        }
        zone
    }

    fn sitting(zone: &ZoneSimulation, player: PlayerId) -> Sitting {
        zone.players[&player].sitting
    }

    fn errors(zone: &ZoneSimulation, player: PlayerId) -> Vec<ErrorCode> {
        zone.players[&player]
            .events
            .iter()
            .filter_map(|event| match event {
                ZoneEvent::Error { code, .. } => Some(*code),
                _ => None,
            })
            .collect()
    }

    fn send(zone: &mut ZoneSimulation, player: PlayerId, sequence: u32, command: ZoneCommand) {
        zone.apply_command(player, sequence, command).unwrap();
        zone.advance_tick().unwrap();
    }

    fn posture(zone: &ZoneSimulation, viewer: PlayerId, entity: crate::EntityRef) -> PostureView {
        zone.snapshot_for_player(viewer)
            .unwrap()
            .entities
            .iter()
            .find(|record| record.entity() == entity)
            .unwrap()
            .flags
            .posture
    }

    const GROUND: ZoneCommand = ZoneCommand::Sit { seat: None };
    const STILL: [i32; 2] = [0, 1_600];

    #[test]
    fn slash_sit_sits_on_the_ground_projected_and_moving_or_jumping_stands() {
        let mut z = zone(0, &[STILL]);
        send(&mut z, P, 1, GROUND);
        assert_eq!(sitting(&z, P), Sitting::Ground);
        assert_eq!(
            posture(&z, P, crate::EntityRef::Player(P)),
            PostureView::Sitting
        );
        send(
            &mut z,
            P,
            2,
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: 0,
            },
        );
        assert_eq!(sitting(&z, P), Sitting::No);
        send(
            &mut z,
            P,
            3,
            ZoneCommand::Move {
                forward: 0,
                strafe: 0,
                facing: 0,
            },
        );
        for _ in 0..30 {
            z.advance_tick().unwrap();
        }
        send(&mut z, P, 4, GROUND);
        assert_eq!(sitting(&z, P), Sitting::Ground);
        send(&mut z, P, 5, ZoneCommand::Jump);
        assert_eq!(sitting(&z, P), Sitting::No);
        // Asking again stands back up.
        for _ in 0..60 {
            z.advance_tick().unwrap();
        }
        send(&mut z, P, 6, GROUND);
        send(&mut z, P, 7, GROUND);
        assert_eq!(sitting(&z, P), Sitting::No);
    }

    #[test]
    fn a_chair_moves_the_sitter_to_its_spot_and_a_held_seat_refuses() {
        let seat = *z_seat(2);
        let beside = [seat.stand[0] + 100, seat.stand[1]];
        let mut z = zone(0, &[beside, [seat.stand[0] - 100, seat.stand[1]]]);
        let take = ZoneCommand::Sit {
            seat: Some(seat.id),
        };
        send(&mut z, P, 1, take);
        assert_eq!(sitting(&z, P), Sitting::On(seat.id));
        let body = z.player_position(P).unwrap();
        assert_eq!([body.x, body.z], seat.stand);
        assert_eq!(z.players[&P].facing, seat.facing);
        // The other player is refused with the clear code and stays standing.
        send(&mut z, OTHER, 1, take);
        assert_eq!(errors(&z, OTHER), [ErrorCode::SeatTaken]);
        assert_eq!(sitting(&z, OTHER), Sitting::No);
        // The projection names the seat by where the body stands.
        assert_eq!(
            posture(&z, OTHER, crate::EntityRef::Player(P)),
            PostureView::Sitting
        );
    }

    #[test]
    fn a_seat_out_of_reach_is_refused() {
        let seat = *z_seat(2);
        let mut z = zone(0, &[[seat.stand[0] + 400, seat.stand[1]]]);
        send(
            &mut z,
            P,
            1,
            ZoneCommand::Sit {
                seat: Some(seat.id),
            },
        );
        assert_eq!(errors(&z, P), [ErrorCode::OutOfRange]);
        assert_eq!(sitting(&z, P), Sitting::No);
    }

    #[test]
    fn damage_and_attacking_end_a_sit() {
        let mut z = zone(0, &[STILL]);
        send(&mut z, P, 1, GROUND);
        z.damage_player(
            crate::EntityRef::Creature(crate::CreatureId::new(1)),
            P,
            1,
            false,
        )
        .unwrap();
        assert_eq!(sitting(&z, P), Sitting::No);
        send(&mut z, P, 2, GROUND);
        assert_eq!(sitting(&z, P), Sitting::Ground);
        send(&mut z, P, 3, ZoneCommand::Swing { facing: 0 });
        assert_eq!(sitting(&z, P), Sitting::No);
    }

    #[test]
    fn the_market_bench_seats_match_where_the_square_places_the_bench() {
        use crate::greyhaven_vale::market;
        let content = greyhaven_vale::content();
        let house = content
            .definition()
            .colliders()
            .iter()
            .find(|collider| collider.id == market::HOUSE)
            .unwrap()
            .position;
        let bench = market::placements([house[0], house[2]])
            .into_iter()
            .find(|placement| placement.item == market::Item::Bench)
            .unwrap();
        assert_eq!([bench.position[0], bench.position[2]], market::BENCH);
        assert_eq!(bench.yaw, 0, "its front faces +Z, as the seats sit");
    }

    fn z_seat(id: u8) -> &'static crate::Seat {
        // The hosted content lives for the process, so tests can hold its seats.
        let content: &'static _ = Box::leak(Box::new(greyhaven_vale::content()));
        content.seat(SeatId::new(id)).unwrap()
    }

    #[test]
    fn residents_sit_at_their_seats_by_their_routines_and_a_held_seat_is_theirs() {
        let content = greyhaven_vale::content();
        let finn = crate::NpcId::new(21);
        let hilda = crate::NpcId::new(2);
        // Finn is on the bench from the morning, and refuses a player his place.
        let seat = *content.seat(SeatId::new(1)).unwrap();
        let mut z = zone(0, &[[seat.stand[0] + 60, seat.stand[1] + 60]]);
        assert_eq!(z.npc_seat(finn), Some(seat.id));
        assert_eq!(
            posture(&z, P, crate::EntityRef::Npc(finn)),
            PostureView::Sitting
        );
        send(
            &mut z,
            P,
            1,
            ZoneCommand::Sit {
                seat: Some(seat.id),
            },
        );
        assert_eq!(errors(&z, P), [ErrorCode::SeatTaken]);
        // Hilda takes her chair for the night, and a player on it stands up as she arrives.
        let chair = *content.seat(SeatId::new(3)).unwrap();
        let night = at(crate::DUSK + crate::HOUR_TICKS);
        let mut z = zone(night, &[[chair.stand[0] + 80, chair.stand[1]]]);
        assert_eq!(
            z.npc_seat(hilda),
            Some(chair.id),
            "she is home by this hour"
        );
        z.players.get_mut(&P).unwrap().sitting = Sitting::On(chair.id);
        z.advance_tick().unwrap();
        assert_eq!(sitting(&z, P), Sitting::No);
        // By day she is at her tannery and sits on nothing.
        assert_eq!(zone(0, &[STILL]).npc_seat(hilda), None);
    }

    #[test]
    fn a_sit_survives_a_snapshot_restore() {
        let seat = *z_seat(2);
        let mut z = zone(0, &[[seat.stand[0] + 100, seat.stand[1]]]);
        send(
            &mut z,
            P,
            1,
            ZoneCommand::Sit {
                seat: Some(seat.id),
            },
        );
        let snapshot = z.snapshot().unwrap();
        let mut restored =
            ZoneSimulation::from_snapshot(snapshot, Arc::clone(z.content())).unwrap();
        assert_eq!(sitting(&restored, P), Sitting::On(seat.id));
        for _ in 0..20 {
            z.advance_tick().unwrap();
            restored.advance_tick().unwrap();
            assert_eq!(z.snapshot().unwrap(), restored.snapshot().unwrap());
        }
        // Two sitters on one seat in a recovered state fail closed.
        let mut bad = z.snapshot().unwrap();
        let held = bad.players[0].clone();
        bad.players.push(crate::CanonicalPlayerSnapshot {
            player_id: OTHER,
            ..held
        });
        assert!(ZoneSimulation::from_snapshot(bad, Arc::clone(z.content())).is_err());
    }
}
