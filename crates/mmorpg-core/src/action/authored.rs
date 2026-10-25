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
//! A trigger is performed through `input-bindings`' press/release lifecycle
//! as a [`Gesture`]: the press, a hold-to-charge released after a minimum,
//! or a chord pressed while a partner control is held. A trigger may also be
//! a [`TimedFollowUp`] of another authored action: it counts only in a
//! window after that action's use, and a press before the window waits in a
//! buffer that may reach back into the source's startup (an edge's buffer
//! cannot precede its anchor); a press before the buffer loses the
//! follow-up. Holds and releases arrive as ordinary commands and are read in
//! arrival ticks: nothing is backdated, and a late input's grace (#415)
//! applies only to the follow-up window, as to an edge's. Like a running
//! action, the holds are zone setup, not canonical state.
//!
//! A stun or death ends a running authored action. Nothing here is a second
//! executor: the zone only times the phases, and the follow-up runs through
//! the ordinary intent, edge and movement code. The production graph authors
//! none, and like the graph itself a running authored action is zone setup,
//! not canonical state: a zone restored from a snapshot runs the baseline.

use super::{
    ActionGraph, ActionId, ActionInput, Anchor, BlockReason, Condition, EdgeDef, EdgeId, Effect,
    GraphError, MAX_IMPULSE_FORWARD_CMS, MAX_IMPULSE_UP_CMS, Movement, Outcome, Phase, Window,
    duration_ok, input_of, validate_movement_values,
};
use crate::snapshot::{PlayerIntent, QueuedIntent};
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

/// The held control a chord needs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Partner {
    /// The raised shield guard (Wardens).
    Guard,
    /// Another use of this input, pressed and not released yet.
    Input(ActionInput),
}

/// How the trigger's input is performed, through `input-bindings`' press
/// and release lifecycle (Atlas 5.9.5).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Gesture {
    /// The press.
    #[default]
    Press,
    /// Hold to charge, release to fire. The startup lasts while the input is
    /// held, at most its own length (a full charge fires by itself). A
    /// release at least `min_ms` after the press fires the use with the
    /// action's effects; an earlier one is an ordinary tap: the action ends
    /// and the use resolves as usual. A retired hold (focus loss, a menu)
    /// ends the action without a use.
    Charge { min_ms: u32 },
    /// The press while the partner is held.
    Chord(Partner),
}

impl Gesture {
    const fn rank(self) -> u8 {
        match self {
            Self::Press | Self::Charge { .. } => 0,
            Self::Chord(_) => 1,
        }
    }
}

/// A timed follow-up (Atlas 5.9.5): the trigger counts only in `window`,
/// measured from the use of the authored action in `source`. Its buffer may
/// exceed the window's opening and reach back into the source's startup.
/// Following it ends the source, as taking an edge does.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TimedFollowUp {
    pub source: u8,
    pub window: Window,
}

/// The semantic input that starts an authored action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Trigger {
    /// Warden melee, Shield Bash or another class ability.
    pub input: ActionInput,
    /// Only this ability's use; `None` is any use the input covers.
    pub ability: Option<AbilityId>,
    pub context: InputContext,
    pub gesture: Gesture,
    /// Only as this timed follow-up of another authored action.
    pub follows: Option<TimedFollowUp>,
}

impl Trigger {
    /// The plain press of `input` in `context`.
    #[must_use]
    pub const fn press(
        input: ActionInput,
        ability: Option<AbilityId>,
        context: InputContext,
    ) -> Self {
        Self {
            input,
            ability,
            context,
            gesture: Gesture::Press,
            follows: None,
        }
    }

    /// Whether two triggers claim the same input: the same input, ability,
    /// context and follow-up source, and the same chord partner or neither a
    /// chord (a charge and a press of one input conflict).
    fn conflicts(&self, other: &Self) -> bool {
        let gesture = match (self.gesture, other.gesture) {
            (Gesture::Chord(a), Gesture::Chord(b)) => a == b,
            (Gesture::Chord(_), _) | (_, Gesture::Chord(_)) => false,
            _ => true,
        };
        gesture
            && self.input == other.input
            && self.ability == other.ability
            && self.context == other.context
            && self.follows.map(|follow| follow.source) == other.follows.map(|follow| follow.source)
    }

