//! Authored actions (#414, Atlas 5.9.4–5.9.5): new experimental actions
//! composed from the graph's existing primitives.
//!
//! An [`AuthoredAction`] is a named offense-region action of one class. Its
//! [`Trigger`] is an ordinary semantic input (Warden melee, Shield Bash or
//! another class ability, optionally one ability) in an [`InputContext`]: the
//! plain binding, direction + action or jump + attack. It runs three timed
//! phases, startup, active and recovery, each with bounded [`Movement`]:
//!
//! - **Startup.** The triggering input waits in the intent queue, exactly
//!   like an input a compatibility rule holds.
//! - **Active.** When startup ends the input resolves through its ordinary
//!   path, so the ability's own eligibility, cost, cooldowns, range and hit
//!   resolution decide the combat outcome. The action's typed [`Effect`]s
//!   (an impulse, a recovery, a refund, a waived global cooldown) apply to
//!   that use like an edge's. A refused use ends the action.
//! - **Recovery.** Other offense inputs wait until the action ends, unless an
//!   outgoing edge ([`EdgeId::Authored`], anchored at the use's
//!   [`Anchor::Impact`]) is open: following it ends the action, as a landing
//!   edge ends a landing.
//!
//! A stun or death ends a running authored action. Nothing here is a second
//! executor: the zone only times the phases, and the follow-up runs through
//! the ordinary intent, edge and movement code. The production graph authors
//! none, and like the graph itself a running authored action is zone setup,
//! not canonical state: a zone restored from a snapshot runs the baseline.

use super::{
    ActionGraph, ActionId, ActionInput, Anchor, Condition, EdgeDef, EdgeId, Effect, GraphError,
    MAX_IMPULSE_FORWARD_CMS, MAX_IMPULSE_UP_CMS, Movement, Outcome, Phase, duration_ok, input_of,
    validate_movement_values,
};
use crate::snapshot::PlayerIntent;
use crate::zone::{Axis, PlayerState};
use crate::{AbilityId, AbilityUser, PlayerClass, PlayerId, ZoneError, ZoneSimulation, aura};

/// The most authored actions one graph may have; their slots are below it.
pub const MAX_AUTHORED_ACTIONS: u8 = 8;
/// The most edges out of authored actions; their ids are below it.
pub const MAX_AUTHORED_EDGES: u8 = 16;
/// The longest authored action, all three phases together.
pub const MAX_AUTHORED_MS: u32 = 5_000;
/// The longest authored action name, in bytes.
pub const MAX_AUTHORED_NAME: usize = 40;

/// Which held movement a direction + action trigger needs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Direction {
    /// Any movement intent.
    Any,
    Forward,
    Backward,
    /// Strafing left or right.
    Sideways,
}

/// The context an input must arrive in (Atlas 5.9.5). More specific
/// contexts win over less specific ones for the same input.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InputContext {
    /// The plain binding.
    Direct,
    /// Jump + attack: while airborne.
    Airborne,
    /// Direction + action: while the movement intent holds this direction.
    Moving(Direction),
}

impl InputContext {
    const fn rank(self) -> u8 {
        match self {
            Self::Direct => 0,
            Self::Moving(Direction::Any) => 1,
            Self::Airborne | Self::Moving(_) => 2,
        }
    }

    fn holds(self, state: &PlayerState) -> bool {
        match self {
            Self::Direct => true,
            Self::Airborne => state.landing.airborne,
            Self::Moving(direction) => match direction {
                Direction::Any => state.forward != Axis::Zero || state.strafe != Axis::Zero,
                Direction::Forward => state.forward == Axis::Positive,
                Direction::Backward => state.forward == Axis::Negative,
                Direction::Sideways => state.strafe != Axis::Zero,
            },
        }
    }
}

/// The semantic input that starts an authored action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Trigger {
    /// Warden melee, Shield Bash or another class ability.
    pub input: ActionInput,
    /// Only this ability's use; `None` is any use the input covers.
    pub ability: Option<AbilityId>,
    pub context: InputContext,
}

/// One timed phase of an authored action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AuthoredPhase {
    pub ms: u32,
    pub movement: Movement,
}

impl AuthoredPhase {
    /// A phase of `ms` that moves freely.
    #[must_use]
    pub const fn free(ms: u32) -> Self {
        Self {
            ms,
            movement: Movement::FREE,
        }
    }
}

