//! The movement outbox: which movement commands the current input needs.
//!
//! Held movement and swim rise go out when they change, facing-only changes
//! (camera drags) at most once per server tick, and the current intent is
//! resent periodically so a lost datagram, a key release included, is
//! corrected. Jumps and dodge rolls are press counters, so coalesced input
//! can merge presses but never replay them. While the player is dead nothing
//! held moves the released spirit: the intent is zero at the last sent facing
//! and presses are dropped. It performs no I/O and allocates nothing.
use mmorpg_core::{TICK_HZ, ZoneCommand};

/// Periodic resend of the current intent.
pub const RESEND_INTERVAL_MS: f64 = 50.0;
/// Facing-only changes go out at most once per server tick.
pub const FACING_INTERVAL_MS: f64 = 1_000.0 / TICK_HZ as f64;

/// Local movement intent and press counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MovementInput {
    pub forward: i8,
    pub strafe: i8,
    pub facing: u16,
    pub jumps: u32,
    /// Dodge-roll presses (#516).
    pub rolls: u32,
    /// Held swim ascent (`1`), descent (`-1`) or neither (#274).
    pub rise: i8,
}

/// The movement commands for one observation, sent in field order: a jump,
/// the swim rise, a move, then a roll (after the move, so the zone rolls along
/// the latest intent).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MovementCommands {
    pub jump: bool,
    pub rise: Option<i8>,
    /// Forward, strafe and facing.
    pub movement: Option<(i8, i8, u16)>,
    pub roll: bool,
}

impl MovementCommands {
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.jump && self.rise.is_none() && self.movement.is_none() && !self.roll
    }

    /// The commands in send order.
    pub fn commands(self) -> impl Iterator<Item = ZoneCommand> {
        [
            self.jump.then_some(ZoneCommand::Jump),
            self.rise.map(ZoneCommand::Rise),
            self.movement
                .map(|(forward, strafe, facing)| ZoneCommand::Move {
                    forward,
                    strafe,
                    facing,
                }),
            self.roll.then_some(ZoneCommand::Roll),
        ]
        .into_iter()
        .flatten()
    }
}

/// Tracks what was last sent; times are milliseconds on any monotonic clock.
#[derive(Clone, Copy, Debug)]
pub struct MovementOutbox {
    sent: Option<(i8, i8, u16)>,
    sent_at: f64,
    jumps: u32,
    rolls: u32,
    /// The swim rise last sent; `None` after a resume, which resends only a held one.
    sent_rise: Option<i8>,
}

impl MovementOutbox {
    #[must_use]
    pub const fn new(input: MovementInput) -> Self {
        Self {
            sent: None,
            sent_at: f64::NEG_INFINITY,
            jumps: input.jumps,
            rolls: input.rolls,
            sent_rise: Some(0),
        }
    }

    /// A new connection: only current input counts, earlier presses are dropped.
    pub const fn resume(&mut self, input: MovementInput) {
        *self = Self::new(input);
        self.sent_rise = None;
    }

    /// Commands for the current input. `dead` comes from the latest
    /// projection; `aimed` sends a facing change at once, because a swing or
    /// ability just went out toward it (#517).
    pub fn update(
        &mut self,
        held: MovementInput,
        now_ms: f64,
        dead: bool,
        aimed: bool,
    ) -> MovementCommands {
        let input = self.intent(held, dead);
        let jumped = input.jumps != self.jumps;
        let rolled = input.rolls != self.rolls;
        self.jumps = input.jumps;
        self.rolls = input.rolls;
        let elapsed = now_ms - self.sent_at;
        let intent_changed = self
            .sent
            .is_none_or(|(forward, strafe, _)| (forward, strafe) != (input.forward, input.strafe));
        let facing_due = self
            .sent
            .is_none_or(|(_, _, facing)| facing != input.facing)
            && (aimed || elapsed >= FACING_INTERVAL_MS);
        let resend = elapsed >= RESEND_INTERVAL_MS;
        MovementCommands {
            jump: jumped && !dead,
            rise: self.rise(input.rise),
            movement: (intent_changed || facing_due || resend).then(|| self.sent(input, now_ms)),
            roll: rolled && !dead,
        }
    }

    /// The periodic resend of the current intent and a changed swim rise.
    /// Pending presses are left to [`Self::update`], so none is ever sent twice.
    pub fn heartbeat(&mut self, held: MovementInput, now_ms: f64, dead: bool) -> MovementCommands {
        let input = self.intent(held, dead);
        MovementCommands {
            rise: self.rise(input.rise),
            movement: Some(self.sent(input, now_ms)),
            ..MovementCommands::default()
        }
    }

    /// The intent to send: the dead hold nothing and keep the last sent facing.
    fn intent(&self, held: MovementInput, dead: bool) -> MovementInput {
        if dead {
            MovementInput {
                forward: 0,
                strafe: 0,
                facing: self.sent.map_or(held.facing, |(_, _, facing)| facing),
                rise: 0,
                ..held
            }
        } else {
            held
        }
    }

    fn rise(&mut self, rise: i8) -> Option<i8> {
        let sent = self.sent_rise.unwrap_or(0);
        let resumed = self.sent_rise.is_none();
        self.sent_rise = Some(rise);
        (sent != rise || (resumed && rise != 0)).then_some(rise)
    }

