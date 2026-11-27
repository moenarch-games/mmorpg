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
//!   protection. A cancelling rule instead lets the input end a running
//!   action: moving or jumping ends a cast.
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
//! (5.2.37–5.2.51, 5.2.69–5.2.70, 5.2.i910) expressed as data, with unchanged values:
//! it has no precision window, no window modifier and no effect beyond the
//! combo's confirmed target and global-cooldown waiver. A zone runs another
//! validated [`ActionGraph`] only through [`ZoneSimulation::set_action_graph`],
//! which tests and the lab use; it is zone setup, not canonical state, and a
//! zone restored from a snapshot runs the baseline.
//!
//! - **Overrides** (Atlas 5.7.6). The graph's definitions are shared by
//!   every class. An [`Override`] replaces one value (a recovery, an edge
//!   window, or whether a rule applies) for one class, main-hand weapon or
//!   ability. The most specific override wins (ability over weapon over
//!   class over the baseline), two overrides of one value in one scope fail
//!   validation, and every effective value reports its [`Origin`]. The
//!   production graph has none, so every value is the baseline's. An
//!   override can also replace one action phase's authored movement.
//! - **Traversal** (#413, Atlas 5.9.1–5.9.3). The movement region runs for
//!   every class: airborne, then a landing recovery. [`MovementRule`]s give
//!   an action phase bounded [`Movement`] (steering, kept momentum, an
//!   acceleration limit) that the zone turns into the controller's desired
//!   velocity, so physics still resolves every contact; running phases
//!   combine to the most restrictive values. Edges out of a landing
//!   ([`EdgeId::LandingToMelee`] and its siblings, anchored at touchdown)
//!   win over the landing's hold, end its recovery and may carry an
//!   [`Effect::Impulse`]: run → jump → aerial attack → landing → follow-up
//!   composes from ordinary actions. The baseline authors first values for the
//!   basic strikes' wind-up, running recovery, air and landing (#1247); its
//!   one landing edge is the sword master's [`EdgeId::LeapingSlash`] (#915).
//! - **Swimming** (#413, Atlas 5.1.41–5.1.43). [`ActionId::Swim`] runs in
//!   the movement region while a living player swims (at the surface or
//!   under it), ahead of being airborne. Its phase can author movement like
//!   any other, and compatibility rules can [`RuleEffect::Refuse`] an attack
//!   in water or [`RuleEffect::Hold`] it until the swimmer is ashore, so
//!   water never implicitly permits every attack. The baseline authors no
//!   such rule: attacks while swimming stay allowed, and combat with
//!   submerged units stays blocked by the zone (5.1.43). Climbing and
//!   mantling join the movement region once they exist as authoritative
//!   actions.
//!
//! - **Authored actions** (#414, Atlas 5.9.4–5.9.5). An experimental graph
//!   may compose new named actions ([`AuthoredAction`], see [`authored`]):
//!   a contextual trigger (plain, direction + action, jump + attack), timed
//!   startup, active and recovery phases with bounded movement, typed
//!   effects on the ordinary use the active phase resolves, and outgoing
//!   [`EdgeId::Authored`] edges. The baseline authors none.
//!
//! Every playable class runs on the same model. Wardens add the directed
//! slash, the guard and the landing hold. Casts and channels run as
//! [`ActionId::Cast`]: moving or jumping cancels one through
//! [`RuleEffect::Cancel`] rules, and a stun or death cancels it through
//! interruptions. Every class ability starts [`ActionId::GlobalCooldown`].
//!
//! Runtime state stays in the existing canonical fields: `landing` for the
//! movement region, `cast`, a timed `swing` (#679), `strike_recovery`, `combo`, the basic
//! strike's `melee` state and swing timer, and `global_cooldown` for the offense region, `guard` for the defense region, and auras, the
//! guard break and health for the reaction region.
//! [`ZoneSimulation::action_inspection`] is the read-only query surface that
//! the Action Lab (#381) reads.
//!
//! Layout: this file holds the definitions (regions, actions, phases, inputs, windows,
//! conditions, effects, rules and overrides); `baseline.rs` the production graph;
//! `validation.rs` the checks a graph passes; `runtime.rs` what runs now, arbitration,
//! follow-ups and authored movement; `inspection.rs` the read-only reports; [`authored`]
//! experimental actions.

pub mod authored;