/// A new experimental action composed from existing primitives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoredAction {
    /// Its stable identity in the graph, below [`MAX_AUTHORED_ACTIONS`].
    pub slot: u8,
    pub name: &'static str,
    pub class: PlayerClass,
    pub trigger: Trigger,
    /// Conditions that must hold when the trigger arrives.
    pub conditions: &'static [Condition],
    pub startup: AuthoredPhase,
    pub active: AuthoredPhase,
    pub recovery: AuthoredPhase,
    /// Applied to the use the active phase resolves: an impulse, a recovery
    /// (the global cooldown it starts), a refund or a waived global cooldown.
    pub effects: &'static [Effect],
}

impl AuthoredAction {
    /// Startup, active and recovery in ticks.
    #[must_use]
    pub const fn ticks(&self) -> [u16; 3] {
        [
            crate::ticks_from_ms(self.startup.ms),
            crate::ticks_from_ms(self.active.ms),
            crate::ticks_from_ms(self.recovery.ms),
        ]
    }

    /// The phase `elapsed` ticks after the trigger, once the use has (or has
    /// not yet) resolved; `None` once it is over.
    #[must_use]
    pub const fn phase_at(&self, elapsed: u16, released: bool) -> Option<Phase> {
        let [startup, active, recovery] = self.ticks();
        if !released {
            return if elapsed <= startup {
                Some(Phase::Startup)
            } else {
                None
            };
        }
        let since = elapsed.saturating_sub(startup);
        if since < active {
            Some(Phase::Active)
        } else if since < active + recovery {
            Some(Phase::Recovery)
        } else {
            None
        }
    }

    /// The movement of `phase`.
    #[must_use]
    pub const fn movement(&self, phase: Phase) -> Movement {
        match phase {
            Phase::Startup => self.startup.movement,
            Phase::Active => self.active.movement,
            Phase::Recovery | Phase::FollowUp => self.recovery.movement,
        }
    }

    /// The ticks from the use (the active phase's start) to the end.
    #[must_use]
    pub const fn after_impact_ticks(&self) -> u16 {
        let [_, active, recovery] = self.ticks();
        active + recovery
    }

    fn matches(&self, input: ActionInput, ability: Option<AbilityId>) -> bool {
        self.trigger.input == input
            && self
                .trigger
                .ability
                .is_none_or(|only| Some(only) == ability)
    }
}

/// What validation found wrong with an authored action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthoredProblem {
    /// The slot is not below [`MAX_AUTHORED_ACTIONS`].
    Slot,
    /// The name is empty or longer than [`MAX_AUTHORED_NAME`].
    Name,
    /// A phase is shorter than half a tick (but not zero), the active phase
    /// is under a tick, or all three outlast [`MAX_AUTHORED_MS`].
    Duration,
    /// No runtime starts an action from this input: only Warden melee and
    /// Shield Bash (for Wardens) and other class abilities do. Movement,
    /// jump and guard triggers need a primitive that does not exist yet.
    Input,
    /// The ability is not one of the class's, or the input does not cover it.
    Ability,
    /// A phase's movement is out of range.
    Movement,
    /// Only one impulse, recovery, refund (not zero) and the global-cooldown
    /// waiver are allowed; a confirmed target or a kept opening need an edge
    /// source that has them.
    Effect,
}

/// The input and ability an intent makes.
pub(crate) fn intent_input(intent: &PlayerIntent) -> Option<(ActionInput, AbilityId, bool)> {
    let input = input_of(intent)?;
    match *intent {
        PlayerIntent::UseAbility { ability, .. } => Some((input, AbilityId::new(ability), false)),
        PlayerIntent::DirectedStrike { ability, .. } => {
            Some((input, AbilityId::new(ability), true))
        }
        _ => None,
    }
}

