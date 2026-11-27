//! The graph at run time (#380, Atlas 5.7–5.9): what each region runs now and in which phase,
//! read from canonical player state; compatibility, interruptions, edge windows, arbitration,
//! follow-ups and authored movement; and the zone steps that apply them.
use super::*;

/// The ability a running action runs: Heroic Strike for the directed
/// slash, the cast's ability for a cast.
pub(super) fn action_ability(state: &PlayerState, id: ActionId) -> Option<AbilityId> {
    match id {
        ActionId::DirectedSlash => Some(ids::HEROIC_STRIKE),
        ActionId::Cast => state.cast.map(|cast| cast.ability),
        _ => None,
    }
}

/// The committed recovery `id` starts for `ability`, in ticks, after
/// overrides.
pub(crate) fn recovery_for(
    graph: &ActionGraph,
    state: &PlayerState,
    id: ActionId,
    ability: Option<AbilityId>,
) -> u16 {
    match graph
        .resolve(Key::Recovery(id), Context::of(state, ability))
        .map(|effective| effective.setting)
    {
        Some(Setting::Recovery { ms, .. }) => crate::ticks_from_ms(ms),
        _ => recovery_ticks(id),
    }
}

/// An edge's window for this player before window modifiers, and its origin.
pub(super) fn window_for(
    graph: &ActionGraph,
    state: &PlayerState,
    def: &EdgeDef,
) -> (Window, Origin) {
    let context = Context::of(state, action_ability(state, def.from));
    match graph.resolve(Key::Window(def.id), context) {
        Some(Effective {
            setting: Setting::Window { window, .. },
            origin,
        }) => (window, origin),
        _ => (def.window, Origin::Baseline),
    }
}

/// Whether a rule applies to this player, and why.
pub(super) fn rule_applies(
    graph: &ActionGraph,
    state: &PlayerState,
    rule: &CompatibilityRule,
) -> Effective {
    let context = Context::of(state, action_ability(state, rule.during));
    graph
        .resolve(Key::Rule(rule.id), context)
        .unwrap_or(Effective {
            setting: Setting::Rule {
                rule: rule.id,
                applies: true,
            },
            origin: Origin::Baseline,
        })
}

pub(super) fn applies(graph: &ActionGraph, state: &PlayerState, rule: &CompatibilityRule) -> bool {
    matches!(
        rule_applies(graph, state, rule).setting,
        Setting::Rule { applies: true, .. }
    ) && all_hold(state, rule.conditions)
}

/// The directed slash's runtime: ticks since it resolved, and the opponent
/// its confirmed result damaged.
fn slash(graph: &ActionGraph, state: &PlayerState) -> Option<(u16, Option<EntityRef>)> {
    let recovery = recovery_for(
        graph,
        state,
        ActionId::DirectedSlash,
        Some(ids::HEROIC_STRIKE),
    );
    match state.combo {
        Some(combo) => Some((combo.elapsed, Some(combo.target))),
        None if state.strike_recovery > 0 => {
            Some((recovery.saturating_sub(state.strike_recovery), None))
        }
        None => None,
    }
}

/// Whether the slash's opening is past its window and kept only for late
/// inputs (#415): the slash no longer runs for anything else.
fn slash_in_late_grace(graph: &ActionGraph, state: &PlayerState) -> bool {
    let Some(def) = graph.edge(EdgeId::SlashToShieldBash) else {
        return false;
    };
    state
        .combo
        .is_some_and(|combo| combo.elapsed > effective_window(state, graph, def).closes())
}

/// The landing's runtime: ticks since the touchdown, while it recovers.
fn landing(state: &PlayerState) -> Option<u16> {
    let left = recovery_left(state, ActionId::Landing)?;
    let full = if state.landing.attacked_airborne {
        crate::OVERHEAD_LANDING_TICKS
    } else {
        crate::LANDING_RECOVERY_TICKS
    };
    Some(full.saturating_sub(left))
}