pub use authored::{
    AuthoredAction, AuthoredPhase, AuthoredProblem, AuthoredReport, Direction, Gesture,
    InputContext, MAX_AUTHORED_ACTIONS, MAX_AUTHORED_EDGES, MAX_AUTHORED_MS, MAX_AUTHORED_NAME,
    Partner, TimedFollowUp, Trigger, TriggerRefusal, TriggerStatus,
};

use crate::snapshot::{PlayerIntent, QueuedIntent};
use crate::zone::PlayerState;
use crate::{
    AbilityId, AbilityUser, AuraKind, EntityRef, EquipmentSlot, ItemId, PlayerClass, PlayerId,
    ZoneSimulation, ability::ids, aura,
};

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
    /// Swimming in deep water, at the surface or under it (#274).
    Swim,
    /// Holding a climbable face (#490).
    Climb,
    /// Pulling up onto a ledge (#490).
    Mantle,
    /// A dodge roll in progress (#516).
    Roll,
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
    /// A cast or channel in progress, of any class.
    Cast,
    /// The global cooldown a class ability started, of any class.
    GlobalCooldown,
    /// The basic strike every class presses (#885, #1221): a standing chain, running or
    /// jumping strike, from its wind-up through its recovery and chain window.
    BasicMelee,
    /// The experimental graph's authored action in this slot (#414).
    Authored(u8),
}

/// The phase of a running action.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Phase {
    /// An authored action's startup: its use has not resolved yet (#414).
    Startup,
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
    /// Any other class ability use: a shot, a spell or a heal.
    Ability,
    /// A movement intent: walking, strafing, or swimming up or down.
    Move,
    /// A jump.
    Jump,
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
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
    /// A melee action was made during the current airborne phase, or the
    /// one the current landing recovery ends.
    AttackedAirborne,
    /// Not grounded after the last physics step.
    Airborne,
    /// A stun aura is active.
    Stunned,
    /// The source strike's confirmed result damaged two or more units: a
    /// cleaving timed swing (#910).
    MultiHit,
    /// The acting player wields a sword and has reached the sword mastery that unlocks
    /// the Leaping Slash (#915).
    SwordMastery,
    /// The basic strike winding up or recovering is a standing chain strike (#1247).
    StandingStrike,
    /// The basic strike winding up or recovering is a running strike (#1247).
    RunningStrike,
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
    /// The follow-up launches the player (Atlas 5.9.2): `forward_cms` cm/s
    /// along its facing (negative: backward) is added to its horizontal
    /// velocity, and its vertical velocity becomes at least `up_cms` cm/s,
    /// for the physics step of the tick it resolves in. Physics still resolves
    /// every contact. At most [`MAX_IMPULSE_FORWARD_CMS`] and
    /// [`MAX_IMPULSE_UP_CMS`].
    Impulse { forward_cms: i16, up_cms: u16 },
}

/// The largest forward or backward [`Effect::Impulse`]: twice the run speed.
pub const MAX_IMPULSE_FORWARD_CMS: u16 = 2 * crate::RUN_SPEED_UNITS_PER_SECOND as u16;
/// The largest upward [`Effect::Impulse`]: the jump's launch speed.
pub const MAX_IMPULSE_UP_CMS: u16 = crate::JUMP_VELOCITY_UNITS_PER_SECOND as u16;

/// Stable edge identities.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EdgeId {
    /// The first Warden combo, directed slash → Shield Bash (#157).
    SlashToShieldBash,
    /// A directed slash that damaged two or more units → another Warden melee
    /// use (#910).
    CleaveToSlash,
    /// A sword master's jumping strike lands into an immediate Warden melee use: the
    /// Leaping Slash (#915), out of the landing recovery.
    LeapingSlash,
    /// Out of a landing recovery into Warden melee (#413). Experimental
    /// graphs author the landing edges; the baseline has none.
    LandingToMelee,
    /// Out of a landing recovery into Shield Bash.
    LandingToShieldBash,
    /// Out of a landing recovery into any other class ability.
    LandingToAbility,
    /// Out of an authored action (#414), numbered below
    /// [`MAX_AUTHORED_EDGES`].
    Authored(u8),
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