impl ActionGraph {
    /// The authored action in `slot`.
    #[must_use]
    pub fn authored_action(&self, slot: u8) -> Option<&'static AuthoredAction> {
        self.authored.iter().find(|def| def.slot == slot)
    }

    pub(super) fn validate_authored(
        &self,
        index: usize,
        def: &AuthoredAction,
    ) -> Result<(), GraphError> {
        let bad = |problem| Err(GraphError::BadAuthored(def.slot, problem));
        if def.slot >= MAX_AUTHORED_ACTIONS {
            return bad(AuthoredProblem::Slot);
        }
        if self.authored[..index]
            .iter()
            .any(|other| other.slot == def.slot)
        {
            return Err(GraphError::DuplicateAuthored(def.slot));
        }
        if def.name.trim().is_empty() || def.name.len() > MAX_AUTHORED_NAME {
            return bad(AuthoredProblem::Name);
        }
        let phases = [def.startup, def.active, def.recovery];
        let total: u64 = phases.iter().map(|phase| u64::from(phase.ms)).sum();
        if !phases.iter().all(|phase| duration_ok(phase.ms))
            || crate::ticks_from_ms(def.active.ms) == 0
            || total > u64::from(MAX_AUTHORED_MS)
        {
            return bad(AuthoredProblem::Duration);
        }
        let warden_input = matches!(
            def.trigger.input,
            ActionInput::WardenMelee | ActionInput::ShieldBash
        );
        if !((warden_input && def.class == PlayerClass::Warden)
            || def.trigger.input == ActionInput::Ability)
        {
            return bad(AuthoredProblem::Input);
        }
        if let Some(id) = def.trigger.ability {
            let covers = crate::ability::ability_by_id(id).is_some_and(|ability| {
                ability.user == AbilityUser::Class(def.class)
                    && input_of(&PlayerIntent::UseAbility {
                        ability: id.get(),
                        target: None,
                    }) == Some(def.trigger.input)
            });
            if !covers {
                return bad(AuthoredProblem::Ability);
            }
        }
        if !phases
            .iter()
            .all(|phase| validate_movement_values(phase.movement))
        {
            return bad(AuthoredProblem::Movement);
        }
        let (mut impulses, mut recoveries) = (0, 0);
        for effect in def.effects {
            match *effect {
                Effect::IgnoreGlobalCooldown => {}
                Effect::Recovery { ms } if duration_ok(ms) => recoveries += 1,
                Effect::ResourceRefund { amount } if amount > 0 => {}
                Effect::Impulse {
                    forward_cms,
                    up_cms,
                } if forward_cms.unsigned_abs() <= MAX_IMPULSE_FORWARD_CMS
                    && up_cms <= MAX_IMPULSE_UP_CMS
                    && (forward_cms != 0 || up_cms != 0) =>
                {
                    impulses += 1;
                }
                _ => return bad(AuthoredProblem::Effect),
            }
        }
        if impulses > 1 || recoveries > 1 {
            return bad(AuthoredProblem::Effect);
        }
        if let Some(other) = self.authored[..index]
            .iter()
            .find(|other| other.class == def.class && other.trigger == def.trigger)
        {
            return Err(GraphError::ConflictingTrigger(other.slot, def.slot));
        }
        Ok(())
    }

    /// The checks an edge out of an authored action passes: its source
    /// exists, it answers an offense input from the use's impact, and it
    /// has neither a target to confirm nor an opening to keep.
    pub(super) fn validate_authored_edge(&self, def: &EdgeDef) -> Result<(), GraphError> {
        let ActionId::Authored(slot) = def.from else {
            return Err(GraphError::UnsupportedEdge(def.id));
        };
        let supported = matches!(def.id, EdgeId::Authored(id) if id < MAX_AUTHORED_EDGES)
            && def.anchor == Anchor::Impact
            && matches!(
                def.input,
                ActionInput::WardenMelee | ActionInput::ShieldBash | ActionInput::Ability
            );
        if !supported {
            return Err(GraphError::UnsupportedEdge(def.id));
        }
        if self.authored_action(slot).is_none() {
            return Err(GraphError::UnknownAuthored(slot));
        }
        let effects = def.effects.iter().chain(
            def.precision
                .iter()
                .flat_map(|precision| precision.benefits),
        );
        if effects
            .clone()
            .any(|effect| matches!(effect, Effect::TargetConfirmed | Effect::KeepOpening))
        {
            return Err(GraphError::BadEffect(def.id));
        }
        Ok(())
    }
}

/// A running authored action. Zone setup like the graph, so not canonical.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AuthoredRun {
    pub(crate) slot: u8,
    /// Ticks since the trigger.
    pub(crate) elapsed: u16,
    /// The triggering use has resolved: the active phase has begun.
    pub(crate) released: bool,
    /// The triggering use is resolving now.
    pub(crate) releasing: bool,
}

impl AuthoredRun {
    pub(crate) fn phase(self, graph: &ActionGraph) -> Option<(&'static AuthoredAction, Phase)> {
        let def = graph.authored_action(self.slot)?;
        Some((def, def.phase_at(self.elapsed, self.released)?))
    }