pub(super) fn holds(state: &PlayerState, condition: Condition) -> bool {
    match condition {
        Condition::Warden => state
            .class
            .is_some_and(|choice| choice.class == PlayerClass::Warden),
        Condition::Alive => state.is_alive(),
        // The opening exists only after positive damage to a living target;
        // a miss, a whiff, an absorbed blow or the target's death clears it.
        Condition::PositiveHealthDamage | Condition::TargetAlive => state.combo.is_some(),
        Condition::MultiHit => state.combo.is_some_and(crate::ComboOpening::cleaved),
        Condition::SwordMastery => {
            crate::melee::weapon_family(state) == crate::WeaponFamily::Sword
                && state.mastery.sword.level >= crate::LEAPING_SLASH_LEVEL
        }
        Condition::AttackedAirborne => state.landing.attacked_airborne,
        Condition::Airborne => state.landing.airborne,
        Condition::Stunned => aura::has_kind(&state.auras, AuraKind::Stun),
    }
}

/// Whether the player swims: alive, in deep water after the last physics
/// step.
fn swimming(state: &PlayerState) -> bool {
    state.swimming && state.is_alive()
}

/// The movement region's traversal actions, which hold while they last
/// rather than while they recover: a swim, a climb, a mantle and a roll.
pub(super) const fn lasting(id: ActionId) -> bool {
    matches!(
        id,
        ActionId::Swim | ActionId::Climb | ActionId::Mantle | ActionId::Roll
    )
}

/// Whether the lasting traversal action `id` runs now (#413, #490, #516).
pub(super) fn running(state: &PlayerState, id: ActionId) -> bool {
    match id {
        ActionId::Swim => swimming(state),
        ActionId::Roll => state.is_alive() && state.roll.rolling().is_some(),
        ActionId::Climb => {
            state.is_alive() && matches!(state.traversal, crate::Traversal::Climbing { .. })
        }
        ActionId::Mantle => {
            state.is_alive() && matches!(state.traversal, crate::Traversal::Mantling { .. })
        }
        _ => false,
    }
}

/// Whether a holding rule's action is in the phase it holds in: a swim,
/// climb or mantle while it lasts, any other action while it recovers.
fn holds_during(state: &PlayerState, id: ActionId) -> bool {
    match id {
        id if lasting(id) => running(state, id),
        // A timed swing (#679) holds from its wind-up through its recovery.
        ActionId::DirectedSlash => state.swing.is_some() || recovery_left(state, id).is_some(),
        _ => recovery_left(state, id).is_some(),
    }
}

fn all_hold(state: &PlayerState, conditions: &[Condition]) -> bool {
    conditions.iter().all(|&condition| holds(state, condition))
}

/// Whether `id` is in its recovery phase, with the ticks of it left.
pub(super) fn recovery_left(state: &PlayerState, id: ActionId) -> Option<u16> {
    let left = match id {
        ActionId::Landing => state.landing.recovery,
        ActionId::DirectedSlash => state.strike_recovery,
        ActionId::GuardBreak => state.guard.broken,
        ActionId::GlobalCooldown => state.global_cooldown,
        // The swing timer runs from the press; the wind-up is the strike's startup.
        ActionId::BasicMelee if state.melee.pending.is_none() => state.swing_timer,
        ActionId::Airborne
        | ActionId::Swim
        | ActionId::Climb
        | ActionId::Mantle
        | ActionId::Roll
        | ActionId::Guard
        | ActionId::Stun
        | ActionId::Death
        | ActionId::Cast
        | ActionId::BasicMelee
        | ActionId::Authored(_) => 0,
    };
    (left > 0).then_some(left)
}

/// The basic strike's phase (#885, #1221), read from its canonical state: winding up while
/// a strike is pending, recovering while the swing timer runs after its contact, following
/// up while a standing chain's window is still open after the recovery.
fn basic_melee(state: &PlayerState) -> Option<Phase> {
    if state.melee.pending.is_some() {
        Some(Phase::Startup)
    } else if recovery_left(state, ActionId::BasicMelee).is_some() {
        Some(Phase::Recovery)
    } else if state.melee.chain > 0 && state.melee.window > 0 {
        Some(Phase::FollowUp)
    } else {
        None
    }
}

/// The chain strike a standing basic press continues with: the next one while the chain's
/// window is open, else the first.
pub(crate) const fn basic_chain_step(state: &PlayerState) -> u8 {
    if state.melee.window > 0 {
        state.melee.chain
    } else {
        0
    }
}