/// Bounded movement behavior of an action phase (Atlas 5.9.2). The zone
/// hands the result to the physics character controller as the desired
/// horizontal velocity, so collision, support and clearance stay
/// authoritative; nothing here moves a body by itself.
///
/// Each tick the horizontal velocity is
/// `intent × steering + last × momentum × (1 − steering)`, where `intent` is
/// the steered velocity the movement intent asks for (after snares, guard
/// and swimming) and `last` is the velocity contact let the body realize in
/// the previous step. With an acceleration limit, the change from `last` is
/// then shortened to it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Movement {
    /// Percent of the movement intent the player steers with: 100 is full
    /// control, 0 none.
    pub steering_percent: u8,
    /// Percent of the last realized velocity kept for the part steering
    /// does not take: 100 keeps the momentum, 0 stops.
    pub momentum_percent: u8,
    /// The largest change of horizontal velocity, in cm/s²; `None` is
    /// unlimited.
    pub max_acceleration_cms2: Option<u32>,
}

impl Movement {
    /// Today's movement: full steering, no momentum, no limit.
    pub const FREE: Self = Self {
        steering_percent: 100,
        momentum_percent: 0,
        max_acceleration_cms2: None,
    };

    /// The most restrictive combination of two phases running at once: the
    /// lower steering, the higher momentum and the lower acceleration limit.
    #[must_use]
    pub fn combine(self, other: Self) -> Self {
        Self {
            steering_percent: self.steering_percent.min(other.steering_percent),
            momentum_percent: self.momentum_percent.max(other.momentum_percent),
            max_acceleration_cms2: match (self.max_acceleration_cms2, other.max_acceleration_cms2) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        }
    }

    /// The horizontal velocity, in subunits per tick, for a steered `intent`
    /// and the `last` realized velocity.
    #[must_use]
    pub fn velocity(self, intent: [i32; 2], last: [i32; 2]) -> [i32; 2] {
        let steering = i64::from(self.steering_percent);
        let kept = i64::from(self.momentum_percent) * (100 - steering);
        let mix = |intent: i32, last: i32| {
            (i64::from(intent) * steering * 100 + i64::from(last) * kept) / 10_000
        };
        let mut velocity = [mix(intent[0], last[0]), mix(intent[1], last[1])];
        if let Some(limit) = self.max_acceleration_cms2 {
            let limit = acceleration_subunits(limit);
            let delta = [
                velocity[0] - i64::from(last[0]),
                velocity[1] - i64::from(last[1]),
            ];
            let length = (delta[0] * delta[0] + delta[1] * delta[1]).isqrt();
            if length > limit {
                velocity = [
                    i64::from(last[0]) + delta[0] * limit / length,
                    i64::from(last[1]) + delta[1] * limit / length,
                ];
            }
        }
        velocity
            .map(|axis| i32::try_from(axis).unwrap_or(if axis < 0 { i32::MIN } else { i32::MAX }))
    }
}

/// Subunits per tick per tick of an acceleration in cm/s², rounded, at least 1.
fn acceleration_subunits(cms2: u32) -> i64 {
    let ticks_squared = i64::from(crate::TICK_HZ) * i64::from(crate::TICK_HZ);
    ((i64::from(cms2) * i64::from(crate::physics::SUBUNITS_PER_UNIT) + ticks_squared / 2)
        / ticks_squared)
        .max(1)
}

/// Subunits per tick of a speed in cm/s, rounded.
pub(crate) fn speed_subunits(cms: i32) -> i32 {
    let hz = i32::from(crate::TICK_HZ);
    let subunits = cms * crate::physics::SUBUNITS_PER_UNIT;
    (subunits + subunits.signum() * hz / 2) / hz
}

/// The largest authored acceleration limit: 1 km/s², far beyond any stop.
pub const MAX_ACCELERATION_CMS2: u32 = 100_000;

/// The movement of one action phase, under conditions (Atlas 5.9.2).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MovementRule {
    pub action: ActionId,
    pub phase: Phase,
    pub conditions: &'static [Condition],
    pub movement: Movement,
}

/// Stable compatibility rule identities.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuleId {
    /// Warden melee waits out landing recovery (5.2.70).
    LandingHoldsMelee,
    /// A guard raised during slash recovery protects only afterwards
    /// (5.2.39, 5.2.41).
    SlashRecoveryHoldsGuard,
    /// Moving or jumping ends a cast or channel.
    MovementCancelsCast,
    /// An attack in water is refused (#413). Experimental graphs author it;
    /// the baseline allows attacks while swimming.
    SwimmingRefuses,
    /// An attack waits until the climber is off the face (#490).
    /// Experimental graphs author it; the baseline allows attacks while
    /// climbing.
    ClimbingHolds,
    /// An attack waits until the dodge roll ends (#516). Experimental
    /// graphs author it; the baseline allows attacks while rolling.
    RollingHolds,
    /// An attack in water waits until the swimmer is ashore (#413).
    SwimmingHolds,
}

