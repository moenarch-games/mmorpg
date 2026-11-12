//! Predicted own movement for a client (#680, Atlas 5.9.i680), a netcode option an arena tester
//! may choose per client.
//!
//! The predictor is a private [`ZoneSimulation`] over the same content holding only the
//! viewer's own body, so it moves with exactly the zone's movement rules and physics
//! (`docs/ARCHITECTURE.md`: no second movement implementation). It never decides anything:
//! the client reconciles it against each projection by [`MovementPredictor::reset`]ting it to
//! the zone's projected own state at the projection's tick and replaying the movement it sent
//! since, as the zone's `acknowledged_sequence` tells it. Other units, auras and combat are
//! absent, so the zone's answer corrects whatever they changed.

use std::sync::Arc;

use crate::entity::body_id;
use crate::physics::{SUBUNITS_PER_UNIT, Vec3i};
use crate::zone::{Axis, physics_error};
use crate::{EntityRef, PlayerId, ZoneCommand, ZoneContent, ZoneError, ZoneId, ZoneSimulation};

const PLAYER: PlayerId = 1;

/// A client's prediction of its own body (#680).
pub struct MovementPredictor {
    zone: ZoneSimulation,
    sequence: u32,
}

impl MovementPredictor {
    /// A predictor over `content`: its static world, platforms and movement rules, with the
    /// viewer's body only.
    ///
    /// # Errors
    /// Fails when the content cannot host a player.
    pub fn new(zone_id: ZoneId, content: Arc<ZoneContent>) -> Result<Self, ZoneError> {
        let mut zone = ZoneSimulation::without_creatures(zone_id, content)?;
        zone.add_player(PLAYER)?;
        Ok(Self { zone, sequence: 0 })
    }

    /// Starts again from the zone's projection at `tick`: the own body's centre (units), its
    /// projected velocity (whole units per tick) and facing. Held movement intent stays as
    /// it was; the caller applies the movement the zone had acknowledged by then.
    ///
    /// # Errors
    /// Fails when the body is missing.
    pub fn reset(
        &mut self,
        tick: u64,
        centre: [i32; 3],
        velocity: [i32; 3],
        facing: u16,
    ) -> Result<(), ZoneError> {
        // Lifts and other schedules read the tick.
        self.zone.tick = tick;
        let body = body_id(EntityRef::Player(PLAYER));
        let [x, y, z] = centre;
        self.zone
            .world
            .set_position(body, Vec3i::new(x, y, z))
            .map_err(physics_error)?;
        let [vx, vy, vz] = velocity.map(|axis| axis * SUBUNITS_PER_UNIT);
        self.zone
            .world
            .set_velocity(body, Vec3i::new(vx, vy, vz))
            .map_err(physics_error)?;
        self.zone.update_player(PLAYER, |player| {
            player.facing = facing;
            player.jump_pending = false;
        });
        self.zone.place_in_interest(EntityRef::Player(PLAYER), x, z);
        Ok(())
    }

    /// Applies one movement command (move, rise, jump or roll) the client sent; any other
    /// command is not movement and changes nothing.
    ///
    /// # Errors
    /// Fails for a malformed movement command.
    pub fn apply(&mut self, command: ZoneCommand) -> Result<(), ZoneError> {
        if !matches!(
            command,
            ZoneCommand::Move { .. } | ZoneCommand::Rise(_) | ZoneCommand::Jump | ZoneCommand::Roll
        ) {
            return Ok(());
        }
        self.sequence += 1;
        self.zone.apply_command(PLAYER, self.sequence, command)
    }

    /// Forgets held movement: standing still.
    pub fn stop(&mut self) {
        self.zone.update_player(PLAYER, |player| {
            player.forward = Axis::Zero;
            player.strafe = Axis::Zero;
            player.rise = Axis::Zero;
        });
    }

    /// Runs one tick of the zone's own movement and physics.
    ///
    /// # Errors
    /// Fails when the private zone does.
    pub fn step(&mut self) -> Result<(), ZoneError> {
        self.zone.advance_tick()
    }

    /// The tick the prediction stands at.
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.zone.tick
    }

    /// The predicted centre in exact units, and the facing.
    #[must_use]
    pub fn position(&self) -> Option<([f32; 3], u16)> {
        let (position, _) = self.zone.player_motion(PLAYER)?;
        let facing = self.zone.players.get(&PLAYER)?.facing;
        Some((position, facing))
    }
}