    fn sent(&mut self, input: MovementInput, now_ms: f64) -> (i8, i8, u16) {
        let movement = (input.forward, input.strafe, input.facing);
        self.sent = Some(movement);
        self.sent_at = now_ms;
        movement
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: f64 = FACING_INTERVAL_MS;

    fn held(forward: i8, strafe: i8, facing: u16) -> MovementInput {
        MovementInput {
            forward,
            strafe,
            facing,
            ..MovementInput::default()
        }
    }

    fn only_move(forward: i8, strafe: i8, facing: u16) -> MovementCommands {
        MovementCommands {
            movement: Some((forward, strafe, facing)),
            ..MovementCommands::default()
        }
    }

    #[test]
    fn the_first_input_moves_and_unchanged_input_waits_for_the_resend() {
        let mut outbox = MovementOutbox::new(held(0, 0, 0));
        assert_eq!(
            outbox.update(held(1, 0, 0), 0.0, false, false),
            only_move(1, 0, 0)
        );
        assert!(outbox.update(held(1, 0, 0), 10.0, false, false).is_empty());
        assert_eq!(
            outbox.update(held(1, 0, 0), RESEND_INTERVAL_MS, false, false),
            only_move(1, 0, 0)
        );
        // A changed intent goes out at once.
        assert_eq!(
            outbox.update(held(0, -1, 0), RESEND_INTERVAL_MS + 1.0, false, false),
            only_move(0, -1, 0)
        );
    }

    #[test]
    fn facing_only_changes_go_out_once_per_tick_unless_aimed() {
        let mut outbox = MovementOutbox::new(held(0, 0, 0));
        outbox.update(held(0, 0, 0), 0.0, false, false);
        assert!(
            outbox
                .update(held(0, 0, 100), TICK / 2.0, false, false)
                .is_empty()
        );
        assert_eq!(
            outbox.update(held(0, 0, 200), TICK / 2.0, false, true),
            only_move(0, 0, 200)
        );
        assert!(
            outbox
                .update(held(0, 0, 300), TICK, false, false)
                .is_empty()
        );
        assert_eq!(
            outbox.update(held(0, 0, 300), 1.5 * TICK + 1.0, false, false),
            only_move(0, 0, 300)
        );
    }

    #[test]
    fn presses_send_once_around_the_move_however_many_merged() {
        let mut outbox = MovementOutbox::new(held(0, 0, 0));
        outbox.update(held(0, 0, 0), 0.0, false, false);
        let pressed = MovementInput {
            jumps: 3,
            rolls: 1,
            ..held(1, 0, 0)
        };
        let commands = outbox.update(pressed, 1.0, false, false);
        assert_eq!(
            commands.commands().collect::<Vec<_>>(),
            [
                ZoneCommand::Jump,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing: 0
                },
                ZoneCommand::Roll,
            ]
        );
        assert!(outbox.update(pressed, 2.0, false, false).is_empty());
        // The heartbeat never repeats a press.
        assert_eq!(outbox.heartbeat(pressed, 3.0, false), only_move(1, 0, 0));
    }

    #[test]
    fn the_swim_rise_goes_out_on_each_change_and_a_held_one_after_a_resume() {
        let mut outbox = MovementOutbox::new(held(0, 0, 0));
        outbox.update(held(0, 0, 0), 0.0, false, false);
        let rising = MovementInput {
            rise: 1,
            ..held(0, 0, 0)
        };
        assert_eq!(outbox.update(rising, 1.0, false, false).rise, Some(1));
        assert_eq!(outbox.update(rising, 2.0, false, false).rise, None);
        outbox.resume(rising);
        assert_eq!(outbox.heartbeat(rising, 3.0, false).rise, Some(1));
        outbox.resume(held(0, 0, 0));
        assert_eq!(outbox.heartbeat(held(0, 0, 0), 4.0, false).rise, None);
    }

    #[test]
    fn a_resume_drops_presses_from_the_outage() {
        let mut outbox = MovementOutbox::new(held(0, 0, 0));
        outbox.update(held(0, 0, 0), 0.0, false, false);
        let pressed = MovementInput {
            jumps: 2,
            ..held(0, 0, 0)
        };
        outbox.resume(pressed);
        let commands = outbox.update(pressed, 1.0, false, false);
        assert!(!commands.jump);
        assert_eq!(
            commands.movement,
            Some((0, 0, 0)),
            "the intent goes out afresh"
        );
    }

    #[test]
    fn the_dead_send_no_held_movement_and_drop_presses() {
        let mut outbox = MovementOutbox::new(held(0, 0, 0));
        outbox.update(held(1, 0, 500), 0.0, false, false);
        let pressed = MovementInput {
            jumps: 1,
            rolls: 1,
            rise: 1,
            ..held(1, 1, 900)
        };
        assert_eq!(
            outbox.update(pressed, 1.0, true, false),
            only_move(0, 0, 500)
        );
        assert!(outbox.update(pressed, 2.0, true, false).is_empty());
        // Alive again: the presses made while dead stay dropped.
        let alive = outbox.update(pressed, 3.0, false, false);
        assert!(!alive.jump && !alive.roll);
        assert_eq!(alive.rise, Some(1));
        assert_eq!(alive.movement, Some((1, 1, 900)));
    }
}