    /// Chords first, then the more specific context.
    const fn rank(&self) -> (u8, u8) {
        (self.gesture.rank(), self.context.rank())
    }
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

    /// The phase `elapsed` ticks after the trigger, once the use resolved
    /// `impact` ticks after it (or before it has); `None` once it is over.
    /// A charge's use resolves at its release, before the startup's end.
    #[must_use]
    pub const fn phase_at(&self, elapsed: u16, impact: Option<u16>) -> Option<Phase> {
        let [startup, active, recovery] = self.ticks();
        let Some(impact) = impact else {
            return if elapsed <= startup {
                Some(Phase::Startup)
            } else {
                None
            };
        };
        let since = elapsed.saturating_sub(impact);
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
    /// A charge's minimum is zero or not a duration within its startup. (A
    /// directed strike is sent on its release, so it is always a tap.)
    Charge,
    /// A chord's partner cannot be held: only the guard (for Wardens) and a
    /// pressed Warden melee, Shield Bash or class ability are.
    Chord,
    /// A follow-up's source is not an earlier authored action of the same
    /// class, or its window is malformed, buffers back past the source's
    /// trigger or closes after the source ends.
    FollowUp,
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
        match def.trigger.gesture {
            Gesture::Press => {}
            Gesture::Charge { min_ms } => {
                if min_ms == 0 || !duration_ok(min_ms) || min_ms > def.startup.ms {
                    return bad(AuthoredProblem::Charge);
                }
            }
            Gesture::Chord(partner) => {
                let holdable = match partner {
                    Partner::Guard => def.class == PlayerClass::Warden,
                    Partner::Input(ActionInput::WardenMelee | ActionInput::ShieldBash) => {
                        def.class == PlayerClass::Warden
                    }
                    Partner::Input(input) => input == ActionInput::Ability,
                };
                if !holdable {
                    return bad(AuthoredProblem::Chord);
                }
            }
        }
        if let Some(follow) = def.trigger.follows {
            let window = follow.window;
            let source = self.authored[..index]
                .iter()
                .find(|other| other.slot == follow.source && other.class == def.class);
            let fits = source.is_some_and(|source| {
                [window.open_after_ms, window.actionable_ms, window.buffer_ms]
                    .into_iter()
                    .all(duration_ok)
                    && u64::from(window.buffer_ms)
                        <= u64::from(window.open_after_ms) + u64::from(source.startup.ms)
                    && window.ticks().closes() < source.after_impact_ticks()
            });
            if !fits {
                return bad(AuthoredProblem::FollowUp);
            }
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
            .find(|other| other.class == def.class && other.trigger.conflicts(&def.trigger))
        {
            return Err(GraphError::ConflictingTrigger(other.slot, def.slot));
        }
        Ok(())
    }

    /// Whether a trigger reads holds and releases: a charge or a chord. The
    /// zone tracks them only then.
    #[must_use]
    pub fn tracks_holds(&self) -> bool {
        self.authored
            .iter()
            .any(|def| def.trigger.gesture != Gesture::Press)
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

/// The most holds one player's tracker keeps.
const MAX_HOLDS: usize = 8;

/// How a hold ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Ended {
    /// The key went up in this tick.
    At(u64),
    /// `input-bindings` retired the hold; it must never act.
    Retired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Hold {
    ability: AbilityId,
    /// The tick the press arrived in.
    pressed: u64,
    ended: Option<Ended>,
}

/// Pressed abilities and their releases, in arrival ticks (#414).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Holds(Vec<Hold>);

impl Holds {
    /// `ability` was pressed in tick `now`; the new hold replaces its last.
    pub(crate) fn press(&mut self, ability: AbilityId, now: u64) {
        self.0.retain(|hold| hold.ability != ability);
        if self.0.len() >= MAX_HOLDS {
            let oldest = self
                .0
                .iter()
                .position(|hold| hold.ended.is_some())
                .unwrap_or(0);
            self.0.remove(oldest);
        }
        self.0.push(Hold {
            ability,
            pressed: now,
            ended: None,
        });
    }

    /// `ability`'s hold ended in tick `now`, or was retired.
    pub(crate) fn release(&mut self, ability: AbilityId, now: u64, retired: bool) {
        if let Some(hold) = self
            .0
            .iter_mut()
            .find(|hold| hold.ability == ability && hold.ended.is_none())
        {
            hold.ended = Some(if retired {
                Ended::Retired
            } else {
                Ended::At(now)
            });
        }
    }

    /// Whether a use of `input`, other than `except`, is held.
    fn held(&self, input: ActionInput, except: Option<AbilityId>) -> bool {
        self.0.iter().any(|hold| {
            hold.ended.is_none()
                && Some(hold.ability) != except
                && input_of(&PlayerIntent::UseAbility {
                    ability: hold.ability.get(),
                    target: None,
                }) == Some(input)
        })
    }

    fn of(&self, ability: AbilityId) -> Option<Hold> {
        self.0.iter().find(|hold| hold.ability == ability).copied()
    }
}

/// Where a hold-to-charge trigger stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Charge {
    Holding,
    /// Released after its minimum.
    Charged,
    /// Released before its minimum, or never held.
    Tapped,
    Retired,
}

/// The charge of `def`'s trigger pressed by `ability`; `None` for another
/// gesture.
fn charge(
    state: &PlayerState,
    def: &AuthoredAction,
    ability: AbilityId,
    directed: bool,
) -> Option<Charge> {
    let Gesture::Charge { min_ms } = def.trigger.gesture else {
        return None;
    };
    // A directed strike is sent on its release: it is never held.
    let hold = state.holds.of(ability).filter(|_| !directed);
    Some(match hold.map(|hold| (hold.pressed, hold.ended)) {
        None => Charge::Tapped,
        Some((_, None)) => Charge::Holding,
        Some((_, Some(Ended::Retired))) => Charge::Retired,
        Some((pressed, Some(Ended::At(at)))) => {
            if at.saturating_sub(pressed) >= u64::from(crate::ticks_from_ms(min_ms)) {
                Charge::Charged
            } else {
                Charge::Tapped
            }
        }
    })
}

/// A running authored action. Zone setup like the graph, so not canonical.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AuthoredRun {
    pub(crate) slot: u8,
    /// Ticks since the trigger.
    pub(crate) elapsed: u16,
    /// Ticks from the trigger to its use, once it resolved: the active phase
    /// began then.
    pub(crate) impact: Option<u16>,
    /// The triggering use is resolving now.
    pub(crate) releasing: bool,
    /// The triggering ability, and whether a directed strike pressed it: a
    /// charge reads its hold.
    pub(crate) ability: u8,
    pub(crate) directed: bool,
    /// A follow-up's input came before its buffer: this run lost it.
    pub(crate) missed: bool,
}

impl AuthoredRun {
    /// The triggering use has resolved: the active phase has begun.
    pub(crate) const fn released(self) -> bool {
        self.impact.is_some()
    }