    /// Ticks since the use resolved, once it has.
    pub(crate) fn since_impact(self, graph: &ActionGraph) -> Option<u16> {
        let def = graph.authored_action(self.slot)?;
        self.released
            .then(|| self.elapsed.saturating_sub(def.ticks()[0]))
    }
}

/// Why an authored action would not start from its trigger now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerRefusal {
    /// The player is not of the action's class.
    Class,
    /// The trigger's context does not hold.
    Context(InputContext),
    /// A condition does not hold.
    Condition(Condition),
    /// This or another authored action is running.
    Busy(u8),
    /// The ordinary graph does not let the input resolve directly now: a
    /// rule holds it, an edge answers or buffers it, or it cancels a cast.
    Arbitration(Outcome),
}

/// How the zone treats an authored action's trigger now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriggerStatus {
    /// Running in this phase, `elapsed` ticks after its trigger.
    Running {
        phase: Phase,
        elapsed: u16,
    },
    /// The trigger, arriving now, starts it (the ability's own readiness
    /// still applies).
    Accepted,
    /// The trigger arrives in its context, but a more specific (or earlier)
    /// authored action of the same input wins.
    Outranked(u8),
    Refused(TriggerRefusal),
}

/// One authored action as the Action Lab inspects it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthoredReport {
    pub slot: u8,
    pub name: &'static str,
    pub trigger: Trigger,
    pub status: TriggerStatus,
    /// Startup, active and recovery in ticks.
    pub ticks: [u16; 3],
}

/// Why `def` would not start for an input in its context now, before the
/// ordinary arbitration.
fn own_refusal(state: &PlayerState, def: &AuthoredAction) -> Option<TriggerRefusal> {
    if state.class.map(|choice| choice.class) != Some(def.class) {
        return Some(TriggerRefusal::Class);
    }
    if !def.trigger.context.holds(state) {
        return Some(TriggerRefusal::Context(def.trigger.context));
    }
    def.conditions
        .iter()
        .copied()
        .find(|&condition| !super::holds(state, condition))
        .map(TriggerRefusal::Condition)
}

/// The authored action an input of `ability` starts now, if one does: the
/// most specific context first, then authored order. The input must resolve
/// directly under the ordinary graph.
pub(crate) fn trigger(
    graph: &ActionGraph,
    state: &PlayerState,
    input: ActionInput,
    ability: Option<AbilityId>,
) -> Option<&'static AuthoredAction> {
    if state.authored.is_some()
        || !state.is_alive()
        || aura::has_kind(&state.auras, crate::AuraKind::Stun)
    {
        return None;
    }
    let winner = graph
        .authored
        .iter()
        .filter(|def| def.matches(input, ability) && own_refusal(state, def).is_none())
        .fold(None::<&'static AuthoredAction>, |best, def| match best {
            Some(best) if best.trigger.context.rank() >= def.trigger.context.rank() => Some(best),
            _ => Some(def),
        })?;
    (super::arbitrate_with(graph, state, input, false).outcome
        == Outcome::Proceed(super::Candidate::Direct))
    .then_some(winner)
}

/// Every authored action's trigger status for this player.
pub(crate) fn reports(graph: &ActionGraph, state: &PlayerState) -> Vec<AuthoredReport> {
    graph
        .authored
        .iter()
        .map(|def| {
            let status = if let Some(run) = state.authored {
                match run.phase(graph) {
                    Some((running, phase)) if running.slot == def.slot => TriggerStatus::Running {
                        phase,
                        elapsed: run.elapsed,
                    },
                    _ => TriggerStatus::Refused(TriggerRefusal::Busy(run.slot)),
                }
            } else if let Some(refusal) = own_refusal(state, def) {
                TriggerStatus::Refused(refusal)
            } else {
                let outcome = super::arbitrate_with(graph, state, def.trigger.input, false).outcome;
                if outcome == Outcome::Proceed(super::Candidate::Direct) {
                    match trigger(graph, state, def.trigger.input, def.trigger.ability) {
                        Some(winner) if winner.slot != def.slot => {
                            TriggerStatus::Outranked(winner.slot)
                        }
                        _ => TriggerStatus::Accepted,
                    }
                } else {
                    TriggerStatus::Refused(TriggerRefusal::Arbitration(outcome))
                }
            };
            AuthoredReport {
                slot: def.slot,
                name: def.name,
                trigger: def.trigger,
                status,
                ticks: def.ticks(),
            }
        })
        .collect()
}

