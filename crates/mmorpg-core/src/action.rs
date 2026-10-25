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
//! (5.2.37–5.2.51, 5.2.69–5.2.70) expressed as data, with unchanged values:
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
//!   velocity, so Rapier still resolves every contact; running phases
//!   combine to the most restrictive values. Edges out of a landing
//!   ([`EdgeId::LandingToMelee`] and its siblings, anchored at touchdown)
//!   win over the landing's hold, end its recovery and may carry an
//!   [`Effect::Impulse`]: run → jump → aerial attack → landing → follow-up
//!   composes from ordinary actions. The baseline authors none of it, so
//!   today's movement is unchanged.
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
//! movement region, `cast`, `strike_recovery`, `combo` and `global_cooldown`
//! for the offense region, `guard` for the defense region, and auras, the
//! guard break and health for the reaction region.
//! [`ZoneSimulation::action_inspection`] is the read-only query surface that
//! the Action Lab (#381) reads.

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
    /// for the physics step of the tick it resolves in. Rapier still resolves
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
/// hands the result to Rapier's character controller as the desired
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
    /// An attack in water waits until the swimmer is ashore (#413).
    SwimmingHolds,
}

/// What a compatibility rule does with its input.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuleEffect {
    /// While `during` recovers, the input waits (is held) until the
    /// recovery ends. During a swim, it waits until the swim ends.
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
pub const ACTIONS: [ActionDef; 10] = [
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
    /// A holding rule covers an action that has no recovery phase (other
    /// than a swim).
    NoRecovery(RuleId),
    /// A duration is shorter than half a tick or too long for a tick timer.
    BadDuration(EdgeId),
    /// The zone has no runtime for this edge's source, input or anchor: a
    /// directed slash answered by Shield Bash from its impact or confirmed
    /// result, and a landing answered by Warden melee, Shield Bash or another
    /// ability from the touchdown, each under its own id.
    UnsupportedEdge(EdgeId),
    /// A landing edge's window closes after the shortest landing recovery
    /// its conditions allow, when the landing it answers is over.
    OutlastsSource(EdgeId),
    /// A movement rule covers a reaction or a phase its action never runs
    /// in, or its values are out of range.
    BadMovement(ActionId, Phase),
    /// Two movement rules cover the same action phase.
    DuplicateMovement(ActionId, Phase),
    /// An override names an action phase the graph authors no movement for.
    UnknownMovement(ActionId, Phase),
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
    /// The zone has no runtime for this rule: only movement and jumps
    /// cancel, and only a cast; only offense inputs are refused, and only
    /// during a swim.
    UnsupportedRule(RuleId),
    /// The zone does not read this value per class, weapon or ability: only
    /// the directed slash's and the global cooldown's recoveries, edge
    /// windows, rules and movement can be overridden.
    UnsupportedOverride(Key),
    /// An override's value is malformed, or outlasts the baseline value that
    /// canonical snapshots bound the timer by.
    BadOverride(Key),
    /// An override's scope names no class ability or main-hand catalog item.
    BadScope(Scope),
    /// Two overrides of the same value in the same scope.
    ConflictingOverride(Scope, Key),
    /// An authored action is malformed or uses a primitive that does not
    /// exist (#414).
    BadAuthored(u8, AuthoredProblem),
    /// Two authored actions share a slot.
    DuplicateAuthored(u8),
    /// Two authored actions of one class have the same trigger: the first
    /// slot, then the second.
    ConflictingTrigger(u8, u8),
    /// An edge leaves an authored action the graph lacks.
    UnknownAuthored(u8),
}

pub(crate) const fn input_region(input: ActionInput) -> Region {
    match input {
        ActionInput::WardenMelee | ActionInput::ShieldBash | ActionInput::Ability => {
            Region::Offense
        }
        ActionInput::GuardProtection => Region::Defense,
        ActionInput::Move | ActionInput::Jump => Region::Movement,
    }
}

/// Whether `ms` converts to a tick duration without failing.
fn duration_ok(ms: u32) -> bool {
    let ticks = u64::from(ms) * u64::from(crate::TICK_HZ);
    (ms == 0 || ticks + 500 >= 1_000) && (ticks + 500) / 1_000 <= u64::from(u16::MAX)
}

