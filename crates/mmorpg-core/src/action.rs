//! The authoritative parallel action graph (#380, Atlas 5.7–5.8).
//!
//! One inspectable model decides how a player's concurrent actions interact.
//! The character is split into four **regions**: movement, offense, defense
//! and reaction. Each region runs at most one [`ActionId`] at a time, so
//! combinations are never enumerated as one state per product. The data says
//! how they meet:
//!
//! - [`RULES`]: **compatibility rules.** An action in its recovery phase
//!   *holds* (buffers) an input aimed at another region. For example,
//!   landing recovery holds Warden melee, and slash recovery holds guard
//!   protection.
//! - [`EDGES`]: **directed edges.** An edge leads from an action to a
//!   follow-up input. Each edge has an authoritative [`Anchor`], an opening
//!   offset, an actionable duration, a bounded early buffer, typed
//!   [`Condition`]s, typed [`Effect`]s, an optional nested [`Precision`]
//!   window with its own typed benefits, and a priority. [`WindowModifier`]s
//!   enlarge or reduce an edge's actionable duration under typed conditions.
//! - [`INTERRUPTIONS`]: **phase-aware interruptions** (Atlas 5.8.4). A
//!   reaction (stun, guard break, death) preserves, suspends or cancels a
//!   running action, an edge's opening or a held input, optionally only in
//!   one phase of it. Without an authored rule the reaction preserves it.
//!   Hits that already resolved stay resolved.
//! - **Arbitration.** [`ZoneSimulation::arbitrate_input`] compares every
//!   edge that answers an input with the input's direct use. The open edge
//!   with the highest priority wins. A buffering (or suspended) edge with
//!   higher priority than the direct use holds the input until it opens.
//!   Waiting inputs stay in the intent queue, so the latest valid input
//!   resolves (5.2.41). Every candidate gets a [`Verdict`] that says why it
//!   won or was refused, and a win inside the precision window says so.
//!
//! Durations are authored in milliseconds and converted once through
//! [`crate::ticks_from_ms`], so they keep their meaning when `TICK_HZ`
//! changes (#379). [`BASELINE`] is the Warden's current gameplay
//! (5.2.37–5.2.51, 5.2.69–5.2.70) expressed as data, with unchanged values:
//! it has no precision window, no window modifier and no effect beyond the
//! combo's confirmed target and global-cooldown waiver. A zone runs another
//! validated [`ActionGraph`] only through [`ZoneSimulation::set_action_graph`],
//! which tests and the lab use; it is zone setup, not canonical state, and a
//! zone restored from a snapshot runs the baseline.
//!
//! Runtime state stays in the existing canonical fields: `landing` for the
//! movement region, `strike_recovery` and `combo` for the offense region,
//! `guard` for the defense region, and auras, the guard break and health for
//! the reaction region. [`ZoneSimulation::action_inspection`] is the
//! read-only query surface that the Action Lab (#381) reads.

use crate::snapshot::PlayerIntent;
use crate::zone::PlayerState;
use crate::{AuraKind, EntityRef, PlayerClass, PlayerId, ZoneSimulation, ability::ids, aura};

/// A concurrent part of a character's action state (Atlas 5.7.5).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Region {
    Movement,
    Offense,
    Defense,
    Reaction,
}

/// An action a region can run.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ActionId {
    /// Not grounded after the last physics step.
    Airborne,
    /// Touchdown recovery.
    Landing,
    /// The Warden's directed (mouse-aimed) Heroic Strike.
    DirectedSlash,
    /// A held shield guard lane.
    Guard,
    /// A stun aura.
    Stun,
    /// A broken guard recovering.
    GuardBreak,
    /// Dead.
    Death,
}

/// The phase of a running action.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Phase {
    /// Running with no committed recovery.
    Active,
    /// Committed recovery; compatibility rules apply.
    Recovery,
    /// Recovery is over, but an outgoing edge is still pending.
    FollowUp,
}

/// An authored action: its region and committed recovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionDef {
    pub id: ActionId,
    pub region: Region,
    /// Committed recovery in ms; `None` when the action has none of its own.
    pub recovery_ms: Option<u32>,
}

/// A typed change of an action's recovery under a condition (Atlas 5.7.3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryModifier {
    pub action: ActionId,
    pub condition: Condition,
    pub recovery_ms: u32,
}

/// What an input asks for, as far as the graph is concerned.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ActionInput {
    /// A Warden melee action other than Shield Bash: auto-attack swing,
    /// Heroic Strike (ordinary or directed) or Cleave.
    WardenMelee,
    /// Shield Bash, which is also Warden melee.
    ShieldBash,
    /// The held guard protecting.
    GuardProtection,
}

/// The authoritative event a window is measured from (Atlas 5.8.2).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Anchor {
    /// The strike's impact.
    Impact,
    /// The strike's confirmed result (resolved in the same tick as the impact).
    ConfirmedResult,
    /// Touchdown after being airborne.
    Landing,
    /// The start of the source action's phase.
    PhaseStart,
    /// The end of the source action's phase.
    PhaseEnd,
}

/// An edge's timing, measured from its anchor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    /// The edge opens this long after the anchor.
    pub open_after_ms: u32,
    /// After opening, it stays open this long. The closing tick is still
    /// actionable.
    pub actionable_ms: u32,
    /// An input this long before opening waits for the opening; earlier
    /// inputs are not buffered by the edge.
    pub buffer_ms: u32,
}

impl Window {
    #[must_use]
    pub const fn open_after_ticks(self) -> u16 {
        crate::ticks_from_ms(self.open_after_ms)
    }

    #[must_use]
    pub const fn actionable_ticks(self) -> u16 {
        crate::ticks_from_ms(self.actionable_ms)
    }

    #[must_use]
    pub const fn buffer_ticks(self) -> u16 {
        crate::ticks_from_ms(self.buffer_ms)
    }

    /// The window in ticks.
    #[must_use]
    pub const fn ticks(self) -> TickWindow {
        TickWindow {
            opens: self.open_after_ticks(),
            actionable: self.actionable_ticks(),
            buffer: self.buffer_ticks(),
        }
    }

