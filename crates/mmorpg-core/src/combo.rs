//! The Warden's first combo (#157, Atlas 5.2.39–5.2.51): a directed Heroic
//! Strike that deals positive health damage to a opponent opens a
//! follow-up for the existing Shield Bash on exactly that opponent.
//!
//! - **Confirmation:** only the directed (mouse-aimed) Heroic Strike, and only
//!   when the struck opponent actually loses health after absorbs (5.2.49).
//!   A miss, a whiff or a fully absorbed blow opens nothing and clears an
//!   earlier opening.
//! - **Phases:** the committed impact lasts [`COMBO_IMPACT_TICKS`]; a Shield
//!   Bash requested meanwhile waits in the queue (the latest-valid buffer,
//!   5.2.41). The follow-up window then stays open for
//!   [`COMBO_WINDOW_TICKS`]; afterwards the opening expires.
//! - **Follow-up:** a Shield Bash resolving while the window is open targets
//!   the struck opponent whatever is selected (5.2.47), may ignore only the
//!   global cooldown the slash started (5.2.51) and otherwise obeys every
//!   Shield Bash rule: unlock, Rage, its own cooldown, range, line of sight
//!   and refusals. It consumes the opening once and starts its own global
//!   cooldown. Any other ability use or death clears the opening.
//!
//! The timing, conditions, effects and interruptions are the zone's
//! [`crate::action`] graph's `SlashToShieldBash` edge; this module keeps the
//! opening's runtime state. [`ComboOpening::is_open`] and
//! [`ComboOpening::is_valid`] use the baseline window.

use crate::zone::PlayerState;
use crate::{EntityRef, ZoneSimulation, action};

/// Committed impact after a confirming slash: the action graph's
/// slash → Shield Bash opening offset, 0.2 s.
pub const COMBO_IMPACT_TICKS: u16 = crate::action::SLASH_TO_SHIELD_BASH.open_after_ticks();
/// The Shield Bash follow-up window after the impact: the edge's actionable
/// duration, 0.6 s.
pub const COMBO_WINDOW_TICKS: u16 = crate::action::SLASH_TO_SHIELD_BASH.actionable_ticks();

/// A confirmed slash's follow-up opening.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComboOpening {
    /// The eligible unit the slash damaged; the follow-up Bash may hit only it.
    pub target: EntityRef,
    /// Ticks since the slash, at most impact + window.
    pub elapsed: u16,
}

impl ComboOpening {
    #[must_use]
    pub const fn is_open(self) -> bool {
        self.elapsed >= COMBO_IMPACT_TICKS
    }

    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.elapsed <= COMBO_IMPACT_TICKS + COMBO_WINDOW_TICKS
    }
}

/// The viewer's combo phase, as the projection shows it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComboPhase {
    /// The slash is still landing; a Shield Bash waits.
    Impact,
    /// Shield Bash now follows up on the struck opponent.
    Open,
}

impl ComboPhase {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Impact => 1,
            Self::Open => 2,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Impact),
            2 => Some(Self::Open),
            _ => None,
        }
    }
}

impl ZoneSimulation {
    /// The viewer's combo phase, if a follow-up is pending.
    pub(crate) fn combo_phase(&self, state: &PlayerState) -> Option<ComboPhase> {
        let graph = self.action_graph;
        let def = graph.edge(action::EdgeId::SlashToShieldBash)?;
        let opens = action::effective_window(state, graph, def).opens;
        state.combo.map(|combo| {
            if combo.elapsed >= opens {
                ComboPhase::Open
            } else {
                ComboPhase::Impact
            }
        })
    }

    /// Applies the reactions' cancellations (a stun discards a guard held by
    /// the strike recovery, in the baseline), then ages every opening by one
    /// tick: an expired or cancelled opening (death, in the baseline) ends
    /// and a suspended one keeps its age (Atlas 5.8.4).
    pub(crate) fn advance_combos(&mut self) {
        self.apply_interruptions();
        let graph = self.action_graph;
        let def = graph.edge(action::EdgeId::SlashToShieldBash);
        for player in self.players.values_mut() {
            let consequence = action::consequence(
                graph,
                player,
                action::Interrupted::Edge(action::EdgeId::SlashToShieldBash),
            );
            let window = def.map(|def| action::effective_window(player, graph, def));
            player.strike_recovery = player.strike_recovery.saturating_sub(1);
            player.combo = match (player.combo, window, consequence) {
                (Some(combo), Some(window), action::Consequence::Preserve) => {
                    let aged = ComboOpening {
                        elapsed: combo.elapsed + 1,
                        ..combo
                    };
                    (aged.elapsed <= window.closes()).then_some(aged)
                }
                (Some(combo), Some(_), action::Consequence::Suspend) => Some(combo),
                _ => None,
            };
        }
    }
}