/// The shortest landing recovery, in ticks, for an edge under `conditions`:
/// the lengthened one when the edge requires an airborne melee action.
fn landing_ticks(conditions: &[Condition]) -> u16 {
    if conditions.contains(&Condition::AttackedAirborne) {
        modified_recovery_ticks(ActionId::Landing, Condition::AttackedAirborne)
    } else {
        recovery_ticks(ActionId::Landing)
    }
}

/// The phases `id` runs in, as [`ZoneSimulation::action_inspection`]
/// reports them.
#[must_use]
pub const fn phases(id: ActionId) -> &'static [Phase] {
    match id {
        ActionId::Airborne
        | ActionId::Swim
        | ActionId::Guard
        | ActionId::Cast
        | ActionId::Stun
        | ActionId::Death => &[Phase::Active],
        ActionId::Landing | ActionId::GlobalCooldown | ActionId::GuardBreak => &[Phase::Recovery],
        ActionId::DirectedSlash => &[Phase::Recovery, Phase::FollowUp],
        ActionId::Authored(_) => &[Phase::Startup, Phase::Active, Phase::Recovery],
    }
}

/// Whether movement values are in range.
pub(crate) fn validate_movement_values(movement: Movement) -> bool {
    movement.steering_percent <= 100
        && movement.momentum_percent <= 100
        && movement
            .max_acceleration_cms2
            .is_none_or(|limit| (1..=MAX_ACCELERATION_CMS2).contains(&limit))
}