    /// The window's phase `elapsed` ticks after its anchor: too early, buffering for the
    /// [`Self::buffer_ticks`] before it opens, open from [`Self::open_after_ticks`] through the
    /// closing tick [`Self::actionable_ticks`] later, then expired.
    #[must_use]
    pub const fn status_at(self, elapsed: u16) -> EdgeStatus {
        self.ticks().status_at(elapsed)
    }
}

/// A window in ticks since its anchor, after window modifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TickWindow {
    pub opens: u16,
    pub actionable: u16,
    pub buffer: u16,
}

impl TickWindow {
    /// The last actionable tick since the anchor.
    #[must_use]
    pub const fn closes(self) -> u16 {
        self.opens + self.actionable
    }

    /// See [`Window::status_at`].
    #[must_use]
    pub const fn status_at(self, elapsed: u16) -> EdgeStatus {
        if elapsed < self.opens.saturating_sub(self.buffer) {
            EdgeStatus::Blocked(BlockReason::TooEarly)
        } else if elapsed < self.opens {
            EdgeStatus::Buffering
        } else if elapsed <= self.closes() {
            EdgeStatus::Open
        } else {
            EdgeStatus::Blocked(BlockReason::Expired)
        }
    }
}

/// An optional sweet spot nested inside an edge's actionable interval
/// (Atlas 5.8.3). An input resolving inside it gets the typed benefits on
/// top of the edge's effects; inputs elsewhere in the interval stay valid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Precision {
    /// The sweet spot starts this long after the edge opens.
    pub after_open_ms: u32,
    /// It lasts this long; its closing tick is still precise.
    pub length_ms: u32,
    pub benefits: &'static [Effect],
}

impl Precision {
    /// Its first and last tick since the edge's anchor, for an edge opening
    /// at `opens`.
    #[must_use]
    pub const fn ticks(self, opens: u16) -> (u16, u16) {
        let from = opens + crate::ticks_from_ms(self.after_open_ms);
        (from, from + crate::ticks_from_ms(self.length_ms))
    }
}

/// A typed, validated condition (Atlas 5.7.3, 5.8.1).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Condition {
    /// The source strike's confirmed result dealt positive health damage
    /// after absorbs (5.2.49).
    PositiveHealthDamage,
    /// The confirmed target is still alive.
    TargetAlive,
    /// The acting player is alive.
    Alive,
    /// The acting player is a Warden.
    Warden,
    /// A melee action was made during the airborne phase that just ended.
    AttackedAirborne,
    /// Not grounded after the last physics step.
    Airborne,
    /// A stun aura is active.
    Stunned,
}

/// A typed effect an edge, or its precision window, applies when it wins
/// (Atlas 5.7.3, 5.8.3). Ordinary validation of the follow-up still applies.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Effect {
    /// The follow-up hits the source's confirmed target, whatever is selected
    /// (5.2.47).
    TargetConfirmed,
    /// The follow-up may ignore the global cooldown the source started
    /// (5.2.51).
    IgnoreGlobalCooldown,
    /// The follow-up's recovery (the global cooldown it starts) lasts this
    /// long instead: a faster or slower next action.
    Recovery { ms: u32 },
    /// Returns this much of the follow-up's resource cost, at most the cost.
    ResourceRefund { amount: u16 },
    /// The follow-up keeps the opening instead of consuming it, so the edge
    /// stays available for the rest of its window.
    KeepOpening,
}

/// Stable edge identities.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EdgeId {
    /// The first Warden combo, directed slash → Shield Bash (#157).
    SlashToShieldBash,
}

/// A directed, conditional transition (Atlas 5.7.2).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EdgeDef {
    pub id: EdgeId,
    pub from: ActionId,
    pub input: ActionInput,
    pub anchor: Anchor,
    pub window: Window,
    pub conditions: &'static [Condition],
    pub effects: &'static [Effect],
    pub precision: Option<Precision>,
    /// Higher wins. An input's direct use has priority 0.
    pub priority: u8,
}

/// A typed enlargement (positive) or reduction (negative) of an edge's
/// actionable duration while every condition holds (Atlas 5.7.3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowModifier {
    pub edge: EdgeId,
    pub conditions: &'static [Condition],
    pub actionable_delta_ms: i32,
}

/// Stable compatibility rule identities.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuleId {
    /// Warden melee waits out landing recovery (5.2.70).
    LandingHoldsMelee,
    /// A guard raised during slash recovery protects only afterwards
    /// (5.2.39, 5.2.41).
    SlashRecoveryHoldsGuard,
}

/// A region-compatibility rule (Atlas 5.7.5): while `during` recovers, the
/// input waits (is held) until the recovery ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompatibilityRule {
    pub id: RuleId,
    /// The action whose recovery phase the rule covers.
    pub during: ActionId,
    pub input: ActionInput,
    pub conditions: &'static [Condition],
}

/// What an interruption acts on.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Interrupted {
    /// A running action.
    Action(ActionId),
    /// An edge's pending opening (its source action has resolved).
    Edge(EdgeId),
    /// The inputs a compatibility rule holds (the buffer).
    Held(RuleId),
}

/// What a reaction does to an interrupted action, opening or buffer.
/// Stronger consequences win: cancel over suspend over preserve.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Consequence {
    /// Keeps running unchanged.
    Preserve,
    /// Pauses: a guard stops protecting but stays held; an edge's clock
    /// stops and it refuses inputs, still holding a buffered one.
    Suspend,
    /// Ends it: a guard is lowered, an opening discarded (a Bash buffered
    /// for it then resolves as a direct use), a held input discarded.
    Cancel,
}

/// A phase-aware interruption rule (Atlas 5.8.4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptionRule {
    /// The reaction-region action that interrupts.
    pub reaction: ActionId,
    pub interrupted: Interrupted,
    /// Only in this phase of the interrupted action (an edge's: its source
    /// action's phase); `None` in every phase.
    pub phase: Option<Phase>,
    pub consequence: Consequence,
}

