//! The Warden's directional shield guard (#156, Atlas 5.2.29–5.2.68).
//!
//! The client holds a guard lane (high, middle or low) with
//! [`crate::ZoneCommand::Guard`]; the zone keeps that hold until a
//! deliberate release, death or a directed strike (the last-pressed gesture
//! wins, 5.2.37) lowers it. A held guard **protects** only while the player
//! is a living, unstunned Warden standing on ground (physics-confirmed, so a
//! jump suspends it, 5.2.63) and not recovering from a guard break.
//!
//! An incoming creature melee hit that rolled damage is resolved against a
//! protecting guard:
//!
//! - its **lane** is the band ([`crate::StrikeHeight::band`]) containing the
//!   attacker's contact point, three quarters up its physics body, measured
//!   from the defender's feet: rats bite low, wolves and boars middle,
//!   humanoids high;
//! - its **bearing** must fall inside the shield's coverage (5.2.68): an arc
//!   of ±[`GUARD_ARC_HALF_WIDTH`] centred [`GUARD_ARC_OFFSET`] from the
//!   defender's facing toward the shield (left) side, so the front and the
//!   shield flank are covered and the sword flank and the rear are not;
//! - its **impact** is the rolled damage plus the attacker's momentum: its
//!   physics velocity toward the defender (units per tick) scaled by its body
//!   volume over the defender's. Below [`GUARD_BREAK_IMPACT`] the blow is
//!   fully blocked; at or above it the blow lands in full and breaks the
//!   guard for [`GUARD_BREAK_TICKS`] (5.2.33, 5.2.35). Stationary blows of
//!   ordinary creatures are blocked; charging blows of wolf-sized or larger
//!   creatures and the heaviest critical hits overpower.
//!
//! A directed strike commits the Warden to [`STRIKE_RECOVERY_TICKS`] of
//! recovery: a guard pressed during it (or just before it, in the same
//! queue) is held but protects only once the recovery ends, the buffered
//! guard transition of Atlas 5.2.39 and 5.2.41; releasing it or a stun before
//! then discards it.
//!
//! A broken guard protects nothing; the hold survives the break, so the same
//! held lane protects again when recovery ends (5.2.59). A protecting guard
//! slows horizontal movement to [`GUARD_SPEED_PERCENT`] (5.2.61). Guarding
//! costs no resource (5.2.66) and a block grants none.

use crate::trig::{self, YAW_QUARTER_TURN};
use crate::{EntityRef, PLAYER_HALF_EXTENTS_UNITS, PlayerId, StrikeHeight, ZoneError};

/// Horizontal speed of a protecting guard, in percent of the unguarded speed.
pub const GUARD_SPEED_PERCENT: i32 = 60;
/// A directed strike's committed recovery: 0.2 s. A guard raised meanwhile
/// is held but protects only when the recovery ends; a stun before then
/// discards it (Atlas 5.2.39, 5.2.41).
/// The action graph's `DirectedSlash` recovery.
pub const STRIKE_RECOVERY_TICKS: u16 =
    crate::action::recovery_ticks(crate::action::ActionId::DirectedSlash);
/// Ticks a broken guard protects nothing: the graph's `GuardBreak`
/// recovery, 1.5 s.
pub const GUARD_BREAK_TICKS: u16 =
    crate::action::recovery_ticks(crate::action::ActionId::GuardBreak);
/// The impact (damage plus momentum) at which a blow overpowers a guard.
pub const GUARD_BREAK_IMPACT: u32 = 18;
/// The coverage arc's centre turns this far from the facing toward the
/// shield side (30°).
pub const GUARD_ARC_OFFSET: u16 = YAW_QUARTER_TURN / 3;
/// Half the coverage arc (75°): it spans 45° toward the sword side to 105°
/// toward the shield side.
pub const GUARD_ARC_HALF_WIDTH: u16 = YAW_QUARTER_TURN / 6 * 5;

/// A player's guard: the held lane and the remaining break recovery.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct GuardState {
    /// The lane the guard key holds; `None` once released.
    pub raised: Option<StrikeHeight>,
    /// Ticks of guard-break recovery left; nothing is blocked meanwhile.
    pub broken: u16,
}

/// The lane of a contact `height` units above the defender's feet.
#[must_use]
pub const fn lane_of(height: i32) -> StrikeHeight {
    if height < StrikeHeight::Low.band()[1] {
        StrikeHeight::Low
    } else if height < StrikeHeight::Middle.band()[1] {
        StrikeHeight::Middle
    } else {
        StrikeHeight::High
    }
}

/// How a guard met one incoming blow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GuardOutcome {
    /// No protecting guard, a different lane, or outside the shield's coverage.
    Unguarded,
    Blocked(StrikeHeight),
    Broken(StrikeHeight),
}

/// Whether a player's state lets its held guard protect, physics aside.
pub(crate) fn guard_ready(player: &crate::zone::PlayerState) -> Option<StrikeHeight> {
    let warden = player
        .class
        .is_some_and(|choice| choice.class == crate::PlayerClass::Warden);
    let stunned = crate::aura::has_kind(&player.auras, crate::AuraKind::Stun);
    match player.guard.raised {
        Some(lane)
            if warden
                && player.is_alive()
                && !stunned
                && player.guard.broken == 0
                && crate::action::rule_applying(
                    player,
                    crate::action::ActionInput::GuardProtection,
                    crate::action::Interaction::Hold,
                )
                .is_none() =>
        {
            Some(lane)
        }
        _ => None,
    }
}