/// What a compatibility rule does with its input.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuleEffect {
    /// While `during` recovers, the input waits (is held) until the
    /// recovery ends. During a swim, climb, mantle or roll, it waits until
    /// that ends.
    Hold,
    /// While `during` runs, the input ends it. Only a cast supports this,
    /// for movement and jump inputs.
    Cancel,
    /// While `during` runs, the input is refused with
    /// [`crate::ErrorCode::NotWhileSwimming`]. Only a swim supports this, for
    /// offense inputs.
    Refuse,
}

/// A region-compatibility rule (Atlas 5.7.5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompatibilityRule {
    pub id: RuleId,
    /// The action the rule covers: its recovery phase for a hold (a swim's
    /// whole run), its whole run for a cancellation or refusal.
    pub during: ActionId,
    pub input: ActionInput,
    pub conditions: &'static [Condition],
    pub effect: RuleEffect,
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

/// Who an [`Override`] is for.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Scope {
    /// Every player of the class.
    Class(PlayerClass),
    /// Every player wielding this main-hand catalog item.
    Weapon(ItemId),
    /// The action of this class ability: its cast, its directed slash or the
    /// global cooldown it starts.
    Ability(AbilityId),
}

impl Scope {
    /// More specific scopes win: ability over weapon over class.
    const fn rank(self) -> u8 {
        match self {
            Self::Class(_) => 1,
            Self::Weapon(_) => 2,
            Self::Ability(_) => 3,
        }
    }
}

/// A value an [`Override`] can replace.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Key {
    /// An action's committed recovery: the directed slash's or the global
    /// cooldown's.
    Recovery(ActionId),
    /// An edge's window.
    Window(EdgeId),
    /// Whether a compatibility rule applies.
    Rule(RuleId),
    /// An action phase's authored movement.
    Movement(ActionId, Phase),
}

/// A value of a [`Key`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Setting {
    Recovery {
        action: ActionId,
        ms: u32,
    },
    Window {
        edge: EdgeId,
        window: Window,
    },
    Rule {
        rule: RuleId,
        applies: bool,
    },
    Movement {
        action: ActionId,
        phase: Phase,
        movement: Movement,
    },
}

impl Setting {
    #[must_use]
    pub const fn key(self) -> Key {
        match self {
            Self::Recovery { action, .. } => Key::Recovery(action),
            Self::Window { edge, .. } => Key::Window(edge),
            Self::Rule { rule, .. } => Key::Rule(rule),
            Self::Movement { action, phase, .. } => Key::Movement(action, phase),
        }
    }
}

/// A class-, weapon- or ability-specific value replacing the shared
/// baseline definition (Atlas 5.7.6).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Override {
    pub scope: Scope,
    pub setting: Setting,
}

/// Where an effective value comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    /// The shared definition.
    Baseline,
    /// The override for this scope.
    Override(Scope),
    /// The authored action in this slot defines it (#414).
    Authored(u8),
}

/// An effective value and its origin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Effective {
    pub setting: Setting,
    pub origin: Origin,
}

/// A complete action graph: the edges, window modifiers, compatibility
/// rules, interruptions, per-phase movement and overrides a zone runs. The
/// [`ACTIONS`] and their recovery modifiers are shared by every graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionGraph {
    pub edges: &'static [EdgeDef],
    pub window_modifiers: &'static [WindowModifier],
    pub rules: &'static [CompatibilityRule],
    pub interruptions: &'static [InterruptionRule],
    /// Authored movement of action phases; a phase without one moves freely
    /// ([`Movement::FREE`]).
    pub movement: &'static [MovementRule],
    pub overrides: &'static [Override],
    /// New experimental actions (#414); the baseline has none.
    pub authored: &'static [AuthoredAction],
}