/// A complete action graph: the edges, window modifiers, compatibility
/// rules and interruptions a zone runs. The [`ACTIONS`] and their recovery
/// modifiers are shared by every graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionGraph {
    pub edges: &'static [EdgeDef],
    pub window_modifiers: &'static [WindowModifier],
    pub rules: &'static [CompatibilityRule],
    pub interruptions: &'static [InterruptionRule],
}

/// The authored actions.
pub const ACTIONS: [ActionDef; 7] = [
    ActionDef {
        id: ActionId::Airborne,
        region: Region::Movement,
        recovery_ms: None,
    },
    ActionDef {
        id: ActionId::Landing,
        region: Region::Movement,
        recovery_ms: Some(300),
    },
    ActionDef {
        id: ActionId::DirectedSlash,
        region: Region::Offense,
        recovery_ms: Some(200),
    },
    ActionDef {
        id: ActionId::Guard,
        region: Region::Defense,
        recovery_ms: None,
    },
    ActionDef {
        id: ActionId::Stun,
        region: Region::Reaction,
        recovery_ms: None,
    },
    ActionDef {
        id: ActionId::GuardBreak,
        region: Region::Reaction,
        recovery_ms: Some(1_500),
    },
    ActionDef {
        id: ActionId::Death,
        region: Region::Reaction,
        recovery_ms: None,
    },
];

/// The authored recovery modifiers: a landing after an airborne melee
/// action recovers in 0.5 s instead of 0.3 s.
pub const RECOVERY_MODIFIERS: [RecoveryModifier; 1] = [RecoveryModifier {
    action: ActionId::Landing,
    condition: Condition::AttackedAirborne,
    recovery_ms: 500,
}];

/// The baseline edges.
pub const EDGES: [EdgeDef; 1] = [EdgeDef {
    id: EdgeId::SlashToShieldBash,
    from: ActionId::DirectedSlash,
    input: ActionInput::ShieldBash,
    anchor: Anchor::ConfirmedResult,
    window: Window {
        open_after_ms: 200,
        actionable_ms: 600,
        buffer_ms: 200,
    },
    conditions: &[
        Condition::Warden,
        Condition::Alive,
        Condition::PositiveHealthDamage,
        Condition::TargetAlive,
    ],
    effects: &[Effect::TargetConfirmed, Effect::IgnoreGlobalCooldown],
    precision: None,
    priority: 1,
}];

/// The baseline compatibility rules.
pub const RULES: [CompatibilityRule; 3] = [
    CompatibilityRule {
        id: RuleId::LandingHoldsMelee,
        during: ActionId::Landing,
        input: ActionInput::WardenMelee,
        conditions: &[Condition::Warden],
    },
    CompatibilityRule {
        id: RuleId::LandingHoldsMelee,
        during: ActionId::Landing,
        input: ActionInput::ShieldBash,
        conditions: &[Condition::Warden],
    },
    CompatibilityRule {
        id: RuleId::SlashRecoveryHoldsGuard,
        during: ActionId::DirectedSlash,
        input: ActionInput::GuardProtection,
        conditions: &[],
    },
];

/// The baseline interruptions: today's stun, guard-break and death
/// behavior as data.
pub const INTERRUPTIONS: [InterruptionRule; 7] = [
    // A stun pauses guard protection; the hold survives (5.2.29).
    InterruptionRule {
        reaction: ActionId::Stun,
        interrupted: Interrupted::Action(ActionId::Guard),
        phase: None,
        consequence: Consequence::Suspend,
    },
    // A stun during slash recovery discards the buffered guard (5.2.39).
    InterruptionRule {
        reaction: ActionId::Stun,
        interrupted: Interrupted::Held(RuleId::SlashRecoveryHoldsGuard),
        phase: None,
        consequence: Consequence::Cancel,
    },
    // The combo opening keeps running through a stun; the stun refuses the
    // Bash itself.
    InterruptionRule {
        reaction: ActionId::Stun,
        interrupted: Interrupted::Edge(EdgeId::SlashToShieldBash),
        phase: None,
        consequence: Consequence::Preserve,
    },
    // A broken guard protects nothing; the same held lane protects again
    // after the recovery (5.2.59).
    InterruptionRule {
        reaction: ActionId::GuardBreak,
        interrupted: Interrupted::Action(ActionId::Guard),
        phase: None,
        consequence: Consequence::Suspend,
    },
    InterruptionRule {
        reaction: ActionId::GuardBreak,
        interrupted: Interrupted::Edge(EdgeId::SlashToShieldBash),
        phase: None,
        consequence: Consequence::Preserve,
    },
    // Death lowers the guard and discards the opening (5.2.51).
    InterruptionRule {
        reaction: ActionId::Death,
        interrupted: Interrupted::Action(ActionId::Guard),
        phase: None,
        consequence: Consequence::Cancel,
    },
    InterruptionRule {
        reaction: ActionId::Death,
        interrupted: Interrupted::Edge(EdgeId::SlashToShieldBash),
        phase: None,
        consequence: Consequence::Cancel,
    },
];

/// The production graph: current gameplay, unchanged.
pub const BASELINE: ActionGraph = ActionGraph {
    edges: &EDGES,
    window_modifiers: &[],
    rules: &RULES,
    interruptions: &INTERRUPTIONS,
};

/// The definition of `id`.
#[must_use]
pub fn action(id: ActionId) -> &'static ActionDef {
    ACTIONS
        .iter()
        .find(|def| def.id == id)
        .expect("every action id is authored")
}

/// An action's committed recovery in ticks; zero without one.
#[must_use]
pub const fn recovery_ticks(id: ActionId) -> u16 {
    let mut index = 0;
    while index < ACTIONS.len() {
        if ACTIONS[index].id as u8 == id as u8 {
            return match ACTIONS[index].recovery_ms {
                Some(ms) => crate::ticks_from_ms(ms),
                None => 0,
            };
        }
        index += 1;
    }
    0
}

