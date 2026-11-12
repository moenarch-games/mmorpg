//! Predicted own movement, reconciled against `acknowledged_sequence` (#680, Atlas 5.9.i680):
//! the second arena netcode option, chosen per client.
//!
//! The client moves its own body at once with [`mmorpg_core::MovementPredictor`] (the zone's
//! own movement rules and physics) and keeps every command it sent until the zone
//! acknowledges it. Each projection then restarts the prediction from the zone's answer for
//! the own body and replays the movement sent since, so the client never drifts from the
//! zone: whatever the zone decided (a collision with another unit, a stun, a knockback) wins
//! within one round trip.
//!
//! Time on the two sides is matched by acknowledgements alone. A command sent after local
//! tick `l` moves the prediction from tick `l + 1`; the first projection that acknowledges it,
//! at zone tick `t`, is the tick the zone moved it. Their difference maps every later zone
//! tick onto the prediction's own timeline, so nothing but the zone's own tick numbers and
//! sequences crosses the wire.

use std::collections::VecDeque;
use std::sync::Arc;

use mmorpg_core::{MovementPredictor, ZoneCommand, ZoneContent, ZoneError, ZoneId, ZoneSnapshot};

/// The most ticks one reconciliation replays; a longer gap restarts from the zone's answer.
pub const MAX_REPLAY_TICKS: u64 = 120;

/// A command the zone has not acknowledged yet.
#[derive(Clone, Copy, Debug)]
struct Sent {
    sequence: u32,
    /// The local tick it was sent after.
    after: u64,
    /// Its movement, if it moves the body.
    movement: Option<ZoneCommand>,
}

/// One client's predicted own body (#680).
pub struct PredictedMovement {
    predictor: MovementPredictor,
    sent: VecDeque<Sent>,
    /// The newest acknowledged `Move` and `Rise`: the zone's held movement.
    held: [Option<ZoneCommand>; 2],
    acknowledged: u32,
    /// Local tick minus zone tick for the same moment of the prediction.
    offset: i64,
    /// Local ticks run.
    local: u64,
}

const fn is_movement(command: ZoneCommand) -> bool {
    matches!(
        command,
        ZoneCommand::Move { .. } | ZoneCommand::Rise(_) | ZoneCommand::Jump | ZoneCommand::Roll
    )
}

impl PredictedMovement {
    /// A prediction over `content`, the content the zone runs.
    ///
    /// # Errors
    /// Fails when the content cannot host a player.
    pub fn new(zone_id: ZoneId, content: Arc<ZoneContent>) -> Result<Self, ZoneError> {
        Ok(Self {
            predictor: MovementPredictor::new(zone_id, content)?,
            sent: VecDeque::new(),
            held: [None, None],
            acknowledged: 0,
            offset: 0,
            local: 0,
        })
    }

    /// The client sent `command` under `sequence`: movement moves the prediction from the
    /// next tick on.
    ///
    /// # Errors
    /// Fails for malformed movement.
    pub fn sent(&mut self, sequence: u32, command: ZoneCommand) -> Result<(), ZoneError> {
        let movement = is_movement(command).then_some(command);
        if let Some(movement) = movement {
            self.predictor.apply(movement)?;
        }
        self.sent.push_back(Sent {
            sequence,
            after: self.local,
            movement,
        });
        Ok(())
    }

    /// Runs one local tick of the prediction.
    ///
    /// # Errors
    /// Fails when the private zone does.
    pub fn step(&mut self) -> Result<(), ZoneError> {
        self.local += 1;
        self.predictor.step()
    }

    /// A projection at zone tick `tick` acknowledged commands up to `acknowledged`; call it
    /// for every projection, in order, so the first acknowledgement of each command dates it.
    pub fn acknowledge(&mut self, tick: u64, acknowledged: u32) {
        if acknowledged <= self.acknowledged {
            return;
        }
        self.acknowledged = acknowledged;
        while let Some(sent) = self.sent.front().copied() {
            if sent.sequence > acknowledged {
                break;
            }
            self.sent.pop_front();
            #[allow(clippy::cast_possible_wrap)]
            {
                self.offset = (sent.after + 1) as i64 - tick as i64;
            }
            match sent.movement {
                Some(command @ ZoneCommand::Move { .. }) => self.held[0] = Some(command),
                Some(command @ ZoneCommand::Rise(_)) => self.held[1] = Some(command),
                _ => {}
            }
        }
    }

    /// Restarts the prediction from the zone's own body at zone tick `tick` (centre in
    /// units, velocity in whole units per tick, facing) and replays the movement sent since.
    ///
    /// # Errors
    /// Fails when the private zone does.
    pub fn reconcile(
        &mut self,
        tick: u64,
        centre: [i32; 3],
        velocity: [i32; 3],
        facing: u16,
    ) -> Result<(), ZoneError> {
        self.predictor.reset(tick, centre, velocity, facing)?;
        self.predictor.stop();
        for command in self.held.into_iter().flatten() {
            self.predictor.apply(command)?;
        }
        // The zone had not applied the oldest unacknowledged command by `tick`, so `tick` is at
        // most the local tick it was sent after: a bound before the first acknowledgement and
        // when the delay grew since the last one.
        #[allow(clippy::cast_possible_wrap)]
        let offset = self.sent.front().map_or(self.offset, |sent| {
            self.offset.min(sent.after as i64 - tick as i64)
        });
        #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
        let from = (tick as i64 + offset).max(0) as u64;
        let ticks = self.local.saturating_sub(from).min(MAX_REPLAY_TICKS);
        let start = self.local - ticks;
        let mut pending = self
            .sent
            .iter()
            .filter_map(|sent| Some((sent.after, sent.movement?)))
            .peekable();
        for local in start..self.local {
            while let Some((_, command)) = pending.next_if(|(after, _)| *after <= local) {
                self.predictor.apply(command)?;
            }
            self.predictor.step()?;
        }
        for (_, command) in pending {
            self.predictor.apply(command)?;
        }
        Ok(())
    }

    /// [`Self::acknowledge`] and [`Self::reconcile`] with `snapshot`, the newest projection,
    /// whose own body it reads; nothing when the projection does not show it.
    ///
    /// # Errors
    /// Fails when the private zone does.
    pub fn reconcile_snapshot(&mut self, snapshot: &ZoneSnapshot) -> Result<(), ZoneError> {
        self.acknowledge(snapshot.tick, snapshot.acknowledged_sequence);
        let Some(own) = snapshot.entities.iter().find(|entity| {
            entity.kind == mmorpg_core::EntityKind::Player && entity.id == snapshot.viewer_id
        }) else {
            return Ok(());
        };
        self.reconcile(
            snapshot.tick,
            own.position,
            own.velocity.map(i32::from),
            own.facing,
        )
    }

    /// The predicted centre in exact units, and the facing.
    #[must_use]
    pub fn position(&self) -> Option<([f32; 3], u16)> {
        self.predictor.position()
    }

    /// Local ticks run so far.
    #[must_use]
    pub const fn local_tick(&self) -> u64 {
        self.local
    }
}