/// What the intent queue does with one intent, for authored actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Step {
    /// Not an authored action's business: the ordinary graph decides.
    Ordinary,
    /// The intent starts this authored action; it waits out the startup.
    Start(u8),
    /// The triggering intent waits for the startup to end.
    Startup,
    /// The startup is over: the triggering intent resolves now.
    Release,
    /// The startup is over, but the ordinary graph would not resolve the use
    /// directly (a rule holds it or an edge answers it): the action ends and
    /// the intent is handled as usual.
    Cancel,
}

/// The step for `intent` in `state`, before readiness checks.
pub(crate) fn step(graph: &ActionGraph, state: &PlayerState, intent: &PlayerIntent) -> Step {
    let Some((input, ability, _)) = intent_input(intent) else {
        return Step::Ordinary;
    };
    match state.authored {
        Some(run) if !run.released => match graph.authored_action(run.slot) {
            Some(def) if def.matches(input, Some(ability)) => {
                if run.elapsed >= def.ticks()[0] {
                    Step::Release
                } else {
                    Step::Startup
                }
            }
            _ => Step::Ordinary,
        },
        Some(_) => Step::Ordinary,
        None => trigger(graph, state, input, Some(ability))
            .map_or(Step::Ordinary, |def| Step::Start(def.slot)),
    }
}

/// Whether the running authored action holds an offense input: always,
/// except the triggering use while it resolves.
pub(crate) fn holds_input(state: &PlayerState, input: ActionInput) -> Option<u8> {
    let run = state.authored.filter(|run| !run.releasing)?;
    (super::input_region(input) == super::Region::Offense).then_some(run.slot)
}

impl ZoneSimulation {
    /// What the intent queue does with `intent` for authored actions. A use
    /// the ability's own checks would refuse starts nothing: it is refused
    /// as usual.
    pub(crate) fn authored_step(&self, player_id: PlayerId, intent: &PlayerIntent) -> Step {
        let graph = self.action_graph;
        let Some(state) = self.players.get(&player_id) else {
            return Step::Ordinary;
        };
        let Some((input, ability, directed)) = intent_input(intent) else {
            return Step::Ordinary;
        };
        match step(graph, state, intent) {
            Step::Start(slot) => {
                let waive = graph
                    .authored_action(slot)
                    .is_some_and(|def| def.effects.contains(&Effect::IgnoreGlobalCooldown));
                match self.ready_ability(player_id, ability.get(), directed, waive) {
                    Some(Ok(_)) => Step::Start(slot),
                    _ => Step::Ordinary,
                }
            }
            Step::Release
                if super::arbitrate_with(graph, state, input, false).outcome
                    != Outcome::Proceed(super::Candidate::Direct) =>
            {
                Step::Cancel
            }
            other => other,
        }
    }

    /// Starts the authored action in `slot`; `true` when it has no startup,
    /// so its use resolves at once.
    pub(crate) fn start_authored(&mut self, player_id: PlayerId, slot: u8) -> bool {
        let immediate = self
            .action_graph
            .authored_action(slot)
            .is_some_and(|def| def.ticks()[0] == 0);
        self.update_player(player_id, |player| {
            player.authored = Some(AuthoredRun {
                slot,
                ..AuthoredRun::default()
            });
        });
        immediate
    }

    /// Resolves the triggering intent through its ordinary path with the
    /// action's effects: the active phase begins, or a refused use ends the
    /// action.
    pub(crate) fn release_authored(
        &mut self,
        player_id: PlayerId,
        intent: PlayerIntent,
    ) -> Result<(), ZoneError> {
        self.update_player(player_id, |player| {
            if let Some(run) = &mut player.authored {
                run.releasing = true;
            }
        });
        let refused = self.apply_intent(player_id, intent, self.tick + 1)?;
        self.update_player(player_id, |player| {
            player.authored = player.authored.filter(|_| !refused).map(|run| AuthoredRun {
                released: true,
                releasing: false,
                ..run
            });
        });
        Ok(())
    }

    /// Ages every running authored action by one tick: a stun or death, a
    /// startup that passed without its use, or the end of its recovery ends
    /// it.
    pub(crate) fn advance_authored(&mut self) {
        let graph = self.action_graph;
        for state in self.players.values_mut() {
            let Some(mut run) = state.authored else {
                continue;
            };
            run.elapsed = run.elapsed.saturating_add(1);
            let interrupted =
                !state.is_alive() || aura::has_kind(&state.auras, crate::AuraKind::Stun);
            state.authored = (!interrupted && run.phase(graph).is_some()).then_some(run);
        }
    }
}