/// The first modified recovery of `id` under `condition`, in ticks.
#[must_use]
pub const fn modified_recovery_ticks(id: ActionId, condition: Condition) -> u16 {
    let mut index = 0;
    while index < RECOVERY_MODIFIERS.len() {
        let modifier = RECOVERY_MODIFIERS[index];
        if modifier.action as u8 == id as u8 && modifier.condition as u8 == condition as u8 {
            return crate::ticks_from_ms(modifier.recovery_ms);
        }
        index += 1;
    }
    recovery_ticks(id)
}

/// The window of the first Warden combo, in const form for its constants.
pub(crate) const SLASH_TO_SHIELD_BASH: Window = EDGES[0].window;

/// A definition problem validation found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphError {
    /// An action id is authored twice.
    DuplicateAction(ActionId),
    /// An edge id is authored twice.
    DuplicateEdge(EdgeId),
    /// An edge buffers longer than its anchor-to-opening lead.
    BufferBeforeAnchor(EdgeId),
    /// A rule covers an input of its own action's region.
    SameRegion(ActionId),
    /// A rule covers an action that has no recovery phase.
    NoRecovery(RuleId),
    /// A duration is shorter than half a tick or too long for a tick timer.
    BadDuration(EdgeId),
    /// The zone has no runtime for this edge's source or input: only a
    /// directed slash answered by Shield Bash runs.
    UnsupportedEdge(EdgeId),
    /// An edge confirms a target without requiring a damaging confirmed
    /// result.
    TargetUnconfirmed(EdgeId),
    /// A precision window does not fit inside its edge's actionable interval.
    PrecisionOutsideWindow(EdgeId),
    /// A modifier names an edge the graph lacks.
    UnknownEdge(EdgeId),
    /// The edge's modifiers can reduce its actionable duration below zero.
    NegativeWindow(EdgeId),
    /// An effect is malformed (zero refund) or conflicts with another.
    BadEffect(EdgeId),
    /// An interruption's reaction is not a reaction-region action.
    NotAReaction(ActionId),
    /// An interruption names a rule the graph lacks.
    UnknownRule(RuleId),
    /// The zone cannot apply this consequence to this target: only a guard
    /// and edge openings can be suspended or cancelled, and held inputs can
    /// only be preserved or cancelled.
    UnsupportedInterruption(Interrupted),
    /// Two interruptions of the same reaction, target and phase disagree.
    ConflictingInterruption(Interrupted),
}

const fn input_region(input: ActionInput) -> Region {
    match input {
        ActionInput::WardenMelee | ActionInput::ShieldBash => Region::Offense,
        ActionInput::GuardProtection => Region::Defense,
    }
}

/// Whether `ms` converts to a tick duration without failing.
fn duration_ok(ms: u32) -> bool {
    let ticks = u64::from(ms) * u64::from(crate::TICK_HZ);
    (ms == 0 || ticks + 500 >= 1_000) && (ticks + 500) / 1_000 <= u64::from(u16::MAX)
}

/// Checks the shared actions and the baseline graph for consistency.
pub fn validate() -> Result<(), GraphError> {
    BASELINE.validate()
}

impl ActionGraph {
    /// The definition of `id` in this graph.
    #[must_use]
    pub fn edge(&self, id: EdgeId) -> Option<&'static EdgeDef> {
        self.edges.iter().find(|def| def.id == id)
    }

    /// Checks this graph and the shared actions for consistency.
    pub fn validate(&self) -> Result<(), GraphError> {
        for (index, def) in ACTIONS.iter().enumerate() {
            if ACTIONS[..index].iter().any(|other| other.id == def.id) {
                return Err(GraphError::DuplicateAction(def.id));
            }
        }
        for (index, def) in self.edges.iter().enumerate() {
            self.validate_edge(index, def)?;
        }
        for modifier in self.window_modifiers {
            let Some(def) = self.edge(modifier.edge) else {
                return Err(GraphError::UnknownEdge(modifier.edge));
            };
            if !duration_ok(modifier.actionable_delta_ms.unsigned_abs()) {
                return Err(GraphError::BadDuration(def.id));
            }
        }
        for def in self.edges {
            // Every reduction applying at once must leave a window.
            let reduction: u32 = self
                .window_modifiers
                .iter()
                .filter(|modifier| modifier.edge == def.id && modifier.actionable_delta_ms < 0)
                .map(|modifier| {
                    u32::from(crate::ticks_from_ms(
                        modifier.actionable_delta_ms.unsigned_abs(),
                    ))
                })
                .sum();
            if reduction > u32::from(def.window.actionable_ticks()) {
                return Err(GraphError::NegativeWindow(def.id));
            }
        }
        for rule in self.rules {
            if action(rule.during).region == input_region(rule.input) {
                return Err(GraphError::SameRegion(rule.during));
            }
            if action(rule.during).recovery_ms.is_none() {
                return Err(GraphError::NoRecovery(rule.id));
            }
        }
        for (index, rule) in self.interruptions.iter().enumerate() {
            self.validate_interruption(rule)?;
            if self.interruptions[..index].iter().any(|other| {
                other.reaction == rule.reaction
                    && other.interrupted == rule.interrupted
                    && other.phase == rule.phase
                    && other.consequence != rule.consequence
            }) {
                return Err(GraphError::ConflictingInterruption(rule.interrupted));
            }
        }
        Ok(())
    }

    fn validate_edge(&self, index: usize, def: &EdgeDef) -> Result<(), GraphError> {
        if self.edges[..index].iter().any(|other| other.id == def.id) {
            return Err(GraphError::DuplicateEdge(def.id));
        }
        let window = def.window;
        let mut durations = vec![window.open_after_ms, window.actionable_ms, window.buffer_ms];
        if let Some(precision) = def.precision {
            durations.extend([precision.after_open_ms, precision.length_ms]);
        }
        let effects = def.effects.iter().chain(
            def.precision
                .iter()
                .flat_map(|precision| precision.benefits),
        );
        for effect in effects.clone() {
            match *effect {
                Effect::Recovery { ms } => durations.push(ms),
                Effect::ResourceRefund { amount: 0 } => return Err(GraphError::BadEffect(def.id)),
                _ => {}
            }
        }
        if !durations.into_iter().all(duration_ok) {
            return Err(GraphError::BadDuration(def.id));
        }
        // Two recoveries for one follow-up would be ambiguous.
        if effects
            .filter(|effect| matches!(effect, Effect::Recovery { .. }))
            .count()
            > 1
        {
            return Err(GraphError::BadEffect(def.id));
        }
        if window.buffer_ms > window.open_after_ms {
            return Err(GraphError::BufferBeforeAnchor(def.id));
        }
        if def.from != ActionId::DirectedSlash || def.input != ActionInput::ShieldBash {
            return Err(GraphError::UnsupportedEdge(def.id));
        }
        if def.effects.contains(&Effect::TargetConfirmed)
            && !def.conditions.contains(&Condition::PositiveHealthDamage)
        {
            return Err(GraphError::TargetUnconfirmed(def.id));
        }
        if let Some(precision) = def.precision {
            let (_, last) = precision.ticks(window.open_after_ticks());
            if precision.length_ms == 0 || last > window.ticks().closes() {
                return Err(GraphError::PrecisionOutsideWindow(def.id));
            }
        }
        Ok(())
    }

    fn validate_interruption(&self, rule: &InterruptionRule) -> Result<(), GraphError> {
        if action(rule.reaction).region != Region::Reaction {
            return Err(GraphError::NotAReaction(rule.reaction));
        }
        let supported = match rule.interrupted {
            Interrupted::Action(ActionId::Guard) => true,
            Interrupted::Action(_) => rule.consequence == Consequence::Preserve,
            Interrupted::Edge(id) => {
                if self.edge(id).is_none() {
                    return Err(GraphError::UnknownEdge(id));
                }
                true
            }
            Interrupted::Held(id) => {
                if !self.rules.iter().any(|other| other.id == id) {
                    return Err(GraphError::UnknownRule(id));
                }
                rule.consequence != Consequence::Suspend
            }
        };
        if supported {
            Ok(())
        } else {
            Err(GraphError::UnsupportedInterruption(rule.interrupted))
        }
    }
}

