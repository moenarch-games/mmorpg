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
//! - **Cleave follow-up (#910):** a timed swing that damages two or more
//!   units ([`ComboOpening::hits`]) also opens the `CleaveToSlash` edge in the
//!   same window: the next Warden melee use may ignore the slash's global
//!   cooldown, so the Warden can swing again into the group. The opening is
//!   single-use: the Bash or the follow-up swing consumes it.
//!   Both edges share that one window: graph validation refuses a
//!   configuration (baseline, modifier or override) that times them apart
//!   (#1014), since the Shield Bash edge times the shared opening.
//!
//! The timing, conditions, effects and interruptions are the zone's
//! [`crate::action`] graph's `SlashToShieldBash` and `CleaveToSlash` edges;
//! this module keeps the opening's runtime state. [`ComboOpening::is_open`]
//! and [`ComboOpening::is_valid`] use the baseline window.

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
    /// The units the slash damaged, 1 through [`crate::SWING_HITS`] (#910).
    pub hits: u8,
}

impl ComboOpening {
    #[must_use]
    pub const fn is_open(self) -> bool {
        self.elapsed >= COMBO_IMPACT_TICKS
    }

    /// Within the window, or the late-input grace after it (#415).
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.elapsed
            <= COMBO_IMPACT_TICKS + COMBO_WINDOW_TICKS + action::LATE_INPUT_GRACE_TICKS as u16
            && self.hits >= 1
            && self.hits as usize <= crate::SWING_HITS
    }

    /// Whether the slash damaged several units (#910).
    #[must_use]
    pub const fn cleaved(self) -> bool {
        self.hits >= 2
    }
}

/// The viewer's combo phase, as the projection shows it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComboPhase {
    /// The slash is still landing; a Shield Bash waits.
    Impact,
    /// Shield Bash now follows up on the struck opponent.
    Open,
    /// A cleaving slash is still landing; Shield Bash or another swing waits
    /// (#910).
    CleaveImpact,
    /// Shield Bash on the first struck opponent, or another swing free of the
    /// slash's global cooldown, now follows up (#910).
    CleaveOpen,
}

impl ComboPhase {
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Impact => 1,
            Self::Open => 2,
            Self::CleaveImpact => 3,
            Self::CleaveOpen => 4,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Impact),
            2 => Some(Self::Open),
            3 => Some(Self::CleaveImpact),
            4 => Some(Self::CleaveOpen),
            _ => None,
        }
    }

    /// Whether the follow-up is open now.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open | Self::CleaveOpen)
    }
}

impl ZoneSimulation {
    /// The viewer's combo phase, if a follow-up is pending.
    pub(crate) fn combo_phase(&self, state: &PlayerState) -> Option<ComboPhase> {
        let graph = self.action_graph;
        let def = graph.edge(action::EdgeId::SlashToShieldBash)?;
        let window = action::effective_window(state, graph, def);
        // The cleave follow-up shows only where the graph authors it (#910).
        let cleave = graph.edge(action::EdgeId::CleaveToSlash).is_some();
        // An opening kept only for late inputs (#415) is over for the viewer.
        state
            .combo
            .filter(|combo| combo.elapsed <= window.closes())
            .map(
                |combo| match (combo.elapsed >= window.opens, cleave && combo.cleaved()) {
                    (true, false) => ComboPhase::Open,
                    (false, false) => ComboPhase::Impact,
                    (true, true) => ComboPhase::CleaveOpen,
                    (false, true) => ComboPhase::CleaveImpact,
                },
            )
    }

    /// Applies the reactions' cancellations (a stun discards a guard held by
    /// the strike recovery, in the baseline), then ages every opening by one
    /// tick: an expired or cancelled opening (death, in the baseline) ends
    /// and a suspended one keeps its age (Atlas 5.8.4). An expired opening is
    /// kept for [`action::LATE_INPUT_GRACE_TICKS`] after its window closes,
    /// only so a late input can still answer it (#415).
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
                    let kept = window.closes() + u16::from(action::LATE_INPUT_GRACE_TICKS);
                    (aged.elapsed <= kept).then_some(aged)
                }
                (Some(combo), Some(_), action::Consequence::Suspend) => Some(combo),
                _ => None,
            };
        }
    }
}
