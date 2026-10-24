//! The authoritative parallel action graph (#380, Atlas 5.7–5.8).
//!
//! One inspectable model decides how a player's concurrent actions interact.
//! The character is split into four **regions**: movement, offense, defense
//! and reaction. Each region runs at most one [`ActionId`] at a time, so
//! combinations are never enumerated as one state per product. The data says
//! how they meet:
//!
//! - [`RULES`]: **compatibility rules.** An action in its recovery phase
//!   *holds* (buffers) or *cancels* an input aimed at another region. For
//!   example, landing recovery holds Warden melee, and slash recovery holds
//!   guard protection.
//! - [`EDGES`]: **directed edges.** An edge leads from an action to a
//!   follow-up input. Each edge has an authoritative [`Anchor`], an opening
//!   offset, an actionable duration, a bounded early buffer, typed
//!   [`Condition`]s, typed [`Effect`]s and a priority.
//! - **Arbitration.** [`ZoneSimulation::arbitrate_input`] compares every
//!   edge that answers an input with the input's direct use. The open edge
//!   with the highest priority wins. A buffering edge with higher priority
//!   than the direct use holds the input until it opens. Waiting inputs stay
//!   in the intent queue, so the latest valid input resolves (5.2.41).
//!   Every candidate gets a [`Verdict`] that says why it won or was refused.
//!
//! Durations are authored in milliseconds and converted once through
//! [`crate::ticks_from_ms`], so they keep their meaning when `TICK_HZ`
//! changes (#379). The baseline below is the Warden's current gameplay (5.2.37–5.2.51,
//! 5.2.69–5.2.70) expressed as data, with unchanged values.
//!
//! Runtime state stays in the existing canonical fields: `landing` for the
//! movement region, `strike_recovery` and `combo` for the offense region,
//! `guard` for the defense region, and auras with the guard break for the
//! reaction region. This slice introduces no new snapshot state.
//! [`ZoneSimulation::action_inspection`] is the read-only query surface that
//! the Action Lab (#381) reads.

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

impl ActionInput {
    const fn is_warden_melee(self) -> bool {
        matches!(self, Self::WardenMelee | Self::ShieldBash)
    }
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
    /// A stun aura is active.
    Stunned,
}

/// A typed effect an edge applies when it wins (Atlas 5.7.3).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Effect {
    /// The follow-up hits the source's confirmed target, whatever is selected
    /// (5.2.47).
    TargetConfirmed,
    /// The follow-up may ignore the global cooldown the source started
    /// (5.2.51).
    IgnoreGlobalCooldown,
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
    /// Higher wins. An input's direct use has priority 0.
    pub priority: u8,
}

/// How an active action treats an input aimed at another region.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Interaction {
    /// The input waits, kept in the queue, until the recovery ends.
    Hold,
    /// The pending input is discarded.
    Cancel,
}

/// Stable compatibility rule identities.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuleId {
    /// Warden melee waits out landing recovery (5.2.70).
    LandingHoldsMelee,
    /// A guard raised during slash recovery protects only afterwards
    /// (5.2.39, 5.2.41).
    SlashRecoveryHoldsGuard,
    /// A stun during slash recovery discards the buffered guard.
    StunCancelsBufferedGuard,
}

/// A region-compatibility rule (Atlas 5.7.5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompatibilityRule {
    pub id: RuleId,
    /// The action whose recovery phase the rule covers.
    pub during: ActionId,
    pub input: ActionInput,
    pub conditions: &'static [Condition],
    pub interaction: Interaction,
}

/// The authored actions.
pub const ACTIONS: [ActionDef; 6] = [
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
];

/// The authored recovery modifiers: a landing after an airborne melee
/// action recovers in 0.5 s instead of 0.3 s.
pub const RECOVERY_MODIFIERS: [RecoveryModifier; 1] = [RecoveryModifier {
    action: ActionId::Landing,
    condition: Condition::AttackedAirborne,
    recovery_ms: 500,
}];

/// The authored edges.
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
    priority: 1,
}];

/// The authored compatibility rules.
pub const RULES: [CompatibilityRule; 4] = [
    CompatibilityRule {
        id: RuleId::LandingHoldsMelee,
        during: ActionId::Landing,
        input: ActionInput::WardenMelee,
        conditions: &[Condition::Warden],
        interaction: Interaction::Hold,
    },
    CompatibilityRule {
        id: RuleId::LandingHoldsMelee,
        during: ActionId::Landing,
        input: ActionInput::ShieldBash,
        conditions: &[Condition::Warden],
        interaction: Interaction::Hold,
    },
    CompatibilityRule {
        id: RuleId::SlashRecoveryHoldsGuard,
        during: ActionId::DirectedSlash,
        input: ActionInput::GuardProtection,
        conditions: &[],
        interaction: Interaction::Hold,
    },
    CompatibilityRule {
        id: RuleId::StunCancelsBufferedGuard,
        during: ActionId::DirectedSlash,
        input: ActionInput::GuardProtection,
        conditions: &[Condition::Stunned],
        interaction: Interaction::Cancel,
    },
];