/// The graph input an intent makes, if any.
pub(crate) fn input_of(intent: &PlayerIntent) -> Option<ActionInput> {
    match intent {
        PlayerIntent::UseAbility { ability, .. } if *ability == ids::SHIELD_BASH.get() => {
            Some(ActionInput::ShieldBash)
        }
        PlayerIntent::UseAbility { ability, .. }
            if [ids::HEROIC_STRIKE, ids::CLEAVE]
                .iter()
                .any(|id| id.get() == *ability) =>
        {
            Some(ActionInput::WardenMelee)
        }
        PlayerIntent::DirectedStrike { .. } => Some(ActionInput::WardenMelee),
        _ => None,
    }
}

/// The directed slash's runtime: ticks since it resolved, and the opponent
/// its confirmed result damaged.
fn slash(state: &PlayerState) -> Option<(u16, Option<EntityRef>)> {
    let recovery = recovery_ticks(ActionId::DirectedSlash);
    match state.combo {
        Some(combo) => Some((combo.elapsed, Some(combo.target))),
        None if state.strike_recovery > 0 => {
            Some((recovery.saturating_sub(state.strike_recovery), None))
        }
        None => None,
    }
}

fn holds(state: &PlayerState, condition: Condition) -> bool {
    match condition {
        Condition::Warden => state
            .class
            .is_some_and(|choice| choice.class == PlayerClass::Warden),
        Condition::Alive => state.is_alive(),
        // The opening exists only after positive damage to a living target;
        // a miss, a whiff, an absorbed blow or the target's death clears it.
        Condition::PositiveHealthDamage | Condition::TargetAlive => state.combo.is_some(),
        Condition::AttackedAirborne => state.landing.attacked_airborne,
        Condition::Airborne => state.landing.airborne,
        Condition::Stunned => aura::has_kind(&state.auras, AuraKind::Stun),
    }
}

fn all_hold(state: &PlayerState, conditions: &[Condition]) -> bool {
    conditions.iter().all(|&condition| holds(state, condition))
}

/// Whether `id` is in its recovery phase, with the ticks of it left.
fn recovery_left(state: &PlayerState, id: ActionId) -> Option<u16> {
    let left = match id {
        ActionId::Landing => state.landing.recovery,
        ActionId::DirectedSlash => state.strike_recovery,
        ActionId::GuardBreak => state.guard.broken,
        ActionId::Airborne | ActionId::Guard | ActionId::Stun | ActionId::Death => 0,
    };
    (left > 0).then_some(left)
}

/// The rule of `graph` that currently holds `input`.
pub(crate) fn holding_rule(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
) -> Option<&'static CompatibilityRule> {
    graph.rules.iter().find(|rule| {
        rule.input == input
            && recovery_left(state, rule.during).is_some()
            && all_hold(state, rule.conditions)
    })
}

/// The running action and phase of each region.
fn region_actions(state: &PlayerState) -> [(Option<ActionId>, Option<Phase>, Option<u16>); 4] {
    let movement = if state.landing.airborne {
        (Some(ActionId::Airborne), Some(Phase::Active), None)
    } else if let Some(left) = recovery_left(state, ActionId::Landing) {
        (Some(ActionId::Landing), Some(Phase::Recovery), Some(left))
    } else {
        (None, None, None)
    };
    let offense = match slash(state) {
        Some(_) => match recovery_left(state, ActionId::DirectedSlash) {
            Some(left) => (
                Some(ActionId::DirectedSlash),
                Some(Phase::Recovery),
                Some(left),
            ),
            None => (Some(ActionId::DirectedSlash), Some(Phase::FollowUp), None),
        },
        None => (None, None, None),
    };
    let defense = match state.guard.raised {
        Some(_) => (Some(ActionId::Guard), Some(Phase::Active), None),
        None => (None, None, None),
    };
    let reaction = match active_reactions(state).first() {
        Some(&ActionId::GuardBreak) => (
            Some(ActionId::GuardBreak),
            Some(Phase::Recovery),
            recovery_left(state, ActionId::GuardBreak),
        ),
        Some(&reaction) => (Some(reaction), Some(Phase::Active), None),
        None => (None, None, None),
    };
    [movement, offense, defense, reaction]
}

