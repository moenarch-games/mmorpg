//! A Redbrand signaler's horn call for help (#952): target priority in a small pack.
//!
//! Content binds one creature spawn as a caller and one already-authored spawn of its family as
//! its partner ([`crate::ZoneContent::with_signal_calls`]). Engaged, the caller stands and
//! sounds the Signal Horn, a visible 2.5 s cast, while its partner idles out of aggro and assist
//! reach. Completed, the call engages the partner on the caller's threat leader through the
//! ordinary engagement, so it runs in, fights, leashes and evades like any creature; nothing is
//! summoned. A player answers it by interrupting the cast with an existing stun (Shield Bash,
//! Gouge, …), which puts the call off for its whole cooldown, or by killing the caller first.
//! Only an idle, living partner is worth a call, so one call brings at most one helper.

use crate::snapshot::CreatureAi;
use crate::{CreatureId, EntityRef, ZoneError, ZoneEvent, ZoneSimulation};

/// One signaler and the partner its call brings (#952).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SignalCall {
    pub caller: CreatureId,
    pub partner: CreatureId,
}

/// A partner stands at most this far from its caller's spawn (25 m): near enough to hear.
pub const SIGNAL_REACH_UNITS: i32 = 2_500;

impl ZoneSimulation {
    /// Whether `caller`'s call would bring anyone: its partner lives and idles.
    pub(crate) fn signal_partner_waits(&self, caller: CreatureId) -> bool {
        self.content
            .signal_call(caller)
            .and_then(|call| self.creatures.get(&call.partner))
            .is_some_and(|partner| {
                partner.is_alive() && matches!(partner.ai, CreatureAi::Idle { .. })
            })
    }

    /// `caller`'s completed call: its partner engages the caller's threat leader. A caller
    /// without a leader, or a partner already busy or dead, brings nobody.
    pub(crate) fn complete_signal_call(&mut self, caller: CreatureId) -> Result<(), ZoneError> {
        let source = EntityRef::Creature(caller);
        let leader = self
            .creatures
            .get(&caller)
            .and_then(crate::creature::CreatureState::top_threat);
        let partner = self
            .content
            .signal_call(caller)
            .map(|call| call.partner)
            .filter(|_| self.signal_partner_waits(caller));
        self.notify_units(
            [Some(source), None],
            ZoneEvent::AbilityUsed {
                source,
                target: partner.map(EntityRef::Creature),
                ability: crate::ability::ids::SIGNAL_HORN,
            },
        );
        if let (Some(leader), Some(partner)) = (leader, partner) {
            self.engage(partner, leader, false)?;
        }
        Ok(())
    }
}