/// Whether the player's pressed basic strike begins now (#1221). It waits out the previous
/// strike's wind-up and recovery, unless a hit opened the follow-up (#928) that cuts the
/// recovery short; a stun; and whatever holds or refuses melee: a Warden's landing, or a
/// graph that refuses attacks in water. Basic strikes share [`ActionInput::WardenMelee`]'s
/// compatibility rules but none of its edges, so no directed follow-up's effect (such as
/// the global-cooldown waiver) applies to them.
pub(crate) fn basic_melee_ready(graph: &ActionGraph, state: &PlayerState) -> bool {
    let input = ActionInput::WardenMelee;
    state.is_alive()
        && state.swing_queued
        && state.melee.pending.is_none()
        && (state.swing_timer == 0 || state.melee.follow_up > 0)
        && holding_rule(graph, state, input).is_none()
        && refusing_rule(graph, state, input).is_none()
        && !aura::has_kind(&state.auras, AuraKind::Stun)
}

/// What the reactions in effect do to the player's basic strike now; the baseline's stun
/// cancels a wind-up.
pub(crate) fn basic_melee_interruption(graph: &ActionGraph, state: &PlayerState) -> Consequence {
    consequence(graph, state, Interrupted::Action(ActionId::BasicMelee))
}

/// The ticks a running cast has left, with its ability.
pub(super) fn cast_left(state: &PlayerState) -> Option<(AbilityId, u16)> {
    let cast = state.cast?;
    let ticks =
        crate::ability::ability_by_id(cast.ability).map_or(0, |ability| ability.cast.ticks());
    Some((cast.ability, ticks.saturating_sub(cast.elapsed)))
}

/// The rule of `graph` that currently holds `input`.
pub(crate) fn holding_rule(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
) -> Option<&'static CompatibilityRule> {
    graph.rules.iter().find(|rule| {
        rule.effect == RuleEffect::Hold
            && rule.input == input
            && holds_during(state, rule.during)
            && applies(graph, state, rule)
    })
}

/// The rule of `graph` that refuses `input` now.
pub(crate) fn refusing_rule(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
) -> Option<&'static CompatibilityRule> {
    graph.rules.iter().find(|rule| {
        rule.effect == RuleEffect::Refuse
            && rule.input == input
            && rule.during == ActionId::Swim
            && swimming(state)
            && applies(graph, state, rule)
    })
}

/// The rule of `graph` by which `input` now ends a running action.
pub(crate) fn cancelling_rule(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
) -> Option<&'static CompatibilityRule> {
    graph.rules.iter().find(|rule| {
        rule.effect == RuleEffect::Cancel
            && rule.input == input
            && rule.during == ActionId::Cast
            && state.cast.is_some()
            && applies(graph, state, rule)
    })
}