/// The reactions in effect, dominant first: death, stun, guard break.
fn active_reactions(state: &PlayerState) -> Vec<ActionId> {
    let mut reactions = Vec::new();
    if !state.is_alive() {
        reactions.push(ActionId::Death);
    }
    if aura::has_kind(&state.auras, AuraKind::Stun) {
        reactions.push(ActionId::Stun);
    }
    if state.guard.broken > 0 {
        reactions.push(ActionId::GuardBreak);
    }
    reactions
}

/// The phase of an interruption target, while it runs.
fn target_phase(graph: &ActionGraph, state: &PlayerState, target: Interrupted) -> Option<Phase> {
    let regions = region_actions(state);
    match target {
        Interrupted::Action(id) => regions
            .iter()
            .find(|(action, _, _)| *action == Some(id))
            .and_then(|(_, phase, _)| *phase),
        Interrupted::Edge(id) => {
            let def = graph.edge(id)?;
            // Only an opening (a confirmed source) can be interrupted.
            state.combo?;
            regions
                .iter()
                .find(|(action, _, _)| *action == Some(def.from))
                .and_then(|(_, phase, _)| *phase)
        }
        Interrupted::Held(id) => {
            let held = graph.rules.iter().any(|rule| {
                rule.id == id
                    && holding_rule(graph, state, rule.input).is_some_and(|by| by.id == id)
                    && match rule.input {
                        ActionInput::GuardProtection => state.guard.raised.is_some(),
                        input => state
                            .intents
                            .iter()
                            .any(|intent| input_of(intent) == Some(input)),
                    }
            });
            held.then_some(Phase::Recovery)
        }
    }
}

/// How the reactions in effect treat a running target: the strongest
/// consequence, its reaction and whether a rule authored it. `None` when the
/// target is not running or no reaction is in effect.
pub(crate) fn interruption(
    graph: &ActionGraph,
    state: &PlayerState,
    target: Interrupted,
) -> Option<(Consequence, ActionId, Option<&'static InterruptionRule>)> {
    let reactions = active_reactions(state);
    let first = *reactions.first()?;
    let phase = target_phase(graph, state, target)?;
    let mut decided = (Consequence::Preserve, first, None);
    for &reaction in &reactions {
        let authored = graph.interruptions.iter().find(|rule| {
            rule.reaction == reaction
                && rule.interrupted == target
                && rule.phase.is_none_or(|only| only == phase)
        });
        if let Some(rule) = authored
            && (decided.2.is_none() || rule.consequence > decided.0)
        {
            decided = (rule.consequence, reaction, Some(rule));
        }
    }
    Some(decided)
}

/// The consequence for `target`; preserve when nothing interrupts it.
pub(crate) fn consequence(
    graph: &ActionGraph,
    state: &PlayerState,
    target: Interrupted,
) -> Consequence {
    interruption(graph, state, target)
        .map_or(Consequence::Preserve, |(consequence, ..)| consequence)
}

/// An edge's state for one player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeStatus {
    /// The input would follow this edge now.
    Open,
    /// The edge will open; the input waits for it.
    Buffering,
    /// A reaction suspends the opening: its clock stops and it holds a
    /// buffered input until the reaction ends.
    Suspended(ActionId),
    /// Not usable now.
    Blocked(BlockReason),
}

/// Why an edge is not usable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockReason {
    /// Its source action is not running.
    SourceInactive,
    /// A condition does not hold.
    Condition(Condition),
    /// Before the early buffer.
    TooEarly,
    /// After the actionable window.
    Expired,
    /// A reaction cancels the opening at the end of this tick.
    Cancelled(ActionId),
}

/// An edge's window after the modifiers whose conditions hold.
pub(crate) fn effective_window(
    state: &PlayerState,
    graph: &ActionGraph,
    def: &EdgeDef,
) -> TickWindow {
    let mut window = def.window.ticks();
    for modifier in graph.window_modifiers {
        if modifier.edge == def.id && all_hold(state, modifier.conditions) {
            let delta = crate::ticks_from_ms(modifier.actionable_delta_ms.unsigned_abs());
            window.actionable = if modifier.actionable_delta_ms < 0 {
                window.actionable.saturating_sub(delta)
            } else {
                window.actionable.saturating_add(delta)
            };
        }
    }
    window
}

/// An edge's status plus its window in ticks since the anchor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EdgeEval {
    status: EdgeStatus,
    /// Ticks since the anchor, while the source runs.
    elapsed: Option<u16>,
    window: TickWindow,
    /// An input now falls inside the precision window.
    precise: bool,
}

fn evaluate(graph: &ActionGraph, state: &PlayerState, def: &EdgeDef) -> EdgeEval {
    let window = effective_window(state, graph, def);
    let source = match def.from {
        ActionId::DirectedSlash => slash(state),
        _ => None,
    };
    let Some((elapsed, _)) = source else {
        return EdgeEval {
            status: EdgeStatus::Blocked(BlockReason::SourceInactive),
            elapsed: None,
            window,
            precise: false,
        };
    };
    let failed = def
        .conditions
        .iter()
        .copied()
        .find(|&condition| !holds(state, condition));
    let interrupted = interruption(graph, state, Interrupted::Edge(def.id));
    let status = match (failed, interrupted) {
        (Some(condition), _) => EdgeStatus::Blocked(BlockReason::Condition(condition)),
        (None, Some((Consequence::Cancel, reaction, _))) => {
            EdgeStatus::Blocked(BlockReason::Cancelled(reaction))
        }
        (None, Some((Consequence::Suspend, reaction, _))) => match window.status_at(elapsed) {
            EdgeStatus::Blocked(reason) => EdgeStatus::Blocked(reason),
            _ => EdgeStatus::Suspended(reaction),
        },
        (None, _) => window.status_at(elapsed),
    };
    let precise = status == EdgeStatus::Open
        && def.precision.is_some_and(|precision| {
            let (from, until) = precision.ticks(window.opens);
            (from..=until).contains(&elapsed)
        });
    EdgeEval {
        status,
        elapsed: Some(elapsed),
        window,
        precise,
    }
}

