//! Action Lab experiments (#382, Atlas 5.6.2/5.6.4): experimental action
//! graphs, recorded input scenarios, deterministic replay and A/B comparison.
//!
//! The lab owns no rules and no second simulation. An experiment is an
//! [`ActionGraph`] written as JSON ([`Config`]) that `mmorpg-core` validates
//! exactly like the production [`action::BASELINE`]; a zone runs it only
//! through [`ZoneSimulation::set_action_graph`] in a fresh, unsaved lab world.
//! Production rules and stored characters never see it.
//!
//! A [`ScenarioFile`] is the lab setup, seed, content revision, protocol ID and
//! the command-wire bytes the player sent with the tick each was applied at.
//! [`run`] replays it through the ordinary [`LocalZoneHost`] submit/tick path
//! and reports every tick's region actions, events, health, resource and
//! canonical-snapshot digest; [`compare`] runs it under two graphs and reports
//! the first tick whose canonical state differs.

use std::fmt::Debug;

use mmorpg_core::ability::ids;
use mmorpg_core::action::{
    self, ActionGraph, ActionId, ActionInput, Anchor, Arbitration, AuthoredAction, AuthoredPhase,
    BlockReason, CompatibilityRule, Condition, Consequence, Direction, EdgeDef, EdgeId, EdgeReport,
    EdgeStatus, Effect, InputContext, Interrupted, InterruptionRule, Movement, MovementRule,
    Override, Phase, Precision, RuleEffect, RuleId, Scope, Setting, Trigger, Window,
    WindowModifier,
};
use mmorpg_core::{AbilityId, EntityRef, ItemId, PlayerClass, ZoneCommand, ZoneEvent};
use mmorpg_protocol::{
    PROTOCOL_ID, decode_command, encode_canonical_snapshot, encode_observed_command,
};
use serde::{Deserialize, Serialize};

use crate::action_lab::{INPUTS, LabSetup};
use crate::host::LocalZoneHost;
use crate::lab_diagnostics::{self, Finding};

/// The `format` of an exported experiment configuration.
pub const CONFIG_FORMAT: &str = "mmorpg-action-lab-config";
/// The `format` of an exported scenario.
pub const SCENARIO_FORMAT: &str = "mmorpg-action-lab-scenario";
/// The longest scenario the lab replays: 2 minutes of ticks.
pub const MAX_SCENARIO_TICKS: u32 = 120 * mmorpg_core::TICK_HZ as u32;

const ANCHORS: [Anchor; 5] = [
    Anchor::Impact,
    Anchor::ConfirmedResult,
    Anchor::Landing,
    Anchor::PhaseStart,
    Anchor::PhaseEnd,
];
const CONDITIONS: [Condition; 7] = [
    Condition::PositiveHealthDamage,
    Condition::TargetAlive,
    Condition::Alive,
    Condition::Warden,
    Condition::AttackedAirborne,
    Condition::Airborne,
    Condition::Stunned,
];
/// Every edge id: the named ones, then `Authored(0)`… (#414).
fn edge_ids() -> impl Iterator<Item = EdgeId> + Clone {
    [
        EdgeId::SlashToShieldBash,
        EdgeId::LandingToMelee,
        EdgeId::LandingToShieldBash,
        EdgeId::LandingToAbility,
    ]
    .into_iter()
    .chain((0..action::MAX_AUTHORED_EDGES).map(EdgeId::Authored))
}
const CONTEXTS: [InputContext; 6] = [
    InputContext::Direct,
    InputContext::Airborne,
    InputContext::Moving(Direction::Any),
    InputContext::Moving(Direction::Forward),
    InputContext::Moving(Direction::Backward),
    InputContext::Moving(Direction::Sideways),
];
const RULES: [RuleId; 5] = [
    RuleId::LandingHoldsMelee,
    RuleId::SlashRecoveryHoldsGuard,
    RuleId::MovementCancelsCast,
    RuleId::SwimmingRefuses,
    RuleId::SwimmingHolds,
];
const RULE_EFFECTS: [RuleEffect; 3] = [RuleEffect::Hold, RuleEffect::Cancel, RuleEffect::Refuse];
const PHASES: [Phase; 4] = [
    Phase::Startup,
    Phase::Active,
    Phase::Recovery,
    Phase::FollowUp,
];
const CONSEQUENCES: [Consequence; 3] = [
    Consequence::Preserve,
    Consequence::Suspend,
    Consequence::Cancel,
];

/// Every action id: the shared ones, then `Authored(0)`… (#414).
fn action_ids() -> impl Iterator<Item = ActionId> {
    action::ACTIONS
        .iter()
        .map(|def| def.id)
        .chain((0..action::MAX_AUTHORED_ACTIONS).map(ActionId::Authored))
}

fn interrupted_values() -> impl Iterator<Item = Interrupted> {
    action_ids()
        .map(Interrupted::Action)
        .chain(edge_ids().map(Interrupted::Edge))
        .chain(RULES.into_iter().map(Interrupted::Held))
}

/// The value of `all` whose `Debug` text is `text`.
fn named<T: Copy + Debug>(
    all: impl IntoIterator<Item = T>,
    text: &str,
    what: &str,
) -> Result<T, String> {
    all.into_iter()
        .find(|value| format!("{value:?}") == text)
        .ok_or_else(|| format!("unknown {what} {text:?}"))
}

fn name<T: Debug>(value: T) -> String {
    format!("{value:?}")
}

fn leak<T>(values: Vec<T>) -> &'static [T] {
    if values.is_empty() {
        return &[];
    }
    Box::leak(values.into_boxed_slice())
}

fn conditions(names: &[String]) -> Result<&'static [Condition], String> {
    let parsed = names
        .iter()
        .map(|text| named(CONDITIONS, text, "condition"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(leak(parsed))
}

/// A typed effect with its magnitude.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum EffectDto {
    TargetConfirmed,
    IgnoreGlobalCooldown,
    Recovery {
        ms: u32,
    },
    ResourceRefund {
        amount: u16,
    },
    KeepOpening,
    /// A launch along the facing and upward, in cm/s (#413).
    Impulse {
        forward_cms: i16,
        up_cms: u16,
    },
}

impl EffectDto {
    const fn of(effect: Effect) -> Self {
        match effect {
            Effect::TargetConfirmed => Self::TargetConfirmed,
            Effect::IgnoreGlobalCooldown => Self::IgnoreGlobalCooldown,
            Effect::Recovery { ms } => Self::Recovery { ms },
            Effect::ResourceRefund { amount } => Self::ResourceRefund { amount },
            Effect::KeepOpening => Self::KeepOpening,
            Effect::Impulse {
                forward_cms,
                up_cms,
            } => Self::Impulse {
                forward_cms,
                up_cms,
            },
        }
    }

    const fn effect(&self) -> Effect {
        match *self {
            Self::TargetConfirmed => Effect::TargetConfirmed,
            Self::IgnoreGlobalCooldown => Effect::IgnoreGlobalCooldown,
            Self::Recovery { ms } => Effect::Recovery { ms },
            Self::ResourceRefund { amount } => Effect::ResourceRefund { amount },
            Self::KeepOpening => Effect::KeepOpening,
            Self::Impulse {
                forward_cms,
                up_cms,
            } => Effect::Impulse {
                forward_cms,
                up_cms,
            },
        }
    }
}

