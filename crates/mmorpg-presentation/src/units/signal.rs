//! A Redbrand signaler's horn call (#952): the Ranger figure's `signal` clips (asset-tooling
//! #774), played by the Redbrand figures by joint name, and the signal horn on the weapon hand.
//! Presentation only: the call is the zone's Signal Horn cast; its start, completion and
//! interruption arrive as zone events.
//!
//! - **Start:** the horn comes to the lips ([`START`], at 0.5 s), then the call loops ([`CALL`])
//!   until the cast resolves. The sword rides the hip while the horn is in hand.
//! - **Completed:** the horn comes down ([`END`]) and is put away when the clip ends.
//! - **Interrupted:** the blow knocks the horn loose ([`INTERRUPTED`]); it is gone at
//!   [`DROP_SECONDS`].

use super::chores::Chore;

pub const START: &str = "Signal_Call_Start";
pub const CALL: &str = "Signal_Call_Loop";
pub const INTERRUPTED: &str = "Signal_Call_Interrupted";
pub const END: &str = "Signal_Call_End";
/// The interrupted clip's `signal.dropSeconds`: the horn leaves the hand.
pub const DROP_SECONDS: f64 = 0.1;

/// The signal horn on the right hand's weapon socket, shown like a resident's chore item.
pub const HORN: Chore = Chore {
    item: "equipment.tool.signal-horn",
    socket: "hand_r.weapon",
    idle: None,
    walk: None,
};

/// How long a signaler holds the horn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Horn {
    /// While the call runs.
    Calling,
    /// Until this presentation time, as the call ends or is broken.
    Until(f64),
}

impl Horn {
    /// Whether the horn is in hand at `now`.
    #[must_use]
    pub fn held(self, now: f64) -> bool {
        match self {
            Self::Calling => true,
            Self::Until(until) => now < until,
        }
    }
}