/// A candidate transition for one input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Candidate {
    /// Through an authored edge.
    Edge(EdgeId),
    /// The input's ordinary use, priority 0.
    Direct,
}

/// What arbitration made of one candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Won,
    /// Usable, but a candidate of higher priority won.
    Outranked(Candidate),
    /// Will open (or resume); it holds the input because it outranks every
    /// usable candidate.
    Holding,
    /// A compatibility rule holds the input.
    HeldBy(RuleId),
    Blocked(BlockReason),
}

/// The outcome of arbitrating one input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The input resolves now through this candidate. Ordinary validation,
    /// such as cooldowns, Rage, range and line of sight, still applies.
    Proceed(Candidate),
    /// The input waits in the queue for a compatibility rule.
    WaitForRule(RuleId),
    /// The input waits in the queue for this edge to open.
    WaitForEdge(EdgeId),
}

/// The explained arbitration of one input (Atlas 5.7.4).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Arbitration {
    pub input: ActionInput,
    pub outcome: Outcome,
    pub candidates: Vec<(Candidate, Verdict)>,
    /// The winning edge resolves inside its precision window.
    pub precise: bool,
}

/// Arbitrates `input` for a player in `state`.
pub(crate) fn arbitrate(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
) -> Arbitration {
    let mut edges: Vec<(&EdgeDef, EdgeEval)> = graph
        .edges
        .iter()
        .filter(|def| def.input == input)
        .map(|def| (def, evaluate(graph, state, def)))
        .collect();
    // Highest priority first; ties in authored order.
    edges.sort_by_key(|(def, _)| std::cmp::Reverse(def.priority));
    let hold = holding_rule(graph, state, input);
    let outcome = if let Some(rule) = hold {
        Outcome::WaitForRule(rule.id)
    } else {
        edges
            .iter()
            .find_map(|(def, eval)| match eval.status {
                EdgeStatus::Open => Some(Outcome::Proceed(Candidate::Edge(def.id))),
                EdgeStatus::Buffering | EdgeStatus::Suspended(_) if def.priority > 0 => {
                    Some(Outcome::WaitForEdge(def.id))
                }
                _ => None,
            })
            .unwrap_or(Outcome::Proceed(Candidate::Direct))
    };
    let precise = edges
        .iter()
        .any(|(def, eval)| outcome == Outcome::Proceed(Candidate::Edge(def.id)) && eval.precise);
    let verdict = |candidate: Candidate, status: Option<EdgeStatus>| -> Verdict {
        if let Some(rule) = hold {
            return Verdict::HeldBy(rule.id);
        }
        match (outcome, status) {
            (Outcome::Proceed(winner), _) if winner == candidate => Verdict::Won,
            (Outcome::WaitForEdge(id), _) if Candidate::Edge(id) == candidate => Verdict::Holding,
            (_, Some(EdgeStatus::Blocked(reason))) => Verdict::Blocked(reason),
            (Outcome::Proceed(winner), _) => Verdict::Outranked(winner),
            (Outcome::WaitForEdge(id), _) => Verdict::Outranked(Candidate::Edge(id)),
            (Outcome::WaitForRule(rule), _) => Verdict::HeldBy(rule),
        }
    };
    let mut candidates: Vec<_> = edges
        .iter()
        .map(|(def, eval)| {
            let candidate = Candidate::Edge(def.id);
            (candidate, verdict(candidate, Some(eval.status)))
        })
        .collect();
    candidates.push((Candidate::Direct, verdict(Candidate::Direct, None)));
    Arbitration {
        input,
        outcome,
        candidates,
        precise,
    }
}

/// Whether an intent waits in the queue rather than resolving now.
pub(crate) fn waits(graph: &ActionGraph, state: &PlayerState, intent: &PlayerIntent) -> bool {
    input_of(intent)
        .is_some_and(|input| !matches!(arbitrate(graph, state, input).outcome, Outcome::Proceed(_)))
}

/// An input resolving through an edge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FollowUp {
    pub(crate) edge: EdgeId,
    /// The source's confirmed target, with [`Effect::TargetConfirmed`].
    pub(crate) target: Option<EntityRef>,
    /// The edge's effects, then the precision benefits if it was precise.
    pub(crate) effects: Vec<Effect>,
}

impl FollowUp {
    pub(crate) fn has(&self, effect: Effect) -> bool {
        self.effects.contains(&effect)
    }

    /// The follow-up's recovery in ticks, if an effect sets it.
    pub(crate) fn recovery_ticks(&self) -> Option<u16> {
        self.effects.iter().find_map(|effect| match *effect {
            Effect::Recovery { ms } => Some(crate::ticks_from_ms(ms)),
            _ => None,
        })
    }

    /// The resource the follow-up returns.
    pub(crate) fn refund(&self) -> u16 {
        self.effects
            .iter()
            .map(|effect| match *effect {
                Effect::ResourceRefund { amount } => amount,
                _ => 0,
            })
            .fold(0, u16::saturating_add)
    }
}

/// The edge `input` resolves through now, with its effects.
pub(crate) fn follow_up(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
) -> Option<FollowUp> {
    let arbitration = arbitrate(graph, state, input);
    let Outcome::Proceed(Candidate::Edge(id)) = arbitration.outcome else {
        return None;
    };
    let def = graph.edge(id)?;
    let mut effects = def.effects.to_vec();
    if arbitration.precise
        && let Some(precision) = def.precision
    {
        effects.extend_from_slice(precision.benefits);
    }
    let target = match def.from {
        ActionId::DirectedSlash => slash(state).and_then(|(_, target)| target),
        _ => None,
    }
    .filter(|_| effects.contains(&Effect::TargetConfirmed));
    Some(FollowUp {
        edge: id,
        target,
        effects,
    })
}

/// One region's running action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionReport {
    pub region: Region,
    pub action: Option<ActionId>,
    pub phase: Option<Phase>,
    /// Ticks of the committed recovery left.
    pub recovery_left: Option<u16>,
}