/// The authored actions.
pub const ACTIONS: [ActionDef; 14] = [
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
    // A swim lasts while the player is in deep water.
    ActionDef {
        id: ActionId::Swim,
        region: Region::Movement,
        recovery_ms: None,
    },
    // A climb lasts while the player holds the face, a mantle until it lands (#490).
    ActionDef {
        id: ActionId::Climb,
        region: Region::Movement,
        recovery_ms: None,
    },
    ActionDef {
        id: ActionId::Mantle,
        region: Region::Movement,
        recovery_ms: None,
    },
    // A dodge roll lasts its fixed time (#516); its cooldown is no recovery.
    ActionDef {
        id: ActionId::Roll,
        region: Region::Movement,
        recovery_ms: None,
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
    // A cast lasts its ability's cast time; it has no recovery of its own.
    ActionDef {
        id: ActionId::Cast,
        region: Region::Offense,
        recovery_ms: None,
    },
    ActionDef {
        id: ActionId::GlobalCooldown,
        region: Region::Offense,
        recovery_ms: Some(1_500),
    },
    // A basic strike recovers for its weapon and variant's interval, not a fixed time.
    ActionDef {
        id: ActionId::BasicMelee,
        region: Region::Offense,
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

// The production graph's edges, rules, interruptions and overrides. The
// Action Lab's promotion (#383) regenerates this file from a reviewed
// configuration; it is plain source and is reviewed like any other change.
#[rustfmt::skip]
mod baseline;
pub use baseline::{
    AUTHORED_ACTIONS, BASELINE, EDGES, INTERRUPTIONS, MOVEMENT, OVERRIDES, RULES, WINDOW_MODIFIERS,
};

// The definitions stay here; validation, the runtime and the inspection read them (#1231).
mod inspection;
mod runtime;
mod validation;
pub use inspection::*;
pub use runtime::*;
pub use validation::*;

/// The shared shape of every authored action: offense, with no committed
/// recovery a compatibility rule could hold during.
const AUTHORED_SHAPE: ActionDef = ActionDef {
    id: ActionId::Authored(0),
    region: Region::Offense,
    recovery_ms: None,
};

/// A stable number for `id`, for comparisons in const context.
const fn action_code(id: ActionId) -> u16 {
    match id {
        ActionId::Airborne => 0,
        ActionId::Landing => 1,
        ActionId::DirectedSlash => 2,
        ActionId::Guard => 3,
        ActionId::Stun => 4,
        ActionId::GuardBreak => 5,
        ActionId::Death => 6,
        ActionId::Cast => 7,
        ActionId::GlobalCooldown => 8,
        ActionId::Swim => 9,
        ActionId::Climb => 10,
        ActionId::Mantle => 11,
        ActionId::Roll => 12,
        ActionId::BasicMelee => 13,
        ActionId::Authored(slot) => 256 + slot as u16,
    }
}

/// The most lateness an online input's observation can make up for (Atlas
/// 5.9.6, #415): 150 ms, nine ticks at 60 Hz.
///
/// A client may say which completed zone tick it had last observed when it
/// sent a timing-sensitive command (`ZoneSimulation::apply_observed_command`).
/// The input's **lateness** is how many ticks the zone completed between that
/// observation and the command's arrival, at most this grace. An edge whose
/// window closed no more than that lateness before arrival is still open for
/// the input. Lateness rescues only that ordinary window: a precision window
/// counts only when the input resolves inside it on arrival (owner decision,
/// #415 Q2). Nothing is backdated: the follow-up
/// resolves in the arrival tick against the current world, and every other
/// rule (holds, cancels, conditions, interruptions, cooldowns, range, Rage)
/// reads the arrival state. An observation can be neither ahead of the zone
/// nor older than the player's previous one, and without one the input is
/// not late.
pub const LATE_INPUT_GRACE_MS: u32 = 150;
/// [`LATE_INPUT_GRACE_MS`] in ticks.
pub const LATE_INPUT_GRACE_TICKS: u8 = {
    let ticks = crate::ticks_from_ms(LATE_INPUT_GRACE_MS);
    assert!(ticks <= u8::MAX as u16, "the late-input grace is too long");
    ticks as u8
};

/// The definition of `id`; for an authored action, their shared shape.
#[must_use]
pub fn action(id: ActionId) -> &'static ActionDef {
    if let ActionId::Authored(_) = id {
        return &AUTHORED_SHAPE;
    }
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
        if action_code(ACTIONS[index].id) == action_code(id) {
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
        if action_code(modifier.action) == action_code(id)
            && modifier.condition as u8 == condition as u8
        {
            return crate::ticks_from_ms(modifier.recovery_ms);
        }
        index += 1;
    }
    recovery_ticks(id)
}

/// The window of the first Warden combo, in const form for its constants.
pub(crate) const SLASH_TO_SHIELD_BASH: Window = EDGES[0].window;

pub(crate) const fn input_region(input: ActionInput) -> Region {
    match input {
        ActionInput::WardenMelee | ActionInput::ShieldBash | ActionInput::Ability => {
            Region::Offense
        }
        ActionInput::GuardProtection => Region::Defense,
        ActionInput::Move | ActionInput::Jump => Region::Movement,
    }
}

/// The phases `id` runs in, as [`ZoneSimulation::action_inspection`]
/// reports them.
#[must_use]
pub const fn phases(id: ActionId) -> &'static [Phase] {
    match id {
        ActionId::Airborne
        | ActionId::Swim
        | ActionId::Climb
        | ActionId::Mantle
        | ActionId::Roll
        | ActionId::Guard
        | ActionId::Cast
        | ActionId::Stun
        | ActionId::Death => &[Phase::Active],
        ActionId::Landing | ActionId::GlobalCooldown | ActionId::GuardBreak => &[Phase::Recovery],
        ActionId::DirectedSlash => &[
            Phase::Startup,
            Phase::Active,
            Phase::Recovery,
            Phase::FollowUp,
        ],
        ActionId::BasicMelee => &[Phase::Startup, Phase::Recovery, Phase::FollowUp],
        ActionId::Authored(_) => &[Phase::Startup, Phase::Active, Phase::Recovery],
    }
}

/// The graph input whose edges a command's intent may answer: a class
/// ability use or a directed strike.
#[must_use]
pub fn command_input(command: &crate::ZoneCommand) -> Option<ActionInput> {
    match *command {
        crate::ZoneCommand::UseAbility {
            ability, target, ..
        } => input_of(&PlayerIntent::UseAbility {
            ability,
            target,
            facing: None,
        }),
        crate::ZoneCommand::DirectedStrike { .. } => Some(ActionInput::WardenMelee),
        _ => None,
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
        PlayerIntent::UseAbility { .. } => Some(ActionInput::Ability),
        PlayerIntent::DirectedStrike { .. } => Some(ActionInput::WardenMelee),
        _ => None,
    }
}

/// Whose values apply: a player's class, main-hand weapon and the ability
/// an action runs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Context {
    pub class: Option<PlayerClass>,
    pub weapon: Option<ItemId>,
    pub ability: Option<AbilityId>,
}