/// The definition of `id`.
#[must_use]
pub fn action(id: ActionId) -> &'static ActionDef {
    ACTIONS
        .iter()
        .find(|def| def.id == id)
        .expect("every action id is authored")
}

/// The definition of `id`.
#[must_use]
pub fn edge(id: EdgeId) -> &'static EdgeDef {
    EDGES
        .iter()
        .find(|def| def.id == id)
        .expect("every edge id is authored")
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

/// A definition problem [`validate`] found.
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
}

const fn input_region(input: ActionInput) -> Region {
    match input {
        ActionInput::WardenMelee | ActionInput::ShieldBash => Region::Offense,
        ActionInput::GuardProtection => Region::Defense,
    }
}

/// Checks the authored graph for consistency. Durations are checked at
/// compile time by [`crate::ticks_from_ms`].
pub fn validate() -> Result<(), GraphError> {
    for (index, def) in ACTIONS.iter().enumerate() {
        if ACTIONS[..index].iter().any(|other| other.id == def.id) {
            return Err(GraphError::DuplicateAction(def.id));
        }
    }
    for (index, def) in EDGES.iter().enumerate() {
        if EDGES[..index].iter().any(|other| other.id == def.id) {
            return Err(GraphError::DuplicateEdge(def.id));
        }
        if def.window.buffer_ms > def.window.open_after_ms {
            return Err(GraphError::BufferBeforeAnchor(def.id));
        }
    }
    for rule in &RULES {
        if action(rule.during).region == input_region(rule.input) {
            return Err(GraphError::SameRegion(rule.during));
        }
        if action(rule.during).recovery_ms.is_none() {
            return Err(GraphError::NoRecovery(rule.id));
        }
    }
    Ok(())
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
        Condition::Stunned => aura::has_kind(&state.auras, AuraKind::Stun),
    }
}

/// Whether `id` is in its recovery phase, with the ticks of it left.
fn recovery_left(state: &PlayerState, id: ActionId) -> Option<u16> {
    let left = match id {
        ActionId::Landing => state.landing.recovery,
        ActionId::DirectedSlash => state.strike_recovery,
        ActionId::GuardBreak => state.guard.broken,
        ActionId::Airborne | ActionId::Guard | ActionId::Stun => 0,
    };
    (left > 0).then_some(left)
}

/// The rule of `interaction` that currently applies to `input`.
pub(crate) fn rule_applying(
    state: &PlayerState,
    input: ActionInput,
    interaction: Interaction,
) -> Option<&'static CompatibilityRule> {
    RULES.iter().find(|rule| {
        rule.input == input
            && rule.interaction == interaction
            && recovery_left(state, rule.during).is_some()
            && rule
                .conditions
                .iter()
                .all(|&condition| holds(state, condition))
    })
}

/// An edge's state for one player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeStatus {
    /// The input would follow this edge now.
    Open,
    /// The edge will open; the input waits for it.
    Buffering,
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
}

/// An edge's status plus its window in ticks since the anchor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EdgeEval {
    status: EdgeStatus,
    /// Ticks since the anchor, while the source runs.
    elapsed: Option<u16>,
}

fn evaluate(state: &PlayerState, def: &EdgeDef) -> EdgeEval {
    let source = match def.from {
        ActionId::DirectedSlash => slash(state),
        _ => None,
    };
    let Some((elapsed, _)) = source else {
        return EdgeEval {
            status: EdgeStatus::Blocked(BlockReason::SourceInactive),
            elapsed: None,
        };
    };
    let failed = def
        .conditions
        .iter()
        .copied()
        .find(|&condition| !holds(state, condition));
    let window = def.window;
    let opens = window.open_after_ticks();
    let status = if let Some(condition) = failed {
        EdgeStatus::Blocked(BlockReason::Condition(condition))
    } else if elapsed < opens.saturating_sub(window.buffer_ticks()) {
        EdgeStatus::Blocked(BlockReason::TooEarly)
    } else if elapsed < opens {
        EdgeStatus::Buffering
    } else if elapsed <= opens + window.actionable_ticks() {
        EdgeStatus::Open
    } else {
        EdgeStatus::Blocked(BlockReason::Expired)
    };
    EdgeEval {
        status,
        elapsed: Some(elapsed),
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
    /// Will open; it holds the input because it outranks every usable
    /// candidate.
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
}

/// Arbitrates `input` for a player in `state`.
pub(crate) fn arbitrate(state: &PlayerState, input: ActionInput) -> Arbitration {
    let mut edges: Vec<(&EdgeDef, EdgeEval)> = EDGES
        .iter()
        .filter(|def| def.input == input)
        .map(|def| (def, evaluate(state, def)))
        .collect();
    // Highest priority first; ties in authored order.
    edges.sort_by_key(|(def, _)| std::cmp::Reverse(def.priority));
    let hold = if input.is_warden_melee() || input == ActionInput::GuardProtection {
        rule_applying(state, input, Interaction::Hold)
    } else {
        None
    };
    let outcome = if let Some(rule) = hold {
        Outcome::WaitForRule(rule.id)
    } else {
        edges
            .iter()
            .find_map(|(def, eval)| match eval.status {
                EdgeStatus::Open => Some(Outcome::Proceed(Candidate::Edge(def.id))),
                EdgeStatus::Buffering if def.priority > 0 => Some(Outcome::WaitForEdge(def.id)),
                _ => None,
            })
            .unwrap_or(Outcome::Proceed(Candidate::Direct))
    };
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
    }
}