/// The checks authored or overridden movement passes.
fn validate_movement(id: ActionId, phase: Phase, movement: Movement) -> Result<(), GraphError> {
    // A reaction stops the player by its own rules (a stun stands still);
    // an authored action authors its phases' movement itself.
    if action(id).region == Region::Reaction
        || matches!(id, ActionId::Authored(_))
        || !phases(id).contains(&phase)
        || !validate_movement_values(movement)
    {
        return Err(GraphError::BadMovement(id, phase));
    }
    Ok(())
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
        for (index, def) in self.authored.iter().enumerate() {
            self.validate_authored(index, def)?;
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
            self.validate_window(def, def.window)?;
        }
        for rule in self.rules {
            if action(rule.during).region == input_region(rule.input) {
                return Err(GraphError::SameRegion(rule.during));
            }
            match rule.effect {
                RuleEffect::Hold
                    if action(rule.during).recovery_ms.is_none()
                        && rule.during != ActionId::Swim =>
                {
                    return Err(GraphError::NoRecovery(rule.id));
                }
                RuleEffect::Refuse
                    if rule.during != ActionId::Swim
                        || input_region(rule.input) != Region::Offense =>
                {
                    return Err(GraphError::UnsupportedRule(rule.id));
                }
                RuleEffect::Cancel
                    if rule.during != ActionId::Cast
                        || !matches!(rule.input, ActionInput::Move | ActionInput::Jump) =>
                {
                    return Err(GraphError::UnsupportedRule(rule.id));
                }
                _ => {}
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
        for (index, rule) in self.movement.iter().enumerate() {
            validate_movement(rule.action, rule.phase, rule.movement)?;
            if self.movement[..index]
                .iter()
                .any(|other| other.action == rule.action && other.phase == rule.phase)
            {
                return Err(GraphError::DuplicateMovement(rule.action, rule.phase));
            }
        }
        for (index, entry) in self.overrides.iter().enumerate() {
            self.validate_override(entry)?;
            let key = entry.setting.key();
            if self.overrides[..index]
                .iter()
                .any(|other| other.scope == entry.scope && other.setting.key() == key)
            {
                return Err(GraphError::ConflictingOverride(entry.scope, key));
            }
        }
        Ok(())
    }

    fn validate_edge(&self, index: usize, def: &EdgeDef) -> Result<(), GraphError> {
        if self.edges[..index].iter().any(|other| other.id == def.id) {
            return Err(GraphError::DuplicateEdge(def.id));
        }
        let mut durations = Vec::new();
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
            .clone()
            .filter(|effect| matches!(effect, Effect::Recovery { .. }))
            .count()
            > 1
        {
            return Err(GraphError::BadEffect(def.id));
        }
        if let ActionId::Authored(_) = def.from {
            self.validate_authored_edge(def)?;
        }
        let supported = matches!(def.from, ActionId::Authored(_))
            || matches!(
                (def.id, def.from, def.input, def.anchor),
                (
                    EdgeId::SlashToShieldBash,
                    ActionId::DirectedSlash,
                    ActionInput::ShieldBash,
                    Anchor::Impact | Anchor::ConfirmedResult,
                ) | (
                    EdgeId::LandingToMelee,
                    ActionId::Landing,
                    ActionInput::WardenMelee,
                    Anchor::Landing,
                ) | (
                    EdgeId::LandingToShieldBash,
                    ActionId::Landing,
                    ActionInput::ShieldBash,
                    Anchor::Landing,
                ) | (
                    EdgeId::LandingToAbility,
                    ActionId::Landing,
                    ActionInput::Ability,
                    Anchor::Landing,
                )
            );
        if !supported {
            return Err(GraphError::UnsupportedEdge(def.id));
        }
        if def.effects.contains(&Effect::TargetConfirmed)
            && !def.conditions.contains(&Condition::PositiveHealthDamage)
        {
            return Err(GraphError::TargetUnconfirmed(def.id));
        }
        // A landing has no target to confirm and no opening to keep: the
        // follow-up ends its recovery.
        if def.from == ActionId::Landing
            && effects
                .clone()
                .any(|effect| matches!(effect, Effect::TargetConfirmed | Effect::KeepOpening))
        {
            return Err(GraphError::BadEffect(def.id));
        }
        let mut impulses = 0;
        for effect in effects {
            if let Effect::Impulse {
                forward_cms,
                up_cms,
            } = *effect
            {
                impulses += 1;
                if forward_cms.unsigned_abs() > MAX_IMPULSE_FORWARD_CMS
                    || up_cms > MAX_IMPULSE_UP_CMS
                    || (forward_cms == 0 && up_cms == 0)
                {
                    return Err(GraphError::BadEffect(def.id));
                }
            }
        }
        if impulses > 1 {
            return Err(GraphError::BadEffect(def.id));
        }
        Ok(())
    }

    /// The checks an edge's window passes, authored or overridden.
    fn validate_window(&self, def: &EdgeDef, window: Window) -> Result<(), GraphError> {
        if ![window.open_after_ms, window.actionable_ms, window.buffer_ms]
            .into_iter()
            .all(duration_ok)
        {
            return Err(GraphError::BadDuration(def.id));
        }
        if window.buffer_ms > window.open_after_ms {
            return Err(GraphError::BufferBeforeAnchor(def.id));
        }
        if let Some(source) = self.source_ticks(def) {
            // Every enlargement applying at once must still end in time.
            let enlargement: u32 = self
                .window_modifiers
                .iter()
                .filter(|modifier| modifier.edge == def.id && modifier.actionable_delta_ms > 0)
                .map(|modifier| {
                    u32::from(crate::ticks_from_ms(
                        modifier.actionable_delta_ms.unsigned_abs(),
                    ))
                })
                .sum();
            if u32::from(window.ticks().closes()) + enlargement >= u32::from(source) {
                return Err(GraphError::OutlastsSource(def.id));
            }
        }
        if let Some(precision) = def.precision {
            let (_, last) = precision.ticks(window.open_after_ticks());
            if precision.length_ms == 0 || last > window.ticks().closes() {
                return Err(GraphError::PrecisionOutsideWindow(def.id));
            }
        }
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
        if reduction > u32::from(window.actionable_ticks()) {
            return Err(GraphError::NegativeWindow(def.id));
        }
        Ok(())
    }

    fn validate_override(&self, entry: &Override) -> Result<(), GraphError> {
        let scoped = match entry.scope {
            Scope::Class(_) => true,
            Scope::Weapon(item) => crate::item_template(item)
                .is_some_and(|template| template.slot == Some(EquipmentSlot::MainHand)),
            Scope::Ability(id) => crate::ability::ability_by_id(id)
                .is_some_and(|ability| matches!(ability.user, AbilityUser::Class(_))),
        };
        if !scoped {
            return Err(GraphError::BadScope(entry.scope));
        }
        let key = entry.setting.key();
        match entry.setting {
            Setting::Recovery { action: id, ms } => {
                if !matches!(id, ActionId::DirectedSlash | ActionId::GlobalCooldown) {
                    return Err(GraphError::UnsupportedOverride(key));
                }
                // Canonical snapshots bound the timer by the baseline.
                if !duration_ok(ms) || Some(ms) > action(id).recovery_ms {
                    return Err(GraphError::BadOverride(key));
                }
            }
            Setting::Window { edge, window } => {
                let Some(def) = self.edge(edge) else {
                    return Err(GraphError::UnknownEdge(edge));
                };
                self.validate_window(def, window)?;
                // Canonical snapshots bound an opening's age by the baseline.
                let bound = EDGES
                    .iter()
                    .find(|baseline| baseline.id == edge)
                    .map_or(def.window, |baseline| baseline.window);
                if window.ticks().closes() > bound.ticks().closes() {
                    return Err(GraphError::BadOverride(key));
                }
            }
            Setting::Rule { rule, .. } => {
                if !self.rules.iter().any(|other| other.id == rule) {
                    return Err(GraphError::UnknownRule(rule));
                }
            }
            Setting::Movement {
                action: id,
                phase,
                movement,
            } => {
                if self.movement_rule(id, phase).is_none() {
                    return Err(GraphError::UnknownMovement(id, phase));
                }
                validate_movement(id, phase, movement)?;
            }
        }
        Ok(())
    }

    /// The shortest run, in ticks from its anchor, of an edge's source that
    /// ends by itself: a landing's recovery, an authored action after its
    /// use. An edge out of one must close before it ends.
    fn source_ticks(&self, def: &EdgeDef) -> Option<u16> {
        match def.from {
            ActionId::Landing => Some(landing_ticks(def.conditions)),
            ActionId::Authored(slot) => self
                .authored_action(slot)
                .map(AuthoredAction::after_impact_ticks),
            _ => None,
        }
    }

    /// The movement rule of `id` in `phase`.
    #[must_use]
    pub fn movement_rule(&self, id: ActionId, phase: Phase) -> Option<&'static MovementRule> {
        self.movement
            .iter()
            .find(|rule| rule.action == id && rule.phase == phase)
    }

    fn validate_interruption(&self, rule: &InterruptionRule) -> Result<(), GraphError> {
        if action(rule.reaction).region != Region::Reaction {
            return Err(GraphError::NotAReaction(rule.reaction));
        }
        let supported = match rule.interrupted {
            Interrupted::Action(ActionId::Guard) => true,
            // A stun or death always ends an authored action.
            Interrupted::Action(ActionId::Authored(_)) => false,
            // Death always ends a cast; a cast cannot pause.
            Interrupted::Action(ActionId::Cast) => match rule.consequence {
                Consequence::Cancel => true,
                Consequence::Preserve => rule.reaction != ActionId::Death,
                Consequence::Suspend => false,
            },
            Interrupted::Action(_) => rule.consequence == Consequence::Preserve,
            Interrupted::Edge(id) => {
                let Some(def) = self.edge(id) else {
                    return Err(GraphError::UnknownEdge(id));
                };
                // A landing's recovery and an authored action run on; their
                // edges cannot pause.
                !matches!(def.from, ActionId::Landing | ActionId::Authored(_))
                    || rule.consequence != Consequence::Suspend
            }
            Interrupted::Held(id) => {
                if !self.rules.iter().any(|other| other.id == id) {
                    return Err(GraphError::UnknownRule(id));
                }
                rule.consequence != Consequence::Suspend
                    && self
                        .rules
                        .iter()
                        .any(|other| other.id == id && other.effect == RuleEffect::Hold)
            }
        };
        if supported {
            Ok(())
        } else {
            Err(GraphError::UnsupportedInterruption(rule.interrupted))
        }
    }
}