impl Context {
    fn of(state: &PlayerState, ability: Option<AbilityId>) -> Self {
        Self {
            class: state.class.map(|choice| choice.class),
            weapon: state.equipment.slots()[usize::from(EquipmentSlot::MainHand.index())],
            ability,
        }
    }

    fn matches(self, scope: Scope) -> bool {
        match scope {
            Scope::Class(class) => self.class == Some(class),
            Scope::Weapon(item) => self.weapon == Some(item),
            Scope::Ability(id) => self.ability == Some(id),
        }
    }
}

impl ActionGraph {
    /// The effective value of `key` in `context`: the most specific
    /// matching override, otherwise the baseline. `None` when the graph has
    /// no such value.
    #[must_use]
    pub fn resolve(&self, key: Key, context: Context) -> Option<Effective> {
        let baseline = match key {
            Key::Recovery(id) => action(id)
                .recovery_ms
                .map(|ms| Setting::Recovery { action: id, ms }),
            Key::Window(id) => self.edge(id).map(|def| Setting::Window {
                edge: id,
                window: def.window,
            }),
            Key::Rule(id) => self
                .rules
                .iter()
                .any(|rule| rule.id == id)
                .then_some(Setting::Rule {
                    rule: id,
                    applies: true,
                }),
            Key::Movement(id, phase) => {
                self.movement_rule(id, phase).map(|rule| Setting::Movement {
                    action: id,
                    phase,
                    movement: rule.movement,
                })
            }
        }?;
        Some(
            self.overrides
                .iter()
                .filter(|entry| entry.setting.key() == key && context.matches(entry.scope))
                .max_by_key(|entry| entry.scope.rank())
                .map_or(
                    Effective {
                        setting: baseline,
                        origin: Origin::Baseline,
                    },
                    |entry| Effective {
                        setting: entry.setting,
                        origin: Origin::Override(entry.scope),
                    },
                ),
        )
    }
}