    pub(crate) fn phase(self, graph: &ActionGraph) -> Option<(&'static AuthoredAction, Phase)> {
        let def = graph.authored_action(self.slot)?;
        Some((def, def.phase_at(self.elapsed, self.impact)?))
    }

    /// Ticks since the use resolved, once it has.
    pub(crate) fn since_impact(self) -> Option<u16> {
        self.impact
            .map(|impact| self.elapsed.saturating_sub(impact))
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
    /// A chord's partner is not held.
    Partner(Partner),
    /// A follow-up's source is not running, or its window is not open yet
    /// (before the buffer) or any more.
    FollowUp(BlockReason),
    /// A press before the buffer lost the follow-up for this run of its
    /// source.
    Missed,
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
    /// A follow-up's trigger, arriving now, waits in its buffer for the
    /// window to open.
    Buffering,
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

/// Why `def` would not start for an input of `ability` in its context now,
/// before the ordinary arbitration.
fn own_refusal(
    state: &PlayerState,
    def: &AuthoredAction,
    ability: Option<AbilityId>,
) -> Option<TriggerRefusal> {
    if state.class.map(|choice| choice.class) != Some(def.class) {
        return Some(TriggerRefusal::Class);
    }
    if !def.trigger.context.holds(state) {
        return Some(TriggerRefusal::Context(def.trigger.context));
    }
    if let Some(condition) = def
        .conditions
        .iter()
        .copied()
        .find(|&condition| !super::holds(state, condition))
    {
        return Some(TriggerRefusal::Condition(condition));
    }
    let Gesture::Chord(partner) = def.trigger.gesture else {
        return None;
    };
    let held = match partner {
        Partner::Guard => state.guard.raised.is_some(),
        Partner::Input(input) => state.holds.held(input, ability),
    };
    (!held).then_some(TriggerRefusal::Partner(partner))
}

/// The best of `defs`: chords first, then the most specific context, then
/// authored order.
fn best<'a>(defs: impl Iterator<Item = &'a AuthoredAction>) -> Option<&'a AuthoredAction> {
    defs.fold(None::<&AuthoredAction>, |best, def| match best {
        Some(best) if best.trigger.rank() >= def.trigger.rank() => Some(best),
        _ => Some(def),
    })
}

/// The authored action an input of `ability` starts now, if one does. A
/// follow-up starts only from its source. The input must resolve directly
/// under the ordinary graph.
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
    let winner = best(graph.authored.iter().filter(|def| {
        def.trigger.follows.is_none()
            && def.matches(input, ability)
            && own_refusal(state, def, ability).is_none()
    }))?;
    (super::arbitrate_with(graph, state, input, false, 0).outcome
        == Outcome::Proceed(super::Candidate::Direct))
    .then_some(winner)
}