/// Whether an intent waits in the queue rather than resolving now.
pub(crate) fn waits(state: &PlayerState, intent: &PlayerIntent) -> bool {
    input_of(intent)
        .is_some_and(|input| !matches!(arbitrate(state, input).outcome, Outcome::Proceed(_)))
}

/// The winning edge's confirmed target, for an input that proceeds through
/// an edge with [`Effect::TargetConfirmed`].
pub(crate) fn follow_up(state: &PlayerState, input: ActionInput) -> Option<(EdgeId, EntityRef)> {
    let Outcome::Proceed(Candidate::Edge(id)) = arbitrate(state, input).outcome else {
        return None;
    };
    let target = match edge(id).from {
        ActionId::DirectedSlash => slash(state).and_then(|(_, target)| target)?,
        _ => return None,
    };
    edge(id)
        .effects
        .contains(&Effect::TargetConfirmed)
        .then_some((id, target))
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
    /// The last tick the edge resolves an input.
    pub open_until: Option<u64>,
}

/// One compatibility rule's state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleReport {
    pub rule: RuleId,
    pub input: ActionInput,
    pub interaction: Interaction,
    /// The first tick the input is no longer held, while the rule applies.
    pub released_at: Option<u64>,
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
}

impl ZoneSimulation {
    /// The player's action graph after the last completed tick.
    #[must_use]
    pub fn action_inspection(&self, player_id: PlayerId) -> Option<ActionInspection> {
        let state = self.players.get(&player_id)?;
        let tick = self.tick;
        // Intents of the next tick resolve before its timers advance.
        let next = tick + 1;
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
        let reaction = if aura::has_kind(&state.auras, AuraKind::Stun) {
            (Some(ActionId::Stun), Some(Phase::Active), None)
        } else if let Some(left) = recovery_left(state, ActionId::GuardBreak) {
            (
                Some(ActionId::GuardBreak),
                Some(Phase::Recovery),
                Some(left),
            )
        } else {
            (None, None, None)
        };
        let report = |region, (action, phase, recovery_left)| RegionReport {
            region,
            action,
            phase,
            recovery_left,
        };
        let edges = EDGES
            .iter()
            .map(|def| {
                let eval = evaluate(state, def);
                // The source resolved during the intents of `anchor`; ticks
                // since then are counted at the end of each tick.
                let anchor = eval
                    .elapsed
                    .map(|elapsed| next.saturating_sub(u64::from(elapsed)));
                let window = def.window;
                let opens = u64::from(window.open_after_ticks());
                EdgeReport {
                    edge: def.id,
                    priority: def.priority,
                    status: eval.status,
                    anchor_tick: anchor,
                    buffer_from: anchor.map(|at| at + opens - u64::from(window.buffer_ticks())),
                    open_from: anchor.map(|at| at + opens),
                    open_until: anchor.map(|at| at + opens + u64::from(window.actionable_ticks())),
                }
            })
            .collect();
        let rules = RULES
            .iter()
            .filter(|rule| {
                recovery_left(state, rule.during).is_some()
                    && rule
                        .conditions
                        .iter()
                        .all(|&condition| holds(state, condition))
            })
            .map(|rule| {
                let left = recovery_left(state, rule.during).unwrap_or(0);
                // Recovery counts down after the next tick's intents and
                // combat resolve.
                let released_at = next + u64::from(left);
                RuleReport {
                    rule: rule.id,
                    input: rule.input,
                    interaction: rule.interaction,
                    released_at: (rule.interaction == Interaction::Hold).then_some(released_at),
                }
            })
            .collect();
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
        })
    }

    /// Explains how `input` would be arbitrated at the player's next tick.
    #[must_use]
    pub fn arbitrate_input(&self, player_id: PlayerId, input: ActionInput) -> Option<Arbitration> {
        self.players
            .get(&player_id)
            .map(|state| arbitrate(state, input))
    }
}