/// The graph input whose edges a command's intent may answer: a class
/// ability use or a directed strike.
#[must_use]
pub fn command_input(command: &crate::ZoneCommand) -> Option<ActionInput> {
    match *command {
        crate::ZoneCommand::UseAbility { ability, target } => {
            input_of(&PlayerIntent::UseAbility { ability, target })
        }
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

/// The ability a running action runs: Heroic Strike for the directed
/// slash, the cast's ability for a cast.
fn action_ability(state: &PlayerState, id: ActionId) -> Option<AbilityId> {
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
fn window_for(graph: &ActionGraph, state: &PlayerState, def: &EdgeDef) -> (Window, Origin) {
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
fn rule_applies(graph: &ActionGraph, state: &PlayerState, rule: &CompatibilityRule) -> Effective {
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

fn applies(graph: &ActionGraph, state: &PlayerState, rule: &CompatibilityRule) -> bool {
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

/// Whether the player swims: alive, in deep water after the last physics
/// step.
fn swimming(state: &PlayerState) -> bool {
    state.swimming && state.is_alive()
}

/// Whether a holding rule's action is in the phase it holds in: a swim
/// while it lasts, any other action while it recovers.
fn holds_during(state: &PlayerState, id: ActionId) -> bool {
    match id {
        ActionId::Swim => swimming(state),
        _ => recovery_left(state, id).is_some(),
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
        ActionId::GlobalCooldown => state.global_cooldown,
        ActionId::Airborne
        | ActionId::Swim
        | ActionId::Guard
        | ActionId::Stun
        | ActionId::Death
        | ActionId::Cast
        | ActionId::Authored(_) => 0,
    };
    (left > 0).then_some(left)
}

/// The ticks a running cast has left, with its ability.
fn cast_left(state: &PlayerState) -> Option<(AbilityId, u16)> {
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
fn region_actions(graph: &ActionGraph, state: &PlayerState) -> [RegionReport; 4] {
    let report = |region, action: Option<ActionId>, phase, recovery_left| RegionReport {
        region,
        action,
        phase,
        recovery_left,
        ability: action.and_then(|id| action_ability(state, id)),
        active_left: None,
    };
    let movement = if swimming(state) {
        report(
            Region::Movement,
            Some(ActionId::Swim),
            Some(Phase::Active),
            None,
        )
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
    // A cast outranks the slash, and both outrank the global cooldown they
    // run alongside.
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
            // A swim holds while it runs; every other action while it recovers.
            Some(if held.during == ActionId::Swim {
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
struct EdgeEval {
    status: EdgeStatus,
    /// Ticks since the anchor, while the source runs.
    elapsed: Option<u16>,
    window: TickWindow,
    /// An input now falls inside the precision window.
    precise: bool,
    /// Open only because the input is late.
    compensated: bool,
}

/// Evaluates `def` for an input `late` ticks late (see [`LATE_INPUT_GRACE_MS`]).
fn evaluate(graph: &ActionGraph, state: &PlayerState, def: &EdgeDef, late: u8) -> EdgeEval {
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
fn movement_sources(graph: &ActionGraph, state: &PlayerState) -> Vec<MovementSource> {
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

/// One region's running action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionReport {
    pub region: Region,
    pub action: Option<ActionId>,
    pub phase: Option<Phase>,
    /// Ticks of the committed recovery left.
    pub recovery_left: Option<u16>,
    /// The ability the action runs: a cast's, or Heroic Strike for the
    /// directed slash.
    pub ability: Option<AbilityId>,
    /// Ticks of a cast or channel left.
    pub active_left: Option<u16>,
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
    /// Where the edge's window comes from.
    pub origin: Origin,
}

/// One compatibility rule's state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleReport {
    pub rule: RuleId,
    pub input: ActionInput,
    pub effect: RuleEffect,
    /// The first tick the rule no longer applies to the input: the held
    /// input is released, or the cast it would cancel has ended. `None`
    /// while a swim holds or refuses it: until the swimmer is ashore.
    pub released_at: Option<u64>,
    /// Why the rule applies to this player.
    pub origin: Origin,
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
    /// Every overridable value as it applies to this player now, with its
    /// origin: the directed slash's and the global cooldown's recoveries,
    /// each edge's window and whether each rule applies. An ability override
    /// of the global cooldown shows while that ability casts; it applies
    /// whenever that ability starts the cooldown. Each authored movement
    /// rule's value is listed too.
    pub effective: Vec<Effective>,
    /// The movement the next tick's controller velocity follows.
    pub movement: MovementReport,
    /// Each authored action, and whether its trigger would start it now
    /// (#414).
    pub authored: Vec<AuthoredReport>,
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

    /// The player's action graph after the last completed tick.
    #[must_use]
    pub fn action_inspection(&self, player_id: PlayerId) -> Option<ActionInspection> {
        let state = self.players.get(&player_id)?;
        let graph = self.action_graph;
        let tick = self.tick;
        // Intents of the next tick resolve before its timers advance.
        let next = tick + 1;
        let edges = graph
            .edges
            .iter()
            .map(|def| {
                let eval = evaluate(graph, state, def, 0);
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
                    origin: window_for(graph, state, def).1,
                }
            })
            .collect();
        let rules = graph
            .rules
            .iter()
            .filter_map(|rule| {
                // Recovery counts down after the next tick's intents and
                // combat resolve; movement cancels a cast before it advances.
                let left = match (rule.effect, rule.during) {
                    (_, ActionId::Swim) => swimming(state).then_some(None)?,
                    (RuleEffect::Hold, _) => Some(recovery_left(state, rule.during)?),
                    (RuleEffect::Cancel, _) => Some(
                        cast_left(state)
                            .filter(|_| rule.during == ActionId::Cast)?
                            .1,
                    ),
                    (RuleEffect::Refuse, _) => return None,
                };
                applies(graph, state, rule).then(|| RuleReport {
                    rule: rule.id,
                    input: rule.input,
                    effect: rule.effect,
                    released_at: left.map(|left| next + u64::from(left)),
                    origin: rule_applies(graph, state, rule).origin,
                })
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
        let sources = movement_sources(graph, state);
        Some(ActionInspection {
            tick,
            regions: region_actions(graph, state),
            edges,
            rules,
            interruptions,
            effective: effective_values(graph, state),
            movement: MovementReport {
                movement: sources
                    .iter()
                    .map(|source| source.movement)
                    .reduce(Movement::combine)
                    .unwrap_or(Movement::FREE),
                sources,
            },
            authored: authored::reports(graph, state),
        })
    }

    /// Explains how `input` would be arbitrated at the player's next tick.
    #[must_use]
    pub fn arbitrate_input(&self, player_id: PlayerId, input: ActionInput) -> Option<Arbitration> {
        self.arbitrate_late_input(player_id, input, 0)
    }

    /// [`Self::arbitrate_input`] for an input `late` ticks late; at most
    /// [`LATE_INPUT_GRACE_TICKS`] count.
    #[must_use]
    pub fn arbitrate_late_input(
        &self,
        player_id: PlayerId,
        input: ActionInput,
        late: u8,
    ) -> Option<Arbitration> {
        self.players
            .get(&player_id)
            .map(|state| arbitrate(self.action_graph, state, input, late))
    }
}

/// See [`ActionInspection::effective`].
fn effective_values(graph: &ActionGraph, state: &PlayerState) -> Vec<Effective> {
    let casting = state.cast.map(|cast| cast.ability);
    let mut keys = vec![
        (
            Key::Recovery(ActionId::DirectedSlash),
            Some(ids::HEROIC_STRIKE),
        ),
        (Key::Recovery(ActionId::GlobalCooldown), casting),
    ];
    keys.extend(
        graph
            .edges
            .iter()
            .map(|def| (Key::Window(def.id), action_ability(state, def.from))),
    );
    for rule in graph.rules {
        if !keys.iter().any(|(key, _)| *key == Key::Rule(rule.id)) {
            keys.push((Key::Rule(rule.id), action_ability(state, rule.during)));
        }
    }
    keys.extend(graph.movement.iter().map(|rule| {
        (
            Key::Movement(rule.action, rule.phase),
            action_ability(state, rule.action),
        )
    }));
    let mut values: Vec<Effective> = keys
        .into_iter()
        .filter_map(|(key, ability)| graph.resolve(key, Context::of(state, ability)))
        .collect();
    // Each authored action's phases move as it authors them (#414).
    for def in graph.authored {
        for phase in [Phase::Startup, Phase::Active, Phase::Recovery] {
            values.push(Effective {
                setting: Setting::Movement {
                    action: ActionId::Authored(def.slot),
                    phase,
                    movement: def.movement(phase),
                },
                origin: Origin::Authored(def.slot),
            });
        }
    }
    values
}