/// The follow-up of `run` an input of `ability` makes, if one does; only
/// those whose context, conditions and partner hold when `ready`.
fn follower(
    graph: &ActionGraph,
    state: &PlayerState,
    run: AuthoredRun,
    input: ActionInput,
    ability: Option<AbilityId>,
    ready: bool,
) -> Option<&'static AuthoredAction> {
    best(graph.authored.iter().filter(|def| {
        def.trigger
            .follows
            .is_some_and(|follow| follow.source == run.slot)
            && def.matches(input, ability)
            && (!ready || own_refusal(state, def, ability).is_none())
    }))
}

/// The window of a follow-up of `run`, for an input `late` ticks late: like
/// an edge's from the source's use, with a buffer that may reach into the
/// source's startup, and a late input's grace (#415).
fn follow_status(
    graph: &ActionGraph,
    run: AuthoredRun,
    follow: TimedFollowUp,
    late: u8,
) -> super::EdgeStatus {
    use super::EdgeStatus;
    let Some(source) = graph.authored_action(run.slot) else {
        return EdgeStatus::Blocked(BlockReason::SourceInactive);
    };
    // Before the use, count toward the startup's end.
    let since = i32::from(run.elapsed) - i32::from(run.impact.unwrap_or(source.ticks()[0]));
    let window = follow.window.ticks();
    let opens = i32::from(window.opens);
    let late = i32::from(late.min(super::LATE_INPUT_GRACE_TICKS));
    if since < opens - i32::from(window.buffer) {
        EdgeStatus::Blocked(BlockReason::TooEarly)
    } else if since < opens || !run.released() {
        EdgeStatus::Buffering
    } else if since - late <= i32::from(window.closes()) {
        EdgeStatus::Open
    } else {
        EdgeStatus::Blocked(BlockReason::Expired)
    }
}