impl crate::ZoneSimulation {
    /// The lane the player's guard currently protects: ready and standing
    /// on ground.
    pub(crate) fn guard_lane(
        &self,
        player_id: PlayerId,
    ) -> Result<Option<StrikeHeight>, ZoneError> {
        let Some(lane) = self.players.get(&player_id).and_then(guard_ready) else {
            return Ok(None);
        };
        let body = self
            .world
            .body(crate::entity::body_id(EntityRef::Player(player_id)))
            .ok_or_else(|| ZoneError::new("player physics body is missing"))?;
        Ok(crate::zone::is_grounded(&self.world, body)?.then_some(lane))
    }

    /// The lane of the player's protecting guard when its shield covers the bearing toward
    /// `from` (exact units), for blocking projectiles (#286); any held lane blocks a shot.
    pub(crate) fn shield_covers(
        &self,
        player_id: PlayerId,
        from: [f32; 3],
    ) -> Result<Option<StrikeHeight>, ZoneError> {
        let Some(lane) = self.guard_lane(player_id)? else {
            return Ok(None);
        };
        let Some(facing) = self.players.get(&player_id).map(|player| player.facing) else {
            return Ok(None);
        };
        let to = self.player_position(player_id)?;
        let [x, _, z] = crate::physics::to_units(from);
        let (dx, dz) = (x - to.x, z - to.z);
        let length = i64::try_from(trig::xz_length(dx, dz))
            .map_err(|_| ZoneError::new("guard distance overflow"))?;
        let (arc_x, arc_z) = trig::direction(facing.wrapping_add(GUARD_ARC_OFFSET));
        let along = i64::from(dx) * i64::from(arc_x) + i64::from(dz) * i64::from(arc_z);
        Ok((along >= length * i64::from(trig::cos(GUARD_ARC_HALF_WIDTH))).then_some(lane))
    }

    /// Resolves a melee impact against the player's guard, with an optional aimed lane.
    pub(crate) fn guard_outcome(
        &self,
        attacker: EntityRef,
        player_id: PlayerId,
        amount: u16,
        strike_lane: Option<StrikeHeight>,
    ) -> Result<GuardOutcome, ZoneError> {
        let Some(lane) = self.guard_lane(player_id)? else {
            return Ok(GuardOutcome::Unguarded);
        };
        let Some(facing) = self.players.get(&player_id).map(|player| player.facing) else {
            return Ok(GuardOutcome::Unguarded);
        };
        let body = self
            .world
            .body(crate::entity::body_id(attacker))
            .ok_or_else(|| ZoneError::new("attacker physics body is missing"))?;
        let (from, half, velocity) = (body.position(), body.half_extents(), body.velocity_units());
        let to = self.player_position(player_id)?;
        let feet = to.y - PLAYER_HALF_EXTENTS_UNITS[1];
        // Three quarters up the attacker's body.
        let contact = from.y - half.y + 2 * half.y * 3 / 4 - feet;
        if strike_lane.unwrap_or_else(|| lane_of(contact)) != lane {
            return Ok(GuardOutcome::Unguarded);
        }
        // The bearing from the defender to the attacker against the arc.
        let (dx, dz) = (from.x - to.x, from.z - to.z);
        let length = i64::try_from(trig::xz_length(dx, dz))
            .map_err(|_| ZoneError::new("guard distance overflow"))?;
        let (arc_x, arc_z) = trig::direction(facing.wrapping_add(GUARD_ARC_OFFSET));
        let along = i64::from(dx) * i64::from(arc_x) + i64::from(dz) * i64::from(arc_z);
        if along < length * i64::from(trig::cos(GUARD_ARC_HALF_WIDTH)) {
            return Ok(GuardOutcome::Unguarded);
        }
        // Momentum: the attacker's speed toward the defender, scaled by
        // its body volume over the defender's.
        let closing = if length == 0 {
            0
        } else {
            (-(i64::from(velocity.x) * i64::from(dx) + i64::from(velocity.z) * i64::from(dz))
                / length)
                .max(0)
        };
        let volume = |half: [i32; 3]| half.iter().map(|&value| i64::from(value)).product::<i64>();
        let momentum =
            closing * volume([half.x, half.y, half.z]) / volume(PLAYER_HALF_EXTENTS_UNITS).max(1);
        let impact = i64::from(amount).saturating_add(momentum);
        Ok(if impact >= i64::from(GUARD_BREAK_IMPACT) {
            GuardOutcome::Broken(lane)
        } else {
            GuardOutcome::Blocked(lane)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_split_the_strike_bands() {
        assert_eq!(lane_of(-5), StrikeHeight::Low);
        assert_eq!(lane_of(30), StrikeHeight::Low);
        assert_eq!(lane_of(60), StrikeHeight::Middle);
        assert_eq!(lane_of(67), StrikeHeight::Middle);
        assert_eq!(lane_of(130), StrikeHeight::High);
        assert_eq!(lane_of(400), StrikeHeight::High);
    }
}