/// The running action and phase of each region.
pub(super) fn region_actions(graph: &ActionGraph, state: &PlayerState) -> [RegionReport; 4] {
    let report = |region, action: Option<ActionId>, phase, recovery_left| RegionReport {
        region,
        action,
        phase,
        recovery_left,
        ability: action.and_then(|id| action_ability(state, id)),
        active_left: None,
        strike: None,
        follow_up_left: None,
    };
    let traversing = [
        ActionId::Roll,
        ActionId::Mantle,
        ActionId::Climb,
        ActionId::Swim,
    ]
    .into_iter()
    .find(|&id| running(state, id));
    let movement = if let Some(id) = traversing {
        report(Region::Movement, Some(id), Some(Phase::Active), None)
    } else if state.landing.airborne {
        report(
            Region::Movement,
            Some(ActionId::Airborne),
            Some(Phase::Active),
            None,
        )
    } else if let Some(left) = recovery_left(state, ActionId::Landing) {
        report(
            Region::Movement,
            Some(ActionId::Landing),
            Some(Phase::Recovery),
            Some(left),
        )
    } else {
        report(Region::Movement, None, None, None)
    };
    // A cast outranks the slash, the slash an authored action, that a basic strike, and all
    // of them outrank the global cooldown they run alongside.
    let offense = if let Some((_, left)) = cast_left(state) {
        RegionReport {
            active_left: Some(left),
            ..report(
                Region::Offense,
                Some(ActionId::Cast),
                Some(Phase::Active),
                None,
            )
        }
    } else if let Some(swing) = state.swing {
        // A timed swing (#679) winds up, then sweeps.
        let phase = match swing.phase() {
            crate::SwingPhase::Startup => Phase::Startup,
            crate::SwingPhase::Active => Phase::Active,
        };
        RegionReport {
            active_left: Some(swing.active_left()),
            ..report(
                Region::Offense,
                Some(ActionId::DirectedSlash),
                Some(phase),
                None,
            )
        }
    } else if slash(graph, state).is_some() && !slash_in_late_grace(graph, state) {
        match recovery_left(state, ActionId::DirectedSlash) {
            Some(left) => report(
                Region::Offense,
                Some(ActionId::DirectedSlash),
                Some(Phase::Recovery),
                Some(left),
            ),
            None => report(
                Region::Offense,
                Some(ActionId::DirectedSlash),
                Some(Phase::FollowUp),
                None,
            ),
        }
    } else if let Some((def, phase)) = state.authored.and_then(|run| run.phase(graph)) {
        // An authored action (#414): its ticks left in the active phase or
        // its recovery.
        let elapsed = state.authored.map_or(0, |run| run.elapsed);
        let [startup, active, recovery] = def.ticks();
        let since = elapsed.saturating_sub(startup);
        RegionReport {
            ability: def.trigger.ability,
            active_left: (phase == Phase::Active).then(|| active.saturating_sub(since)),
            ..report(
                Region::Offense,
                Some(ActionId::Authored(def.slot)),
                Some(phase),
                (phase == Phase::Recovery).then(|| (active + recovery).saturating_sub(since)),
            )
        }
    } else if let Some(phase) = basic_melee(state) {
        // The pending strike, else the one that struck; a press follows it up during the
        // hit's follow-up (#928) or the chain's window.
        let melee = state.melee;
        RegionReport {
            strike: melee.pending.map(|pending| pending.variant).or(melee.last),
            follow_up_left: match phase {
                Phase::Recovery => (melee.follow_up > 0).then_some(melee.follow_up),
                Phase::FollowUp => Some(melee.window),
                Phase::Startup | Phase::Active => None,
            },
            ..report(
                Region::Offense,
                Some(ActionId::BasicMelee),
                Some(phase),
                recovery_left(state, ActionId::BasicMelee),
            )
        }
    } else if let Some(left) = recovery_left(state, ActionId::GlobalCooldown) {
        report(
            Region::Offense,
            Some(ActionId::GlobalCooldown),
            Some(Phase::Recovery),
            Some(left),
        )
    } else {
        report(Region::Offense, None, None, None)
    };
    let defense = match state.guard.raised {
        Some(_) => report(
            Region::Defense,
            Some(ActionId::Guard),
            Some(Phase::Active),
            None,
        ),
        None => report(Region::Defense, None, None, None),
    };
    let reaction = match active_reactions(state).first() {
        Some(&ActionId::GuardBreak) => report(
            Region::Reaction,
            Some(ActionId::GuardBreak),
            Some(Phase::Recovery),
            recovery_left(state, ActionId::GuardBreak),
        ),
        Some(&reaction) => report(Region::Reaction, Some(reaction), Some(Phase::Active), None),
        None => report(Region::Reaction, None, None, None),
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
    let regions = region_actions(graph, state);
    let phase_of = |id: ActionId| {
        regions
            .iter()
            .find(|report| report.action == Some(id))
            .and_then(|report| report.phase)
    };
    match target {
        // A basic strike may run beneath a cast or a slash the offense region reports.
        Interrupted::Action(ActionId::BasicMelee) => basic_melee(state),
        Interrupted::Action(id) => phase_of(id),
        Interrupted::Edge(id) => {
            let def = graph.edge(id)?;
            // Only an opening (a confirmed source) can be interrupted.
            if def.from == ActionId::DirectedSlash {
                state.combo?;
            }
            phase_of(def.from)
        }
        Interrupted::Held(id) => {
            let held = graph.rules.iter().find(|rule| {
                rule.id == id
                    && holding_rule(graph, state, rule.input).is_some_and(|by| by.id == id)
                    && match rule.input {
                        ActionInput::GuardProtection => state.guard.raised.is_some(),
                        input => state
                            .intents
                            .iter()
                            .any(|queued| input_of(&queued.intent) == Some(input)),
                    }
            })?;
            // A swim, climb or mantle holds while it runs; any other action while it recovers.
            Some(if lasting(held.during) {
                Phase::Active
            } else {
                Phase::Recovery
            })
        }
    }
}

/// The consequence `graph` authors for `reaction` on `target` in `phase`;
/// preserve without a rule.
pub(crate) fn authored_consequence(
    graph: &ActionGraph,
    reaction: ActionId,
    target: Interrupted,
    phase: Phase,
) -> Consequence {
    graph
        .interruptions
        .iter()
        .find(|rule| {
            rule.reaction == reaction
                && rule.interrupted == target
                && rule.phase.is_none_or(|only| only == phase)
        })
        .map_or(Consequence::Preserve, |rule| rule.consequence)
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

/// An edge's window after its overrides and the modifiers whose conditions
/// hold.
pub(crate) fn effective_window(
    state: &PlayerState,
    graph: &ActionGraph,
    def: &EdgeDef,
) -> TickWindow {
    let mut window = window_for(graph, state, def).0.ticks();
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
pub(super) struct EdgeEval {
    pub(super) status: EdgeStatus,
    /// Ticks since the anchor, while the source runs.
    pub(super) elapsed: Option<u16>,
    pub(super) window: TickWindow,
    /// An input now falls inside the precision window.
    precise: bool,
    /// Open only because the input is late.
    compensated: bool,
}

/// Evaluates `def` for an input `late` ticks late (see [`LATE_INPUT_GRACE_MS`]).
pub(super) fn evaluate(
    graph: &ActionGraph,
    state: &PlayerState,
    def: &EdgeDef,
    late: u8,
) -> EdgeEval {
    let window = effective_window(state, graph, def);
    let source = match def.from {
        ActionId::DirectedSlash => slash(graph, state).map(|(elapsed, _)| elapsed),
        ActionId::Landing => landing(state),
        ActionId::Authored(slot) => state
            .authored
            .filter(|run| run.slot == slot)
            .and_then(|run| run.since_impact()),
        _ => None,
    };
    let Some(elapsed) = source else {
        return EdgeEval {
            status: EdgeStatus::Blocked(BlockReason::SourceInactive),
            elapsed: None,
            window,
            precise: false,
            compensated: false,
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
    // A late input may still catch a window that closed while it travelled
    // (`LATE_INPUT_GRACE_MS`), unless a condition or a reaction refuses it now.
    let earliest = elapsed.saturating_sub(u16::from(late));
    let compensated = status == EdgeStatus::Blocked(BlockReason::Expired)
        && interrupted.is_none_or(|(consequence, ..)| consequence == Consequence::Preserve)
        && earliest <= window.closes();
    // Otherwise an opening kept only for late inputs is gone, as before.
    if !compensated && def.from == ActionId::DirectedSlash && slash_in_late_grace(graph, state) {
        return EdgeEval {
            status: EdgeStatus::Blocked(BlockReason::SourceInactive),
            elapsed: None,
            window,
            precise: false,
            compensated: false,
        };
    }
    let status = if compensated {
        EdgeStatus::Open
    } else {
        status
    };
    // Precision ignores lateness (owner, #415 Q2): only an input resolving
    // inside the sweet spot on arrival is precise.
    let precise = status == EdgeStatus::Open
        && !compensated
        && def.precision.is_some_and(|precision| {
            let (from, until) = precision.ticks(window.opens);
            (from..=until).contains(&elapsed)
        });
    EdgeEval {
        status,
        elapsed: Some(elapsed),
        window,
        precise,
        compensated,
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
    /// A compatibility rule refuses the input.
    RefusedBy(RuleId),
    /// The running authored action in this slot holds the input (#414).
    HeldByAuthored(u8),
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
    /// The input waits in the queue until the authored action in this slot
    /// ends (#414).
    WaitForAuthored(u8),
    /// The input resolves now as its ordinary use and, by this rule, ends
    /// the running action (a cast).
    Cancel(RuleId),
    /// The input is refused now by this rule (an attack in water).
    Refuse(RuleId),
}

/// The explained arbitration of one input (Atlas 5.7.4).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Arbitration {
    pub input: ActionInput,
    pub outcome: Outcome,
    pub candidates: Vec<(Candidate, Verdict)>,
    /// The winning edge resolves inside its precision window.
    pub precise: bool,
    /// The ticks of lateness the input may make up for, at most
    /// [`LATE_INPUT_GRACE_TICKS`]; 0 for an input without an observation.
    pub late: u8,
    /// The winning edge had closed when the input arrived; its lateness
    /// kept it open.
    pub compensated: bool,
}

/// Arbitrates `input`, arriving `late` ticks late, for a player in `state`.
pub(crate) fn arbitrate(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
    late: u8,
) -> Arbitration {
    arbitrate_with(graph, state, input, true, late)
}

/// [`arbitrate`], optionally without the running authored action's hold.
pub(crate) fn arbitrate_with(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
    authored_hold: bool,
    late: u8,
) -> Arbitration {
    let late = late.min(LATE_INPUT_GRACE_TICKS);
    let mut edges: Vec<(&EdgeDef, EdgeEval)> = graph
        .edges
        .iter()
        .filter(|def| def.input == input)
        .map(|def| (def, evaluate(graph, state, def, late)))
        .collect();
    // Highest priority first; ties in authored order.
    edges.sort_by_key(|(def, _)| std::cmp::Reverse(def.priority));
    // An open edge out of the action a rule holds the input during is that
    // action's authored way out: it wins over the hold (a landing follow-up
    // ends the landing recovery).
    let hold = holding_rule(graph, state, input).filter(|rule| {
        !edges
            .iter()
            .any(|(def, eval)| def.from == rule.during && eval.status == EdgeStatus::Open)
    });
    // A refusal outranks everything: no edge leads into a refused attack.
    let refusal = refusing_rule(graph, state, input);
    // A running authored action holds other offense inputs, unless an edge
    // out of it is open (#414).
    let authored = authored_hold
        .then(|| authored::holds_input(state, input))
        .flatten()
        .filter(|&slot| {
            !edges.iter().any(|(def, eval)| {
                def.from == ActionId::Authored(slot) && eval.status == EdgeStatus::Open
            })
        });
    let outcome = if let Some(rule) = refusal {
        Outcome::Refuse(rule.id)
    } else if let Some(rule) = hold {
        Outcome::WaitForRule(rule.id)
    } else if let Some(slot) = authored {
        Outcome::WaitForAuthored(slot)
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
            .unwrap_or_else(|| match cancelling_rule(graph, state, input) {
                Some(rule) => Outcome::Cancel(rule.id),
                None => Outcome::Proceed(Candidate::Direct),
            })
    };
    let won = |def: &EdgeDef| outcome == Outcome::Proceed(Candidate::Edge(def.id));
    let precise = edges.iter().any(|(def, eval)| won(def) && eval.precise);
    let compensated = edges.iter().any(|(def, eval)| won(def) && eval.compensated);
    let verdict = |candidate: Candidate, status: Option<EdgeStatus>| -> Verdict {
        if let Some(rule) = refusal {
            return Verdict::RefusedBy(rule.id);
        }
        if let Some(rule) = hold {
            return Verdict::HeldBy(rule.id);
        }
        if let Some(slot) = authored {
            return Verdict::HeldByAuthored(slot);
        }
        match (outcome, status) {
            (Outcome::Proceed(winner), _) if winner == candidate => Verdict::Won,
            (Outcome::Cancel(_), _) if candidate == Candidate::Direct => Verdict::Won,
            (Outcome::WaitForEdge(id), _) if Candidate::Edge(id) == candidate => Verdict::Holding,
            (_, Some(EdgeStatus::Blocked(reason))) => Verdict::Blocked(reason),
            (Outcome::Proceed(winner), _) => Verdict::Outranked(winner),
            (Outcome::Cancel(_), _) => Verdict::Outranked(Candidate::Direct),
            (Outcome::WaitForEdge(id), _) => Verdict::Outranked(Candidate::Edge(id)),
            (Outcome::WaitForRule(rule), _) => Verdict::HeldBy(rule),
            (Outcome::Refuse(rule), _) => Verdict::RefusedBy(rule),
            (Outcome::WaitForAuthored(slot), _) => Verdict::HeldByAuthored(slot),
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
        late,
        compensated,
    }
}

/// Whether a queued intent waits in the queue rather than resolving now.
pub(crate) fn waits(graph: &ActionGraph, state: &PlayerState, queued: &QueuedIntent) -> bool {
    input_of(&queued.intent).is_some_and(|input| {
        matches!(
            arbitrate(graph, state, input, queued.late).outcome,
            Outcome::WaitForRule(_) | Outcome::WaitForEdge(_) | Outcome::WaitForAuthored(_)
        )
    })
}

/// Whether a rule refuses an intent now.
pub(crate) fn refuses(graph: &ActionGraph, state: &PlayerState, intent: &PlayerIntent) -> bool {
    input_of(intent).is_some_and(|input| refusing_rule(graph, state, input).is_some())
}

/// An input resolving through an edge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FollowUp {
    /// The edge; `None` for an authored action's own use (#414).
    pub(crate) edge: Option<EdgeId>,
    /// The action the edge leaves.
    pub(crate) from: ActionId,
    /// The source's confirmed target, with [`Effect::TargetConfirmed`].
    pub(crate) target: Option<EntityRef>,
    /// The edge's effects, then the precision benefits if it was precise.
    pub(crate) effects: Vec<Effect>,
}

impl FollowUp {
    /// The launch the follow-up gives, `[forward, up]` in cm/s.
    pub(crate) fn impulse(&self) -> Option<[i32; 2]> {
        self.effects.iter().find_map(|effect| match *effect {
            Effect::Impulse {
                forward_cms,
                up_cms,
            } => Some([i32::from(forward_cms), i32::from(up_cms)]),
            _ => None,
        })
    }

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

/// The edge `input`, `late` ticks late, resolves through now, with its effects.
pub(crate) fn follow_up(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
    late: u8,
) -> Option<FollowUp> {
    // The use an authored action's active phase resolves carries its
    // effects (#414).
    if let Some(run) = state.authored.filter(|run| run.releasing)
        && let Some(def) = graph.authored_action(run.slot)
        && def.trigger.input == input
    {
        return Some(FollowUp {
            edge: None,
            from: ActionId::Authored(run.slot),
            target: None,
            effects: def.effects.to_vec(),
        });
    }
    let arbitration = arbitrate(graph, state, input, late);
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
        ActionId::DirectedSlash => slash(graph, state).and_then(|(_, target)| target),
        _ => None,
    }
    .filter(|_| effects.contains(&Effect::TargetConfirmed));
    Some(FollowUp {
        edge: Some(id),
        from: def.from,
        target,
        effects,
    })
}

/// The authored movement of one running action phase, after overrides.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MovementSource {
    pub action: ActionId,
    pub phase: Phase,
    pub movement: Movement,
    pub origin: Origin,
}

/// How a player moves now (Atlas 5.9.2): the combination of every running
/// action phase's authored movement, and those phases.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MovementReport {
    /// [`Movement::FREE`] without a source.
    pub movement: Movement,
    pub sources: Vec<MovementSource>,
}

/// The authored movement of each region's running action phase whose
/// conditions hold.
pub(super) fn movement_sources(graph: &ActionGraph, state: &PlayerState) -> Vec<MovementSource> {
    // A running authored action's phase moves as it authors (#414); a free
    // phase is no source.
    let authored = state
        .authored
        .and_then(|run| run.phase(graph))
        .map(|(def, phase)| MovementSource {
            action: ActionId::Authored(def.slot),
            phase,
            movement: def.movement(phase),
            origin: Origin::Authored(def.slot),
        })
        .filter(|source| source.movement != Movement::FREE);
    if graph.movement.is_empty() {
        return authored.into_iter().collect();
    }
    region_actions(graph, state)
        .iter()
        .filter_map(|report| {
            let (id, phase) = (report.action?, report.phase?);
            let rule = graph.movement_rule(id, phase)?;
            if !all_hold(state, rule.conditions) {
                return None;
            }
            let context = Context::of(state, action_ability(state, id));
            match graph.resolve(Key::Movement(id, phase), context)? {
                Effective {
                    setting: Setting::Movement { movement, .. },
                    origin,
                } => Some(MovementSource {
                    action: id,
                    phase,
                    movement,
                    origin,
                }),
                _ => None,
            }
        })
        .chain(authored)
        .collect()
}

/// The player's combined authored movement; `None` when it moves freely.
pub(crate) fn movement_of(graph: &ActionGraph, state: &PlayerState) -> Option<Movement> {
    movement_sources(graph, state)
        .into_iter()
        .map(|source| source.movement)
        .reduce(Movement::combine)
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
        // A running authored action belongs to the graph it started under.
        for player in self.players.values_mut() {
            player.authored = None;
            player.holds = authored::Holds::default();
        }
        Ok(())
    }

    /// The committed recovery `id` starts when the player uses `ability`, in
    /// ticks, after overrides.
    pub(crate) fn recovery_of(&self, player_id: PlayerId, id: ActionId, ability: AbilityId) -> u16 {
        self.players.get(&player_id).map_or_else(
            || recovery_ticks(id),
            |state| recovery_for(self.action_graph, state, id, Some(ability)),
        )
    }

    /// The edge the player's `input` follows now, with its effects.
    pub(crate) fn follow_up_of(&self, player_id: PlayerId, input: ActionInput) -> Option<FollowUp> {
        let state = self.players.get(&player_id)?;
        follow_up(self.action_graph, state, input, self.resolving_late)
    }

    /// Applies a resolved follow-up's movement: leaving a landing ends its
    /// recovery, and an impulse launches the player in this tick's movement.
    pub(crate) fn spend_follow_up(&mut self, player_id: PlayerId, follow_up: &FollowUp) {
        let impulse = follow_up.impulse();
        let from = follow_up.from;
        let edge = follow_up.edge.is_some();
        self.update_player(player_id, |player| {
            if from == ActionId::Landing {
                player.landing.recovery = 0;
                player.landing.attacked_airborne = false;
            }
            // Following an edge out of an authored action ends it (#414).
            if edge && matches!(from, ActionId::Authored(_)) {
                player.authored = None;
            }
            if impulse.is_some() {
                player.impulse = impulse;
            }
        });
    }

    /// Whether a stun landing on the player now cancels its cast.
    pub(crate) fn stun_cancels_cast(&self) -> bool {
        authored_consequence(
            self.action_graph,
            ActionId::Stun,
            Interrupted::Action(ActionId::Cast),
            Phase::Active,
        ) == Consequence::Cancel
    }

    /// The players whose movement or jump this tick ends their cast.
    pub(crate) fn moves_cancelling_casts(&self) -> Vec<PlayerId> {
        let graph = self.action_graph;
        self.players
            .iter()
            .filter(|(_, state)| {
                let moving = state.forward != crate::zone::Axis::Zero
                    || state.strafe != crate::zone::Axis::Zero
                    || state.rise != crate::zone::Axis::Zero;
                (moving && cancelling_rule(graph, state, ActionInput::Move).is_some())
                    || (state.jump_pending
                        && cancelling_rule(graph, state, ActionInput::Jump).is_some())
            })
            .map(|(&player_id, _)| player_id)
            .collect()
    }

    /// Applies the reactions' cancellations to every player, before the
    /// tick's timers advance. Suspensions are read where they apply. A stun
    /// already interrupts a cast as it lands, and death clears it, so the
    /// cast cancellation here only catches a restored state.
    pub(crate) fn apply_interruptions(&mut self) {
        let graph = self.action_graph;
        let mut casts = Vec::new();
        for (&player_id, player) in &mut self.players {
            for rule in graph.rules {
                if consequence(graph, player, Interrupted::Held(rule.id)) == Consequence::Cancel {
                    if rule.input == ActionInput::GuardProtection {
                        player.guard.raised = None;
                    }
                    player
                        .intents
                        .retain(|queued| input_of(&queued.intent) != Some(rule.input));
                }
            }
            if consequence(graph, player, Interrupted::Action(ActionId::Guard))
                == Consequence::Cancel
            {
                player.guard.raised = None;
            }
            if consequence(graph, player, Interrupted::Action(ActionId::Cast))
                == Consequence::Cancel
            {
                casts.push(player_id);
            }
        }
        for player_id in casts {
            self.interrupt_cast(EntityRef::Player(player_id), None, None);
        }
    }
}