/// Every authored action's trigger status for this player.
pub(crate) fn reports(graph: &ActionGraph, state: &PlayerState) -> Vec<AuthoredReport> {
    graph
        .authored
        .iter()
        .map(|def| {
            let status = match (state.authored, def.trigger.follows) {
                (Some(run), _) if run.slot == def.slot => match run.phase(graph) {
                    Some((_, phase)) => TriggerStatus::Running {
                        phase,
                        elapsed: run.elapsed,
                    },
                    None => TriggerStatus::Refused(TriggerRefusal::Busy(run.slot)),
                },
                (Some(run), Some(follow)) if follow.source == run.slot => {
                    follow_report(graph, state, run, def, follow)
                }
                (Some(run), _) => TriggerStatus::Refused(TriggerRefusal::Busy(run.slot)),
                (None, Some(_)) => {
                    TriggerStatus::Refused(TriggerRefusal::FollowUp(BlockReason::SourceInactive))
                }
                (None, None) => start_report(graph, state, def),
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

fn start_report(graph: &ActionGraph, state: &PlayerState, def: &AuthoredAction) -> TriggerStatus {
    if let Some(refusal) = own_refusal(state, def, def.trigger.ability) {
        return TriggerStatus::Refused(refusal);
    }
    let outcome = super::arbitrate_with(graph, state, def.trigger.input, false, 0).outcome;
    if outcome != Outcome::Proceed(super::Candidate::Direct) {
        return TriggerStatus::Refused(TriggerRefusal::Arbitration(outcome));
    }
    match trigger(graph, state, def.trigger.input, def.trigger.ability) {
        Some(winner) if winner.slot != def.slot => TriggerStatus::Outranked(winner.slot),
        _ => TriggerStatus::Accepted,
    }
}

fn follow_report(
    graph: &ActionGraph,
    state: &PlayerState,
    run: AuthoredRun,
    def: &AuthoredAction,
    follow: TimedFollowUp,
) -> TriggerStatus {
    use super::EdgeStatus;
    if run.missed {
        return TriggerStatus::Refused(TriggerRefusal::Missed);
    }
    match follow_status(graph, run, follow, 0) {
        EdgeStatus::Buffering => TriggerStatus::Buffering,
        EdgeStatus::Open => match own_refusal(state, def, def.trigger.ability) {
            Some(refusal) => TriggerStatus::Refused(refusal),
            None => match follower(
                graph,
                state,
                run,
                def.trigger.input,
                def.trigger.ability,
                true,
            ) {
                Some(winner) if winner.slot != def.slot => TriggerStatus::Outranked(winner.slot),
                _ => TriggerStatus::Accepted,
            },
        },
        EdgeStatus::Blocked(reason) => TriggerStatus::Refused(TriggerRefusal::FollowUp(reason)),
        EdgeStatus::Suspended(_) => {
            TriggerStatus::Refused(TriggerRefusal::FollowUp(BlockReason::SourceInactive))
        }
    }
}

/// What the intent queue does with one intent, for authored actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Step {
    /// Not an authored action's business: the ordinary graph decides.
    Ordinary,
    /// The intent starts this authored action; it waits out the startup.
    Start(u8),
    /// The triggering intent waits for the startup to end (or a charge's
    /// release).
    Startup,
    /// The startup is over, or a charge was released after its minimum: the
    /// triggering intent resolves now.
    Release,
    /// The startup is over, but the ordinary graph would not resolve the use
    /// directly (a rule holds it or an edge answers it), or a charge was
    /// released before its minimum (a tap): the action ends and the intent
    /// is handled as usual.
    Cancel,
    /// A charge's hold was retired: the action ends and the intent is
    /// dropped without a use.
    Drop,
    /// A follow-up's window is open: the intent ends the running action and
    /// starts this one.
    Chain(u8),
    /// A follow-up's input waits in its buffer for the window.
    Buffer,
    /// A follow-up's input came before its buffer, or cannot start it when
    /// the window is open: the run loses the follow-up and the intent is
    /// handled as usual.
    Miss,
}

impl Step {
    /// Whether the step handled the running action's own trigger, so later
    /// intents of the same input are not it.
    pub(crate) const fn is_trigger(self) -> bool {
        matches!(
            self,
            Self::Start(_)
                | Self::Startup
                | Self::Release
                | Self::Cancel
                | Self::Drop
                | Self::Chain(_)
        )
    }
}

/// The step for `queued` in `state`, before readiness checks. The running
/// action's trigger is the first intent of its input in the queue, unless
/// `trigger_seen`.
pub(crate) fn step(
    graph: &ActionGraph,
    state: &PlayerState,
    queued: &QueuedIntent,
    trigger_seen: bool,
) -> Step {
    let Some((input, ability, directed)) = intent_input(&queued.intent) else {
        return Step::Ordinary;
    };
    let Some(run) = state.authored else {
        let Some(def) = trigger(graph, state, input, Some(ability)) else {
            return Step::Ordinary;
        };
        return match charge(state, def, ability, directed) {
            Some(Charge::Tapped) => Step::Ordinary,
            Some(Charge::Retired) => Step::Drop,
            _ => Step::Start(def.slot),
        };
    };
    if let Some(def) = graph.authored_action(run.slot)
        && !run.released()
        && !trigger_seen
        && def.matches(input, Some(ability))
    {
        return match charge(state, def, AbilityId::new(run.ability), run.directed) {
            Some(Charge::Retired) => Step::Drop,
            Some(Charge::Tapped) => Step::Cancel,
            Some(Charge::Charged) => Step::Release,
            _ if run.elapsed >= def.ticks()[0] => Step::Release,
            _ => Step::Startup,
        };
    }
    if run.missed {
        return Step::Ordinary;
    }
    let Some(follow) = follower(graph, state, run, input, Some(ability), false)
        .and_then(|def| def.trigger.follows)
    else {
        return Step::Ordinary;
    };
    match follow_status(graph, run, follow, queued.late) {
        super::EdgeStatus::Blocked(BlockReason::TooEarly) => Step::Miss,
        super::EdgeStatus::Buffering => Step::Buffer,
        super::EdgeStatus::Open => follower(graph, state, run, input, Some(ability), true)
            .map_or(Step::Miss, |def| Step::Chain(def.slot)),
        _ => Step::Ordinary,
    }
}

/// Whether the running authored action holds an offense input: always,
/// except the triggering use while it resolves.
pub(crate) fn holds_input(state: &PlayerState, input: ActionInput) -> Option<u8> {
    let run = state.authored.filter(|run| !run.releasing)?;
    (super::input_region(input) == super::Region::Offense).then_some(run.slot)
}

impl ZoneSimulation {
    /// What the intent queue does with `queued` for authored actions. A use
    /// the ability's own checks would refuse starts nothing: it is refused
    /// as usual (and a follow-up is lost).
    pub(crate) fn authored_step(
        &self,
        player_id: PlayerId,
        queued: &QueuedIntent,
        trigger_seen: bool,
    ) -> Step {
        let graph = self.action_graph;
        let Some(state) = self.players.get(&player_id) else {
            return Step::Ordinary;
        };
        let Some((input, ability, directed)) = intent_input(&queued.intent) else {
            return Step::Ordinary;
        };
        match step(graph, state, queued, trigger_seen) {
            step @ (Step::Start(slot) | Step::Chain(slot)) => {
                let waive = graph
                    .authored_action(slot)
                    .is_some_and(|def| def.effects.contains(&Effect::IgnoreGlobalCooldown));
                match self.ready_ability(player_id, ability.get(), directed, waive) {
                    Some(Ok(_)) => step,
                    _ if step == Step::Start(slot) => Step::Ordinary,
                    _ => Step::Miss,
                }
            }
            Step::Release
                if super::arbitrate_with(graph, state, input, false, 0).outcome
                    != Outcome::Proceed(super::Candidate::Direct) =>
            {
                Step::Cancel
            }
            other => other,
        }
    }

    /// Starts the authored action in `slot` from `intent`, ending any running
    /// one (a follow-up); `true` when it has no startup, so its use resolves
    /// at once.
    pub(crate) fn start_authored(
        &mut self,
        player_id: PlayerId,
        slot: u8,
        intent: &PlayerIntent,
    ) -> bool {
        let immediate = self
            .action_graph
            .authored_action(slot)
            .is_some_and(|def| def.ticks()[0] == 0);
        let (ability, directed) = intent_input(intent)
            .map_or((0, false), |(_, ability, directed)| {
                (ability.get(), directed)
            });
        self.update_player(player_id, |player| {
            player.authored = Some(AuthoredRun {
                slot,
                ability,
                directed,
                ..AuthoredRun::default()
            });
        });
        immediate
    }

    /// The run lost its follow-up (#414).
    pub(crate) fn miss_follow_up(&mut self, player_id: PlayerId) {
        self.update_player(player_id, |player| {
            if let Some(run) = &mut player.authored {
                run.missed = true;
            }
        });
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
                impact: Some(run.elapsed),
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