/// An action phase's bounded movement (#413, Atlas 5.9.2).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MovementRuleDto {
    pub action: String,
    pub phase: String,
    pub conditions: Vec<String>,
    pub steering_percent: u8,
    pub momentum_percent: u8,
    pub max_acceleration_cms2: Option<u32>,
}

const fn movement_of(steering_percent: u8, momentum_percent: u8, max: Option<u32>) -> Movement {
    Movement {
        steering_percent,
        momentum_percent,
        max_acceleration_cms2: max,
    }
}

fn effects(list: &[EffectDto]) -> &'static [Effect] {
    leak(list.iter().map(EffectDto::effect).collect())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrecisionDto {
    pub after_open_ms: u32,
    pub length_ms: u32,
    pub benefits: Vec<EffectDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeDto {
    pub id: String,
    pub from: String,
    pub input: String,
    pub anchor: String,
    pub open_after_ms: u32,
    pub actionable_ms: u32,
    pub buffer_ms: u32,
    pub conditions: Vec<String>,
    pub effects: Vec<EffectDto>,
    pub precision: Option<PrecisionDto>,
    pub priority: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowModifierDto {
    pub edge: String,
    pub conditions: Vec<String>,
    pub actionable_delta_ms: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuleDto {
    pub id: String,
    pub during: String,
    pub input: String,
    pub conditions: Vec<String>,
    pub effect: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InterruptionDto {
    pub reaction: String,
    /// `Action(Cast)`, `Edge(SlashToShieldBash)` or `Held(LandingHoldsMelee)`.
    pub interrupted: String,
    pub phase: Option<String>,
    pub consequence: String,
}

/// Who an override is for: a class by name, a main-hand item or an ability by ID.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ScopeDto {
    Class { class: String },
    Weapon { item: u16 },
    Ability { ability: u8 },
}

/// The value an override replaces.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum SettingDto {
    Recovery {
        action: String,
        ms: u32,
    },
    Window {
        edge: String,
        open_after_ms: u32,
        actionable_ms: u32,
        buffer_ms: u32,
    },
    Rule {
        rule: String,
        applies: bool,
    },
    Movement {
        action: String,
        phase: String,
        steering_percent: u8,
        momentum_percent: u8,
        max_acceleration_cms2: Option<u32>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverrideDto {
    pub scope: ScopeDto,
    pub setting: SettingDto,
}

/// An action graph as editable, exportable JSON. Its identity is a hash of
/// everything but its name, so equal rules have equal identities.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub format: String,
    pub name: String,
    pub edges: Vec<EdgeDto>,
    pub window_modifiers: Vec<WindowModifierDto>,
    pub rules: Vec<RuleDto>,
    pub interruptions: Vec<InterruptionDto>,
    /// Per-phase movement (#413); absent in configurations saved before it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub movement: Vec<MovementRuleDto>,
    pub overrides: Vec<OverrideDto>,
    /// New experimental actions (#414); absent when none is authored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authored: Vec<AuthoredActionDto>,
}

/// One timed phase of an authored action and its movement (#414).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredPhaseDto {
    pub ms: u32,
    pub steering_percent: u8,
    pub momentum_percent: u8,
    pub max_acceleration_cms2: Option<u32>,
}

impl AuthoredPhaseDto {
    const fn of(phase: AuthoredPhase) -> Self {
        Self {
            ms: phase.ms,
            steering_percent: phase.movement.steering_percent,
            momentum_percent: phase.movement.momentum_percent,
            max_acceleration_cms2: phase.movement.max_acceleration_cms2,
        }
    }

    const fn phase(&self) -> AuthoredPhase {
        AuthoredPhase {
            ms: self.ms,
            movement: movement_of(
                self.steering_percent,
                self.momentum_percent,
                self.max_acceleration_cms2,
            ),
        }
    }
}

/// A new experimental action composed from existing primitives (#414).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredActionDto {
    pub slot: u8,
    pub name: String,
    /// A class by name.
    pub class: String,
    /// `WardenMelee`, `ShieldBash` or `Ability`.
    pub input: String,
    /// Only this ability's use; `None` is any the input covers.
    pub ability: Option<u8>,
    /// `Direct`, `Airborne` or `Moving(Any|Forward|Backward|Sideways)`.
    pub context: String,
    pub conditions: Vec<String>,
    pub startup: AuthoredPhaseDto,
    pub active: AuthoredPhaseDto,
    pub recovery: AuthoredPhaseDto,
    pub effects: Vec<EffectDto>,
}

impl AuthoredActionDto {
    fn of(def: &AuthoredAction) -> Self {
        Self {
            slot: def.slot,
            name: def.name.to_owned(),
            class: def.class.name().to_owned(),
            input: name(def.trigger.input),
            ability: def.trigger.ability.map(|id| id.get()),
            context: name(def.trigger.context),
            conditions: def.conditions.iter().copied().map(name).collect(),
            startup: AuthoredPhaseDto::of(def.startup),
            active: AuthoredPhaseDto::of(def.active),
            recovery: AuthoredPhaseDto::of(def.recovery),
            effects: def.effects.iter().copied().map(EffectDto::of).collect(),
        }
    }

    fn action(&self) -> Result<AuthoredAction, String> {
        Ok(AuthoredAction {
            slot: self.slot,
            name: Box::leak(self.name.clone().into_boxed_str()),
            class: class_named(&self.class)?,
            trigger: Trigger {
                input: named(INPUTS, &self.input, "input")?,
                ability: self.ability.map(AbilityId::new),
                context: named(CONTEXTS, &self.context, "input context")?,
            },
            conditions: conditions(&self.conditions)?,
            startup: self.startup.phase(),
            active: self.active.phase(),
            recovery: self.recovery.phase(),
            effects: effects(&self.effects),
        })
    }
}

fn class_named(class: &str) -> Result<PlayerClass, String> {
    PlayerClass::ALL
        .into_iter()
        .find(|candidate| candidate.name() == class)
        .ok_or_else(|| format!("unknown class {class:?}"))
}

fn window_of(open_after_ms: u32, actionable_ms: u32, buffer_ms: u32) -> Window {
    Window {
        open_after_ms,
        actionable_ms,
        buffer_ms,
    }
}

impl Config {
    /// `graph` as a configuration called `name`.
    #[must_use]
    pub fn of(graph: &ActionGraph, name: &str) -> Self {
        let precision = |precision: Precision| PrecisionDto {
            after_open_ms: precision.after_open_ms,
            length_ms: precision.length_ms,
            benefits: precision
                .benefits
                .iter()
                .copied()
                .map(EffectDto::of)
                .collect(),
        };
        let names = |list: &[Condition]| list.iter().copied().map(self::name).collect();
        Self {
            format: CONFIG_FORMAT.to_owned(),
            name: name.to_owned(),
            edges: graph
                .edges
                .iter()
                .map(|def| EdgeDto {
                    id: self::name(def.id),
                    from: self::name(def.from),
                    input: self::name(def.input),
                    anchor: self::name(def.anchor),
                    open_after_ms: def.window.open_after_ms,
                    actionable_ms: def.window.actionable_ms,
                    buffer_ms: def.window.buffer_ms,
                    conditions: names(def.conditions),
                    effects: def.effects.iter().copied().map(EffectDto::of).collect(),
                    precision: def.precision.map(precision),
                    priority: def.priority,
                })
                .collect(),
            window_modifiers: graph
                .window_modifiers
                .iter()
                .map(|modifier| WindowModifierDto {
                    edge: self::name(modifier.edge),
                    conditions: names(modifier.conditions),
                    actionable_delta_ms: modifier.actionable_delta_ms,
                })
                .collect(),
            rules: graph
                .rules
                .iter()
                .map(|rule| RuleDto {
                    id: self::name(rule.id),
                    during: self::name(rule.during),
                    input: self::name(rule.input),
                    conditions: names(rule.conditions),
                    effect: self::name(rule.effect),
                })
                .collect(),
            interruptions: graph
                .interruptions
                .iter()
                .map(|rule| InterruptionDto {
                    reaction: self::name(rule.reaction),
                    interrupted: self::name(rule.interrupted),
                    phase: rule.phase.map(self::name),
                    consequence: self::name(rule.consequence),
                })
                .collect(),
            movement: graph
                .movement
                .iter()
                .map(|rule| MovementRuleDto {
                    action: self::name(rule.action),
                    phase: self::name(rule.phase),
                    conditions: names(rule.conditions),
                    steering_percent: rule.movement.steering_percent,
                    momentum_percent: rule.movement.momentum_percent,
                    max_acceleration_cms2: rule.movement.max_acceleration_cms2,
                })
                .collect(),
            overrides: graph
                .overrides
                .iter()
                .map(|entry| OverrideDto {
                    scope: match entry.scope {
                        Scope::Class(class) => ScopeDto::Class {
                            class: class.name().to_owned(),
                        },
                        Scope::Weapon(item) => ScopeDto::Weapon { item: item.get() },
                        Scope::Ability(id) => ScopeDto::Ability { ability: id.get() },
                    },
                    setting: match entry.setting {
                        Setting::Recovery { action, ms } => SettingDto::Recovery {
                            action: self::name(action),
                            ms,
                        },
                        Setting::Window { edge, window } => SettingDto::Window {
                            edge: self::name(edge),
                            open_after_ms: window.open_after_ms,
                            actionable_ms: window.actionable_ms,
                            buffer_ms: window.buffer_ms,
                        },
                        Setting::Rule { rule, applies } => SettingDto::Rule {
                            rule: self::name(rule),
                            applies,
                        },
                        Setting::Movement {
                            action,
                            phase,
                            movement,
                        } => SettingDto::Movement {
                            action: self::name(action),
                            phase: self::name(phase),
                            steering_percent: movement.steering_percent,
                            momentum_percent: movement.momentum_percent,
                            max_acceleration_cms2: movement.max_acceleration_cms2,
                        },
                    },
                })
                .collect(),
            authored: graph.authored.iter().map(AuthoredActionDto::of).collect(),
        }
    }

    /// The production graph as a configuration.
    #[must_use]
    pub fn baseline() -> Self {
        Self::of(&action::BASELINE, "Production baseline")
    }

    /// Parses exported JSON; unknown fields and formats fail closed.
    pub fn parse(json: &str) -> Result<Self, String> {
        let config: Self =
            serde_json::from_str(json).map_err(|error| format!("configuration: {error}"))?;
        if config.format != CONFIG_FORMAT {
            return Err(format!(
                "configuration format {:?} is not {CONFIG_FORMAT:?}",
                config.format
            ));
        }
        Ok(config)
    }

    /// A stable identity of the rules: `exp-` and a hash of everything but the name.
    #[must_use]
    pub fn identity(&self) -> String {
        let mut rules = self.clone();
        rules.name.clear();
        let text = serde_json::to_string(&rules).unwrap_or_default();
        format!("exp-{:016x}", fnv1a(text.as_bytes()))
    }

    /// The validated graph this configuration describes. The definitions are
    /// leaked: a lab session builds a handful, each a few hundred bytes.
    pub fn graph(&self) -> Result<&'static ActionGraph, String> {
        let edges = self
            .edges
            .iter()
            .map(|edge| {
                Ok(EdgeDef {
                    id: named(edge_ids(), &edge.id, "edge")?,
                    from: named(action_ids(), &edge.from, "action")?,
                    input: named(INPUTS, &edge.input, "input")?,
                    anchor: named(ANCHORS, &edge.anchor, "anchor")?,
                    window: window_of(edge.open_after_ms, edge.actionable_ms, edge.buffer_ms),
                    conditions: conditions(&edge.conditions)?,
                    effects: effects(&edge.effects),
                    precision: edge.precision.as_ref().map(|precision| Precision {
                        after_open_ms: precision.after_open_ms,
                        length_ms: precision.length_ms,
                        benefits: effects(&precision.benefits),
                    }),
                    priority: edge.priority,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let window_modifiers = self
            .window_modifiers
            .iter()
            .map(|modifier| {
                Ok(WindowModifier {
                    edge: named(edge_ids(), &modifier.edge, "edge")?,
                    conditions: conditions(&modifier.conditions)?,
                    actionable_delta_ms: modifier.actionable_delta_ms,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let rules = self
            .rules
            .iter()
            .map(|rule| {
                Ok(CompatibilityRule {
                    id: named(RULES, &rule.id, "rule")?,
                    during: named(action_ids(), &rule.during, "action")?,
                    input: named(INPUTS, &rule.input, "input")?,
                    conditions: conditions(&rule.conditions)?,
                    effect: named(RULE_EFFECTS, &rule.effect, "rule effect")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let interruptions = self
            .interruptions
            .iter()
            .map(|rule| {
                Ok(InterruptionRule {
                    reaction: named(action_ids(), &rule.reaction, "action")?,
                    interrupted: named(
                        interrupted_values(),
                        &rule.interrupted,
                        "interruption target",
                    )?,
                    phase: rule
                        .phase
                        .as_deref()
                        .map(|phase| named(PHASES, phase, "phase"))
                        .transpose()?,
                    consequence: named(CONSEQUENCES, &rule.consequence, "consequence")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let overrides = self
            .overrides
            .iter()
            .map(|entry| {
                let scope = match &entry.scope {
                    ScopeDto::Class { class } => Scope::Class(
                        PlayerClass::ALL
                            .into_iter()
                            .find(|candidate| candidate.name() == class)
                            .ok_or_else(|| format!("unknown class {class:?}"))?,
                    ),
                    ScopeDto::Weapon { item } => Scope::Weapon(ItemId::new(*item)),
                    ScopeDto::Ability { ability } => Scope::Ability(AbilityId::new(*ability)),
                };
                let setting = match &entry.setting {
                    SettingDto::Recovery { action, ms } => Setting::Recovery {
                        action: named(action_ids(), action, "action")?,
                        ms: *ms,
                    },
                    SettingDto::Window {
                        edge,
                        open_after_ms,
                        actionable_ms,
                        buffer_ms,
                    } => Setting::Window {
                        edge: named(edge_ids(), edge, "edge")?,
                        window: window_of(*open_after_ms, *actionable_ms, *buffer_ms),
                    },
                    SettingDto::Rule { rule, applies } => Setting::Rule {
                        rule: named(RULES, rule, "rule")?,
                        applies: *applies,
                    },
                    SettingDto::Movement {
                        action,
                        phase,
                        steering_percent,
                        momentum_percent,
                        max_acceleration_cms2,
                    } => Setting::Movement {
                        action: named(action_ids(), action, "action")?,
                        phase: named(PHASES, phase, "phase")?,
                        movement: movement_of(
                            *steering_percent,
                            *momentum_percent,
                            *max_acceleration_cms2,
                        ),
                    },
                };
                Ok(Override { scope, setting })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let movement = self
            .movement
            .iter()
            .map(|rule| {
                Ok(MovementRule {
                    action: named(action_ids(), &rule.action, "action")?,
                    phase: named(PHASES, &rule.phase, "phase")?,
                    conditions: conditions(&rule.conditions)?,
                    movement: movement_of(
                        rule.steering_percent,
                        rule.momentum_percent,
                        rule.max_acceleration_cms2,
                    ),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let graph = ActionGraph {
            edges: leak(edges),
            window_modifiers: leak(window_modifiers),
            rules: leak(rules),
            interruptions: leak(interruptions),
            movement: leak(movement),
            overrides: leak(overrides),
            authored: leak(
                self.authored
                    .iter()
                    .map(AuthoredActionDto::action)
                    .collect::<Result<Vec<_>, String>>()?,
            ),
        };
        graph
            .validate()
            .map_err(|error| format!("the graph is invalid: {error:?}"))?;
        if graph == action::BASELINE {
            return Ok(&action::BASELINE);
        }
        Ok(Box::leak(Box::new(graph)))
    }
}

/// 64-bit FNV-1a, for identities and digests (not security).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The lab setup of a scenario, as the page carries it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetupDto {
    pub class_name: String,
    pub level: u8,
    pub gear: String,
    pub main_hand: u16,
    pub resource: Option<u16>,
}

impl SetupDto {
    pub fn setup(&self) -> Result<LabSetup, String> {
        LabSetup::parse(
            &self.class_name,
            self.level,
            &self.gear,
            self.main_hand,
            self.resource,
        )
    }
}

/// One recorded command: the command-wire bytes (hex) applied before tick `tick + 1`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandDto {
    pub tick: u32,
    pub bytes: String,
}

/// What the recording itself produced, for checking that a replay reproduces it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordedDto {
    /// The identity of the rules the recording ran.
    pub config: String,
    /// The canonical-snapshot digest after its last tick.
    pub digest: String,
}

/// A named, exportable input scenario.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScenarioFile {
    pub format: String,
    pub name: String,
    pub setup: SetupDto,
    /// The world seed, decimal (a `u64` does not fit a JSON number).
    pub seed: String,
    /// The content revision recorded, decimal.
    pub content_revision: String,
    pub protocol_id: u32,
    /// Ticks the scenario lasts.
    pub ticks: u32,
    pub commands: Vec<CommandDto>,
    pub recorded: Option<RecordedDto>,
}

fn hex_bytes(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("command bytes {text:?} are not hex"));
    }
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16).map_err(|error| error.to_string())
        })
        .collect()
}

impl ScenarioFile {
    pub fn parse(json: &str) -> Result<Self, String> {
        let scenario: Self =
            serde_json::from_str(json).map_err(|error| format!("scenario: {error}"))?;
        if scenario.format != SCENARIO_FORMAT {
            return Err(format!(
                "scenario format {:?} is not {SCENARIO_FORMAT:?}",
                scenario.format
            ));
        }
        if scenario.protocol_id != PROTOCOL_ID {
            return Err(format!(
                "the scenario's commands are protocol {:#08x}, this build reads {PROTOCOL_ID:#08x}",
                scenario.protocol_id
            ));
        }
        if scenario.ticks > MAX_SCENARIO_TICKS {
            return Err(format!(
                "the scenario lasts {} ticks, at most {MAX_SCENARIO_TICKS} replay",
                scenario.ticks
            ));
        }
        Ok(scenario)
    }

    fn seed(&self) -> Result<u64, String> {
        self.seed
            .parse()
            .map_err(|_| format!("seed {:?} is not a u64", self.seed))
    }

    /// The commands in application order, decoded, with their bytes.
    fn commands(&self) -> Result<Vec<(u32, Vec<u8>, ZoneCommand)>, String> {
        let mut commands = self
            .commands
            .iter()
            .map(|command| {
                let bytes = hex_bytes(&command.bytes)?;
                let decoded = decode_command(&bytes)
                    .map_err(|error| format!("command at tick {}: {error:?}", command.tick))?;
                Ok((command.tick, bytes, decoded))
            })
            .collect::<Result<Vec<_>, String>>()?;
        // Stable: commands of one tick keep their recorded order.
        commands.sort_by_key(|(tick, ..)| *tick);
        Ok(commands)
    }
}

/// A simulated network between the scenario's player and the lab world
/// (#415, Atlas 5.9.6): the scenario's inputs keep their timing relative to
/// what the player saw, so each arrives one round trip (plus jitter) after its
/// recorded tick, in order, reporting the tick it was made against. The
/// presets are illustrative, not production thresholds.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Network {
    /// The round trip: the projection's way to the player and the input's way back.
    pub round_trip_ms: u32,
    /// Each input arrives up to this much later again, deterministically per
    /// input and seed.
    pub jitter_ms: u32,
    /// Timing-sensitive inputs report their observed tick, so the zone's
    /// bounded late-input grace applies; off, they arrive plain.
    pub compensate: bool,
}

/// The longest simulated round trip plus jitter: two seconds.
pub const MAX_NETWORK_MS: u32 = 2_000;

impl Network {
    /// No network: the local arena.
    pub const LOCAL: Self = Self {
        round_trip_ms: 0,
        jitter_ms: 0,
        compensate: true,
    };

    pub fn parse(json: &str) -> Result<Self, String> {
        let network: Self =
            serde_json::from_str(json).map_err(|error| format!("network: {error}"))?;
        if network.round_trip_ms.saturating_add(network.jitter_ms) > MAX_NETWORK_MS {
            return Err(format!(
                "a simulated network delays at most {MAX_NETWORK_MS} ms"
            ));
        }
        Ok(network)
    }

    /// The ticks input `index` of a scenario seeded `seed` travels.
    fn delay_ticks(&self, seed: u64, index: usize) -> u32 {
        let jitter = u64::from(mmorpg_core::ticks_from_ms(self.jitter_ms));
        let roll = if jitter == 0 {
            0
        } else {
            let mut bytes = seed.to_be_bytes().to_vec();
            bytes.extend_from_slice(&(index as u64).to_be_bytes());
            fnv1a(&bytes) % (jitter + 1)
        };
        u32::from(mmorpg_core::ticks_from_ms(self.round_trip_ms)) + roll as u32
    }
}

/// The canonical-snapshot digest of the lab world.
pub fn digest(host: &LocalZoneHost) -> Result<u64, String> {
    let snapshot = host.zone().snapshot().map_err(|error| error.to_string())?;
    let bytes = encode_canonical_snapshot(&snapshot).map_err(|error| format!("{error:?}"))?;
    Ok(fnv1a(&bytes))
}

/// One replayed tick.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickRecord {
    pub tick: u64,
    /// Per region (movement, offense, defense, reaction): `Action·Phase`, or empty.
    pub regions: Vec<String>,
    pub events: Vec<String>,
    pub health: u32,
    pub resource: Option<u16>,
    /// The body centre in whole cm and its horizontal speed in cm/s after the
    /// tick's physics step (#413): the physical result of movement tuning.
    pub position: [i32; 3],
    pub speed: u32,
    pub digest: String,
    /// The edges whose source runs, as the next tick's intents meet them
    /// (#383): status, anchor and eligible window in absolute ticks.
    #[serde(serialize_with = "edge_traces")]
    pub edges: Vec<EdgeReport>,
}

/// What one command led to.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub index: usize,
    pub tick: u32,
    pub label: String,
    /// Errors the zone reported in the tick that applied it; empty when accepted.
    pub refused: Vec<String>,
    /// The first tick with an effect of the player's (an attack, strike, cast
    /// or ability) before the next action command, for action commands.
    pub effect_tick: Option<u64>,
    /// `effect_tick - tick`, the input-to-effect delay in ticks, network included.
    pub delay_ticks: Option<u64>,
    /// The scenario tick the input reached the zone at under the run's
    /// network; `None` when the scenario ended first.
    pub arrived_tick: Option<u32>,
    /// The zone tick the input was made against, when it reported one.
    pub observed_tick: Option<u64>,
    /// The ticks of lateness the zone may make up for (#415).
    pub late_ticks: u8,
    /// How the zone arbitrated the command's graph input against the state
    /// it met on arrival (#383, Atlas 5.7.4), for commands that make one,
    /// with its lateness (#415).
    #[serde(rename = "decision", serialize_with = "decision_trace")]
    pub arbitration: Option<Arbitration>,
}

fn edge_traces<S: serde::Serializer>(
    edges: &[EdgeReport],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(edges.iter().map(edge_trace))
}

/// An edge report as the page shows it.
#[must_use]
pub fn edge_trace(report: &EdgeReport) -> serde_json::Value {
    serde_json::json!({
        "edge": name(report.edge),
        "status": name(report.status),
        "anchorTick": report.anchor_tick,
        "bufferFrom": report.buffer_from,
        "openFrom": report.open_from,
        "openUntil": report.open_until,
        "precision": report.precision.map(|(from, until)| [from, until]),
    })
}

#[expect(clippy::ref_option, reason = "serde's serialize_with signature")]
fn decision_trace<S: serde::Serializer>(
    arbitration: &Option<Arbitration>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    arbitration.as_ref().map(decision).serialize(serializer)
}

/// An arbitration as the page shows it.
#[must_use]
pub fn decision(arbitration: &Arbitration) -> serde_json::Value {
    let candidates: Vec<_> = arbitration
        .candidates
        .iter()
        .map(|(candidate, verdict)| {
            serde_json::json!({ "candidate": name(candidate), "verdict": name(verdict) })
        })
        .collect();
    serde_json::json!({
        "input": name(arbitration.input),
        "outcome": name(arbitration.outcome),
        "candidates": candidates,
        "precise": arbitration.precise,
        "late": arbitration.late,
        "compensated": arbitration.compensated,
    })
}

/// The graph input `command` makes for a player of `class`, as the zone
/// classifies intents.
fn graph_input(command: &ZoneCommand, class: PlayerClass) -> Option<ActionInput> {
    match *command {
        ZoneCommand::UseAbility { ability, .. } if ability == ids::SHIELD_BASH.get() => {
            Some(ActionInput::ShieldBash)
        }
        ZoneCommand::UseAbility { ability, .. }
            if [ids::HEROIC_STRIKE, ids::CLEAVE]
                .iter()
                .any(|id| id.get() == ability) =>
        {
            Some(ActionInput::WardenMelee)
        }
        ZoneCommand::UseAbility { .. } => Some(ActionInput::Ability),
        ZoneCommand::DirectedStrike { .. } => Some(ActionInput::WardenMelee),
        ZoneCommand::StartAttack if class == PlayerClass::Warden => Some(ActionInput::WardenMelee),
        ZoneCommand::Guard { lane: Some(_) } => Some(ActionInput::GuardProtection),
        ZoneCommand::Move {
            forward, strafe, ..
        } if forward != 0 || strafe != 0 => Some(ActionInput::Move),
        ZoneCommand::Rise(rise) if rise != 0 => Some(ActionInput::Move),
        ZoneCommand::Jump => Some(ActionInput::Jump),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub damage_dealt: u32,
    pub damage_taken: u32,
    pub refusals: u32,
    pub kills: u32,
    pub final_health: u32,
    pub final_resource: Option<u16>,
    pub final_digest: String,
}

/// A replay of a scenario under one configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub config: String,
    pub config_name: String,
    pub baseline: bool,
    pub content_revision: String,
    /// The scenario's content revision differs from this build's.
    pub content_changed: bool,
    /// The recording's final digest, when it was recorded under these rules.
    pub reproduces_recording: Option<bool>,
    pub ticks: Vec<TickRecord>,
    pub commands: Vec<CommandResult>,
    pub summary: Summary,
    /// Explained timing and transition diagnostics of this run (#383).
    pub findings: Vec<Finding>,
}

const fn is_action(command: &ZoneCommand) -> bool {
    matches!(
        command,
        ZoneCommand::StartAttack
            | ZoneCommand::UseAbility { .. }
            | ZoneCommand::DirectedStrike { .. }
            | ZoneCommand::Guard { .. }
            | ZoneCommand::Jump
    )
}

/// Whether `event` is an effect of `me`'s own action.
fn own_effect(event: &ZoneEvent, me: EntityRef) -> bool {
    match *event {
        ZoneEvent::AttackPerformed { source, .. }
        | ZoneEvent::DamageDealt { source, .. }
        | ZoneEvent::Miss { source, .. }
        | ZoneEvent::CastStarted { source, .. }
        | ZoneEvent::AbilityUsed { source, .. }
        | ZoneEvent::StrikeSwept { source, .. }
        | ZoneEvent::Healed { source, .. }
        | ZoneEvent::AuraApplied { source, .. } => source == me,
        _ => false,
    }
}

fn region_text(report: &action::RegionReport) -> String {
    match (report.action, report.phase) {
        (Some(action), Some(phase)) => format!("{action:?}·{phase:?}"),
        (Some(action), None) => format!("{action:?}"),
        _ => String::new(),
    }
}

/// Replays `scenario` under `config` in a fresh lab world, its inputs
/// crossing `network`.
pub fn run(scenario: &ScenarioFile, config: &Config, network: &Network) -> Result<Run, String> {
    let graph = config.graph()?;
    let identity = config.identity();
    let commands = scenario.commands()?;
    let seed = scenario.seed()?;
    let (mut host, player) = LocalZoneHost::action_lab(scenario.setup.setup()?, seed)
        .map_err(|error| error.to_string())?;
    host.set_action_graph(graph)
        .map_err(|error| format!("{error:?}"))?;
    let me = EntityRef::Player(player);
    let class = scenario.setup.setup()?.class;
    let mut ticks = Vec::with_capacity(scenario.ticks as usize);
    let mut results: Vec<CommandResult> = Vec::with_capacity(commands.len());
    let mut summary = Summary::default();
    let mut next = 0;
    let mut sequence = 0;
    // Inputs on their way, in order: arrival tick, result index, the zone
    // tick they were made against.
    let mut travelling = std::collections::VecDeque::new();
    let mut last_arrival = 0;
    // The action command still waiting for its first effect.
    let mut waiting: Option<usize> = None;
    for tick in 0..scenario.ticks {
        while let Some((at, _, command)) = commands.get(next).filter(|(at, ..)| *at == tick) {
            // In order: an input never overtakes an earlier one.
            last_arrival = (at + network.delay_ticks(seed, next)).max(last_arrival);
            travelling.push_back((last_arrival, results.len(), host.zone().current_tick()));
            results.push(CommandResult {
                index: next,
                tick: *at,
                label: format!("{command:?}"),
                refused: Vec::new(),
                effect_tick: None,
                delay_ticks: None,
                arrived_tick: None,
                observed_tick: None,
                late_ticks: 0,
                arbitration: None,
            });
            next += 1;
        }
        let mut delivered = Vec::new();
        while let Some(&(_, index, observed)) =
            travelling.front().filter(|(arrival, ..)| *arrival == tick)
        {
            travelling.pop_front();
            let (_, bytes, command) = &commands[results[index].index];
            // An input that did not travel stays the recorded bytes, so a
            // local run still reproduces its recording.
            let travelled = tick > results[index].tick;
            let observed =
                (network.compensate && travelled && action::command_input(command).is_some())
                    .then_some(observed);
            let late = observed.map_or(0, |observed| {
                u8::try_from(host.zone().current_tick() - observed)
                    .unwrap_or(u8::MAX)
                    .min(action::LATE_INPUT_GRACE_TICKS)
            });
            let result = &mut results[index];
            result.arrived_tick = Some(tick);
            result.observed_tick = observed;
            result.late_ticks = late;
            result.arbitration = graph_input(command, class)
                .and_then(|input| host.zone().arbitrate_late_input(player, input, late));
            let payload = match observed {
                Some(observed) => encode_observed_command(*command, observed),
                None => bytes.clone(),
            };
            sequence += 1;
            host.submit(player, sequence, &payload)
                .map_err(|error| error.to_string())?;
            if is_action(command) {
                waiting = Some(index);
            }
            delivered.push(index);
        }
        host.tick().map_err(|error| error.to_string())?;
        let projection = host
            .zone()
            .snapshot_for_player(player)
            .map_err(|error| error.to_string())?;
        let mut events = Vec::with_capacity(projection.events.len());
        for event in &projection.events {
            match *event {
                ZoneEvent::Error { code, .. } => {
                    summary.refusals += 1;
                    for &index in &delivered {
                        results[index].refused.push(name(code));
                    }
                }
                ZoneEvent::DamageDealt { source, amount, .. } if source == me => {
                    summary.damage_dealt += u32::from(amount);
                }
                ZoneEvent::DamageTaken { target, amount, .. } if target == me => {
                    summary.damage_taken += u32::from(amount);
                }
                ZoneEvent::Died {
                    killer: Some(killer),
                    ..
                } if killer == me => summary.kills += 1,
                _ => {}
            }
            if let Some(index) = waiting.filter(|_| own_effect(event, me)) {
                let result = &mut results[index];
                result.effect_tick = Some(projection.tick);
                result.delay_ticks = Some(projection.tick - u64::from(result.tick));
                waiting = None;
            }
            events.push(name(event));
        }
        let inspection = host.zone().action_inspection(player);
        let regions = inspection
            .as_ref()
            .map(|inspection| inspection.regions.iter().map(region_text).collect())
            .unwrap_or_default();
        let edges = inspection
            .map(|inspection| {
                inspection
                    .edges
                    .into_iter()
                    .filter(|edge| edge.status != EdgeStatus::Blocked(BlockReason::SourceInactive))
                    .collect()
            })
            .unwrap_or_default();
        let state = projection.viewer;
        summary.final_health = state.health;
        summary.final_resource = state.resource.map(|resource| resource.value);
        let snapshot = host.zone().snapshot().map_err(|error| error.to_string())?;
        let body = snapshot
            .players
            .iter()
            .find(|candidate| candidate.player_id == player)
            .ok_or("the lab character is missing")?;
        let hz = f32::from(mmorpg_core::TICK_HZ);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (position, speed) = (
            body.position.map(|axis| axis.round() as i32),
            (body.velocity[0].hypot(body.velocity[2]) * hz).round() as u32,
        );
        let bytes = encode_canonical_snapshot(&snapshot).map_err(|error| format!("{error:?}"))?;
        ticks.push(TickRecord {
            tick: projection.tick,
            regions,
            events,
            health: state.health,
            resource: summary.final_resource,
            position,
            speed,
            digest: format!("{:016x}", fnv1a(&bytes)),
            edges,
        });
    }
    summary.final_digest = ticks
        .last()
        .map(|tick| tick.digest.clone())
        .unwrap_or_default();
    let content_revision = host.content_revision().to_string();
    Ok(Run {
        config: identity.clone(),
        config_name: config.name.clone(),
        baseline: *graph == action::BASELINE,
        content_changed: content_revision != scenario.content_revision,
        content_revision,
        reproduces_recording: scenario
            .recorded
            .as_ref()
            .filter(|recorded| recorded.config == identity)
            .map(|recorded| recorded.digest == summary.final_digest),
        ticks,
        commands: results,
        summary,
        findings: Vec::new(),
    })
    .map(|mut run| {
        run.findings = lab_diagnostics::diagnose(&run, graph);
        run
    })
}

/// A tick where the two runs differ in what the lab shows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Difference {
    pub tick: u64,
    pub a: TickRecord,
    pub b: TickRecord,
}

/// The same scenario under two configurations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub a: Run,
    pub b: Run,
    /// The first tick whose canonical state differs; `None` when the runs agree.
    pub diverges_at: Option<u64>,
    /// Ticks whose regions, events, health or resource differ, at most 200.
    pub differences: Vec<Difference>,
    /// Combos that succeeded under A and fail under B (#383).
    pub regressions: Vec<Finding>,
}

/// Runs `scenario` under `a` and `b` across one `network` and reports where they diverge.
pub fn compare(
    scenario: &ScenarioFile,
    a: &Config,
    b: &Config,
    network: &Network,
) -> Result<Comparison, String> {
    let a = run(scenario, a, network)?;
    let b = run(scenario, b, network)?;
    let diverges_at = a
        .ticks
        .iter()
        .zip(&b.ticks)
        .find(|(left, right)| left.digest != right.digest)
        .map(|(left, _)| left.tick);
    let shown = |tick: &TickRecord| {
        (
            tick.regions.clone(),
            tick.events.clone(),
            tick.health,
            tick.resource,
            tick.position,
        )
    };
    let differences = a
        .ticks
        .iter()
        .zip(&b.ticks)
        .filter(|(left, right)| shown(left) != shown(right))
        .take(200)
        .map(|(left, right)| Difference {
            tick: left.tick,
            a: left.clone(),
            b: right.clone(),
        })
        .collect();
    let regressions = lab_diagnostics::regressions(&a, &b, diverges_at);
    Ok(Comparison {
        a,
        b,
        diverges_at,
        differences,
        regressions,
    })
}

/// A configuration's identity and validation: `{identity, baseline, error}`.
#[must_use]
pub fn check(json: &str) -> serde_json::Value {
    match Config::parse(json).and_then(|config| config.graph().map(|graph| (config, graph))) {
        Ok((config, graph)) => serde_json::json!({
            "identity": config.identity(),
            "baseline": *graph == action::BASELINE,
            "error": null,
        }),
        Err(error) => serde_json::json!({ "identity": null, "baseline": false, "error": error }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mmorpg_protocol::encode_command;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// A Warden at the wolves who swings and jumps.
    fn scenario(commands: &[(u32, ZoneCommand)]) -> ScenarioFile {
        ScenarioFile {
            format: SCENARIO_FORMAT.to_owned(),
            name: "test".to_owned(),
            setup: SetupDto {
                class_name: "warden".to_owned(),
                level: 6,
                gear: "standard".to_owned(),
                main_hand: 0,
                resource: Some(60),
            },
            seed: "0".to_owned(),
            content_revision: crate::host::hosted_content().revision().to_string(),
            protocol_id: PROTOCOL_ID,
            ticks: 90,
            commands: commands
                .iter()
                .map(|(tick, command)| CommandDto {
                    tick: *tick,
                    bytes: hex(&encode_command(*command)),
                })
                .collect(),
            recorded: None,
        }
    }

    #[test]
    fn the_baseline_round_trips_to_the_production_graph() {
        let config = Config::baseline();
        let json = serde_json::to_string(&config).unwrap();
        let parsed = Config::parse(&json).unwrap();
        assert_eq!(parsed, config);
        assert_eq!(*parsed.graph().unwrap(), action::BASELINE);
        assert_eq!(check(&json)["baseline"], true);
    }

    #[test]
    fn edited_configurations_are_validated_and_isolated() {
        let mut config = Config::baseline();
        config.name = "longer window".to_owned();
        config.edges[0].actionable_ms += 300;
        let graph = config.graph().unwrap();
        assert_ne!(*graph, action::BASELINE);
        assert_ne!(config.identity(), Config::baseline().identity());
        // The name is not part of the identity.
        let mut renamed = config.clone();
        renamed.name = "other".to_owned();
        assert_eq!(renamed.identity(), config.identity());
        // Core validation still applies: a buffer longer than the lead fails.
        let mut invalid = Config::baseline();
        invalid.edges[0].buffer_ms = invalid.edges[0].open_after_ms + 1_000;
        assert!(invalid.graph().is_err());
        assert!(Config::parse(r#"{"format":"x"}"#).is_err());
        // The production graph is untouched.
        assert_eq!(
            Config::of(&action::BASELINE, "Production baseline"),
            Config::baseline()
        );
    }

    #[test]
    fn a_scenario_replays_deterministically_and_compares() {
        let scenario = scenario(&[(2, ZoneCommand::Jump), (10, ZoneCommand::StartAttack)]);
        let baseline = Config::baseline();
        let first = run(&scenario, &baseline, &Network::LOCAL).unwrap();
        let second = run(&scenario, &baseline, &Network::LOCAL).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.ticks.len(), 90);
        assert!(first.baseline && !first.content_changed);
        assert_eq!(first.commands.len(), 2);
        assert!(first.commands[0].label.starts_with("Jump"));
        let same = compare(&scenario, &baseline, &baseline, &Network::LOCAL).unwrap();
        assert_eq!(same.diverges_at, None);
        assert!(same.differences.is_empty());
        // Removing the jump changes the canonical state from its tick on.
        let mut edited = scenario.clone();
        edited.commands.remove(0);
        let a = run(&scenario, &baseline, &Network::LOCAL).unwrap();
        let b = run(&edited, &baseline, &Network::LOCAL).unwrap();
        assert_ne!(a.summary.final_digest, b.summary.final_digest);
        // A recording reproduces under its own rules.
        let mut recorded = scenario.clone();
        recorded.recorded = Some(RecordedDto {
            config: baseline.identity(),
            digest: first.summary.final_digest.clone(),
        });
        assert_eq!(
            run(&recorded, &baseline, &Network::LOCAL)
                .unwrap()
                .reproduces_recording,
            Some(true)
        );
    }

    /// A sliding slash: Heroic Strike while moving forward (#414).
    fn sliding_slash() -> AuthoredActionDto {
        let phase = |ms, steering_percent, momentum_percent| AuthoredPhaseDto {
            ms,
            steering_percent,
            momentum_percent,
            max_acceleration_cms2: None,
        };
        AuthoredActionDto {
            slot: 0,
            name: "Sliding slash".to_owned(),
            class: "warden".to_owned(),
            input: "WardenMelee".to_owned(),
            ability: Some(mmorpg_core::ability::ids::HEROIC_STRIKE.get()),
            context: "Moving(Forward)".to_owned(),
            conditions: Vec::new(),
            startup: phase(100, 100, 0),
            active: phase(300, 0, 100),
            recovery: phase(200, 50, 0),
            effects: vec![EffectDto::Impulse {
                forward_cms: 400,
                up_cms: 0,
            }],
        }
    }

    #[test]
    fn an_authored_action_is_validated_replayed_and_compared() {
        let mut config = Config::baseline();
        config.name = "sliding slash".to_owned();
        config.authored.push(sliding_slash());
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(Config::parse(&json).unwrap(), config);
        let graph = config.graph().unwrap();
        assert_eq!(graph.authored.len(), 1);
        assert_eq!(Config::of(graph, "sliding slash"), config);
        assert_eq!(check(&json)["baseline"], false);
        // An unsupported primitive is named, not guessed.
        let mut jumping = config.clone();
        jumping.authored[0].input = "Jump".to_owned();
        jumping.authored[0].ability = None;
        let error = jumping.graph().unwrap_err();
        assert!(error.contains("BadAuthored(0, Input)"), "{error}");
        let mut unknown = config.clone();
        unknown.authored[0].context = "Crouching".to_owned();
        assert!(unknown.graph().is_err());

        let facing = crate::action_lab::LAB_LOCATION.1;
        let scenario = scenario(&[
            (
                2,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing,
                },
            ),
            (
                8,
                ZoneCommand::DirectedStrike {
                    ability: mmorpg_core::ability::ids::HEROIC_STRIKE.get(),
                    facing,
                    aim: mmorpg_core::StrikeAim::new(
                        mmorpg_core::StrikeSide::Centre,
                        mmorpg_core::StrikeHeight::Low,
                    ),
                },
            ),
        ]);
        let authored = run(&scenario, &config, &Network::LOCAL).unwrap();
        assert!(!authored.baseline);
        assert!(
            authored.ticks.iter().any(|tick| tick
                .regions
                .iter()
                .any(|region| region.contains("Authored(0)"))),
            "the replay shows the authored action running"
        );
        let comparison = compare(&scenario, &Config::baseline(), &config, &Network::LOCAL).unwrap();
        assert!(comparison.diverges_at.is_some());
        assert_eq!(run(&scenario, &config, &Network::LOCAL).unwrap(), authored);
    }

    #[test]
    fn a_long_authored_startup_is_not_reported_as_lag() {
        let mut config = Config::baseline();
        config.name = "slow sliding slash".to_owned();
        let mut slow = sliding_slash();
        slow.startup.ms = 200;
        config.authored.push(slow);
        let facing = crate::action_lab::LAB_LOCATION.1;
        let scenario = scenario(&[
            (
                2,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing,
                },
            ),
            (
                8,
                ZoneCommand::DirectedStrike {
                    ability: mmorpg_core::ability::ids::HEROIC_STRIKE.get(),
                    facing,
                    aim: mmorpg_core::StrikeAim::new(
                        mmorpg_core::StrikeSide::Centre,
                        mmorpg_core::StrikeHeight::Low,
                    ),
                },
            ),
        ]);
        let replay = run(&scenario, &config, &Network::LOCAL).unwrap();
        let strike = replay.commands.last().unwrap();
        // The use waited out the 12-tick startup: past the lag threshold.
        let delay = strike.delay_ticks.expect("the strike took effect");
        let threshold = 1 + u64::from(mmorpg_core::ticks_from_ms(lab_diagnostics::LAG_MS));
        assert!(delay > threshold, "{delay}");
        assert!(
            !replay
                .findings
                .iter()
                .any(|finding| finding.kind == lab_diagnostics::Kind::InputLag),
            "{:#?}",
            replay.findings
        );
    }

    #[test]
    fn scenarios_fail_closed() {
        let mut stale = scenario(&[]);
        stale.protocol_id ^= 1;
        assert!(ScenarioFile::parse(&serde_json::to_string(&stale).unwrap()).is_err());
        let mut garbled = scenario(&[]);
        garbled.commands.push(CommandDto {
            tick: 1,
            bytes: "zz".to_owned(),
        });
        assert!(run(&garbled, &Config::baseline(), &Network::LOCAL).is_err());
    }

    #[test]
    fn a_local_run_with_ability_use_reproduces_its_plain_recording() {
        let strike = ZoneCommand::UseAbility {
            ability: mmorpg_core::ability::ids::HEROIC_STRIKE.get(),
            target: None,
        };
        let mut scenario = scenario(&[(3, strike), (20, ZoneCommand::Jump), (40, strike)]);
        // The arena records plain commands: replay them directly.
        let (mut host, player) =
            LocalZoneHost::action_lab(scenario.setup.setup().unwrap(), 0).unwrap();
        let mut sequence = 0;
        for tick in 0..scenario.ticks {
            for (at, bytes, _) in scenario.commands().unwrap() {
                if at == tick {
                    sequence += 1;
                    host.submit(player, sequence, &bytes).unwrap();
                }
            }
            host.tick().unwrap();
        }
        let baseline = Config::baseline();
        scenario.recorded = Some(RecordedDto {
            config: baseline.identity(),
            digest: format!("{:016x}", digest(&host).unwrap()),
        });
        let local = run(&scenario, &baseline, &Network::LOCAL).unwrap();
        assert_eq!(local.reproduces_recording, Some(true));
        assert!(
            local
                .commands
                .iter()
                .all(|command| command.observed_tick.is_none())
        );
    }

    #[test]
    fn a_simulated_network_delays_inputs_in_order_and_reports_their_lateness() {
        let strike = ZoneCommand::UseAbility {
            ability: mmorpg_core::ability::ids::HEROIC_STRIKE.get(),
            target: None,
        };
        let scenario = scenario(&[(2, ZoneCommand::Jump), (3, strike), (40, strike)]);
        let baseline = Config::baseline();
        let local = run(&scenario, &baseline, &Network::LOCAL).unwrap();
        assert!(local.commands.iter().all(|command| {
            command.arrived_tick == Some(command.tick) && command.late_ticks == 0
        }));
        assert!(
            local.commands[1].arbitration.is_some(),
            "a melee input is arbitrated"
        );
        assert!(local.commands[0].arbitration.is_some(), "so is a jump");

        let network = Network {
            round_trip_ms: 100,
            jitter_ms: 50,
            compensate: true,
        };
        let delayed = run(&scenario, &baseline, &network).unwrap();
        assert_eq!(
            delayed,
            run(&scenario, &baseline, &network).unwrap(),
            "deterministic"
        );
        let rtt = u32::from(mmorpg_core::ticks_from_ms(100));
        let jitter = u32::from(mmorpg_core::ticks_from_ms(50));
        let mut previous = 0;
        for command in &delayed.commands {
            let arrived = command.arrived_tick.unwrap();
            assert!(arrived >= previous, "inputs stay in order");
            assert!(arrived >= command.tick + rtt);
            assert!(arrived <= (command.tick + rtt + jitter).max(previous));
            previous = arrived;
        }
        // Only arbitrated inputs report their observation; the zone caps their lateness.
        assert_eq!(delayed.commands[0].observed_tick, None);
        let strike = &delayed.commands[1];
        assert_eq!(strike.observed_tick, Some(u64::from(strike.tick)));
        assert_eq!(
            u32::from(strike.late_ticks),
            (strike.arrived_tick.unwrap() - strike.tick)
                .min(u32::from(action::LATE_INPUT_GRACE_TICKS))
        );
        // Without compensation the same inputs arrive plain.
        let plain = Network {
            compensate: false,
            ..network
        };
        let uncompensated = run(&scenario, &baseline, &plain).unwrap();
        assert!(
            uncompensated
                .commands
                .iter()
                .all(|command| command.late_ticks == 0 && command.observed_tick.is_none())
        );
        assert!(
            Network::parse(r#"{"roundTripMs":1900,"jitterMs":200,"compensate":true}"#).is_err()
        );
        assert_eq!(
            Network::parse(r#"{"roundTripMs":0,"jitterMs":0,"compensate":true}"#).unwrap(),
            Network::LOCAL
        );
    }
}