/// One edge's state, in absolute ticks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EdgeReport {
    pub edge: EdgeId,
    pub priority: u8,
    pub status: EdgeStatus,
    /// The tick of the authoritative anchor.
    pub anchor_tick: Option<u64>,
    /// The first tick an input is buffered.
    pub buffer_from: Option<u64>,
    /// The first tick the edge resolves an input.
    pub open_from: Option<u64>,
    /// The last tick the edge resolves an input, after window modifiers.
    pub open_until: Option<u64>,
    /// The first and last tick of the precision window.
    pub precision: Option<(u64, u64)>,
}

/// One compatibility rule's state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleReport {
    pub rule: RuleId,
    pub input: ActionInput,
    /// The first tick the input is no longer held.
    pub released_at: u64,
}

/// How the reactions in effect treat one running target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptionReport {
    pub interrupted: Interrupted,
    /// The reaction that decides the consequence.
    pub reaction: ActionId,
    pub consequence: Consequence,
    /// An authored rule decided it; otherwise the default preserves.
    pub authored: bool,
}

/// A read-only view of one player's action graph (#380, #381).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionInspection {
    /// The last completed tick.
    pub tick: u64,
    pub regions: [RegionReport; 4],
    pub edges: Vec<EdgeReport>,
    /// The rules that currently apply.
    pub rules: Vec<RuleReport>,
    /// What the reactions in effect do to each running action, opening and
    /// buffer.
    pub interruptions: Vec<InterruptionReport>,
}

impl ZoneSimulation {
    /// The action graph this zone runs.
    #[must_use]
    pub fn action_graph(&self) -> &'static ActionGraph {
        self.action_graph
    }

    /// Runs `graph` instead of the [`BASELINE`] after validating it. This is
    /// zone setup for tests and the lab, not canonical state.
    pub fn set_action_graph(&mut self, graph: &'static ActionGraph) -> Result<(), GraphError> {
        graph.validate()?;
        self.action_graph = graph;
        Ok(())
    }

    /// Applies the reactions' cancellations to every player, before the
    /// tick's timers advance. Suspensions are read where they apply.
    pub(crate) fn apply_interruptions(&mut self) {
        let graph = self.action_graph;
        for player in self.players.values_mut() {
            for rule in graph.rules {
                if consequence(graph, player, Interrupted::Held(rule.id)) == Consequence::Cancel {
                    if rule.input == ActionInput::GuardProtection {
                        player.guard.raised = None;
                    }
                    player
                        .intents
                        .retain(|intent| input_of(intent) != Some(rule.input));
                }
            }
            if consequence(graph, player, Interrupted::Action(ActionId::Guard))
                == Consequence::Cancel
            {
                player.guard.raised = None;
            }
        }
    }

    /// The player's action graph after the last completed tick.
    #[must_use]
    pub fn action_inspection(&self, player_id: PlayerId) -> Option<ActionInspection> {
        let state = self.players.get(&player_id)?;
        let graph = self.action_graph;
        let tick = self.tick;
        // Intents of the next tick resolve before its timers advance.
        let next = tick + 1;
        let [movement, offense, defense, reaction] = region_actions(state);
        let report = |region, (action, phase, recovery_left)| RegionReport {
            region,
            action,
            phase,
            recovery_left,
        };
        let edges = graph
            .edges
            .iter()
            .map(|def| {
                let eval = evaluate(graph, state, def);
                // The source resolved during the intents of `anchor`; ticks
                // since then are counted at the end of each tick. A
                // suspended clock reports the ticks it would resume with.
                let anchor = eval
                    .elapsed
                    .map(|elapsed| next.saturating_sub(u64::from(elapsed)));
                let window = eval.window;
                let at = |offset: u16| anchor.map(|at| at + u64::from(offset));
                EdgeReport {
                    edge: def.id,
                    priority: def.priority,
                    status: eval.status,
                    anchor_tick: anchor,
                    buffer_from: at(window.opens - window.buffer),
                    open_from: at(window.opens),
                    open_until: at(window.closes()),
                    precision: def.precision.and_then(|precision| {
                        let (from, until) = precision.ticks(window.opens);
                        Some((at(from)?, at(until.min(window.closes()))?))
                    }),
                }
            })
            .collect();
        let rules = graph
            .rules
            .iter()
            .filter(|rule| {
                recovery_left(state, rule.during).is_some() && all_hold(state, rule.conditions)
            })
            .map(|rule| RuleReport {
                rule: rule.id,
                input: rule.input,
                // Recovery counts down after the next tick's intents and
                // combat resolve.
                released_at: next + u64::from(recovery_left(state, rule.during).unwrap_or(0)),
            })
            .collect();
        let targets = ACTIONS
            .iter()
            .filter(|def| def.region != Region::Reaction)
            .map(|def| Interrupted::Action(def.id))
            .chain(graph.edges.iter().map(|def| Interrupted::Edge(def.id)))
            .chain(graph.rules.iter().map(|rule| Interrupted::Held(rule.id)));
        let mut interruptions: Vec<InterruptionReport> = Vec::new();
        for target in targets {
            if interruptions
                .iter()
                .any(|known| known.interrupted == target)
            {
                continue;
            }
            if let Some((consequence, reaction, rule)) = interruption(graph, state, target) {
                interruptions.push(InterruptionReport {
                    interrupted: target,
                    reaction,
                    consequence,
                    authored: rule.is_some(),
                });
            }
        }
        Some(ActionInspection {
            tick,
            regions: [
                report(Region::Movement, movement),
                report(Region::Offense, offense),
                report(Region::Defense, defense),
                report(Region::Reaction, reaction),
            ],
            edges,
            rules,
            interruptions,
        })
    }

    /// Explains how `input` would be arbitrated at the player's next tick.
    #[must_use]
    pub fn arbitrate_input(&self, player_id: PlayerId, input: ActionInput) -> Option<Arbitration> {
        self.players
            .get(&player_id)
            .map(|state| arbitrate(self.action_graph, state, input))
    }
}
