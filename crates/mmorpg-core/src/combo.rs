//! The Warden's first combo (#157, Atlas 5.2.39–5.2.51): a directed Heroic
//! Strike that deals positive health damage to a creature opens a
//! follow-up for the existing Shield Bash on exactly that creature.
//!
//! - **Confirmation:** only the directed (mouse-aimed) Heroic Strike, and only
//!   when the struck creature actually loses health after absorbs (5.2.49).
//!   A miss, a whiff or a fully absorbed blow opens nothing and clears an
//!   earlier opening.
//! - **Phases:** the committed impact lasts [`COMBO_IMPACT_TICKS`]; a Shield
//!   Bash requested meanwhile waits in the queue (the latest-valid buffer,
//!   5.2.41). The follow-up window then stays open for
//!   [`COMBO_WINDOW_TICKS`]; afterwards the opening expires.
//! - **Follow-up:** a Shield Bash resolving while the window is open targets
//!   the struck creature whatever is selected (5.2.47), may ignore only the
//!   global cooldown the slash started (5.2.51) and otherwise obeys every
//!   Shield Bash rule: unlock, Rage, its own cooldown, range, line of sight
//!   and refusals. It consumes the opening once and starts its own global
//!   cooldown. Any other ability use or death clears the opening.

use crate::snapshot::PlayerIntent;
use crate::zone::PlayerState;
use crate::{EntityRef, ZoneSimulation, ability::ids};

/// Committed impact after a confirming slash: 0.2 s.
pub const COMBO_IMPACT_TICKS: u16 = 6;
/// The Shield Bash follow-up window after the impact: 0.6 s.
pub const COMBO_WINDOW_TICKS: u16 = 18;

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
    /// Shield Bash now follows up on the struck creature.
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

pub(crate) const fn is_shield_bash(intent: &PlayerIntent) -> bool {
    matches!(intent, PlayerIntent::UseAbility { ability, .. } if *ability == ids::SHIELD_BASH.get())
}

/// A Shield Bash requested during the committed impact waits for the window.
pub(crate) fn waits_for_window(state: &PlayerState, intent: &PlayerIntent) -> bool {
    is_shield_bash(intent) && state.combo.is_some_and(|combo| !combo.is_open())
}

impl ZoneSimulation {
    /// The viewer's combo phase, if a follow-up is pending.
    pub(crate) fn combo_phase(state: &PlayerState) -> Option<ComboPhase> {
        state.combo.map(|combo| {
            if combo.is_open() {
                ComboPhase::Open
            } else {
                ComboPhase::Impact
            }
        })
    }

    /// Ages every opening by one tick and drops expired ones and those of
    /// the dead.
    pub(crate) fn advance_combos(&mut self) {
        for player in self.players.values_mut() {
            // A stun before the strike recovery ends discards a pending guard.
            if player.strike_recovery > 0 {
                player.strike_recovery -= 1;
                if crate::aura::has_kind(&player.auras, crate::AuraKind::Stun) {
                    player.guard.raised = None;
                }
            }
            player.combo = player.combo.and_then(|combo| {
                let aged = ComboOpening {
                    elapsed: combo.elapsed + 1,
                    ..combo
                };
                (player.health > 0 && aged.is_valid()).then_some(aged)
            });
        }
    }
}
