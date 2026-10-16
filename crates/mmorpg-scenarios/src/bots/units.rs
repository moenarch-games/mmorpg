//! Scenario vocabulary for units and combat feedback: which unit a step or
//! expectation names, which event it looks for and which visible state it
//! checks. Parsed once at the file boundary; matching reads only decoded
//! projections, so the runner still owns no gameplay rule.

use std::fmt;

use mmorpg_core::{CreatureId, EntityRef, EntitySnapshot, ErrorCode, NpcId, ZoneEvent};
use serde::Deserialize;

/// A unit named in a scenario: `none`, `bot:<name>`, `creature:<id>` or `npc:<id>`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "String")]
pub enum UnitSpec {
    None,
    Bot(String),
    Creature(u32),
    Npc(u32),
}

impl TryFrom<String> for UnitSpec {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        if text == "none" {
            return Ok(Self::None);
        }
        let invalid =
            || format!("unit {text:?} must be none, bot:<name>, creature:<id> or npc:<id>");
        let (kind, value) = text.split_once(':').ok_or_else(invalid)?;
        let id = || value.parse::<u32>().map_err(|_| invalid());
        match kind {
            "bot" if !value.is_empty() => Ok(Self::Bot(value.to_owned())),
            "creature" => Ok(Self::Creature(id()?)),
            "npc" => Ok(Self::Npc(id()?)),
            _ => Err(invalid()),
        }
    }
}

impl fmt::Display for UnitSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Bot(name) => write!(formatter, "bot:{name}"),
            Self::Creature(id) => write!(formatter, "creature:{id}"),
            Self::Npc(id) => write!(formatter, "npc:{id}"),
        }
    }
}

impl UnitSpec {
    /// The unit this names; `player_of` maps a bot to its current player.
    /// `Err` carries the bot name when that bot has no session.
    pub fn resolve(
        &self,
        player_of: impl Fn(&str) -> Option<u32>,
    ) -> Result<Option<EntityRef>, String> {
        Ok(match self {
            Self::None => None,
            Self::Bot(name) => Some(EntityRef::Player(
                player_of(name).ok_or_else(|| name.clone())?,
            )),
            Self::Creature(id) => Some(EntityRef::Creature(CreatureId::new(*id))),
            Self::Npc(id) => Some(EntityRef::Npc(NpcId::new(*id))),
        })
    }
}

/// A feedback event kind: `damage_dealt`, `damage_taken`, `miss`, `died`,
/// `evade`, `cast_started`, `ability_used`, `healed`, `aura_applied`,
/// `aura_removed`, `interrupted`, `absorbed`, `quest_progress`,
/// `quest_completed` or `error:<code>`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "String")]
pub enum EventSpec {
    DamageDealt,
    DamageTaken,
    Miss,
    Died,
    Evade,
    CastStarted,
    AbilityUsed,
    Healed,
    AuraApplied,
    AuraRemoved,
    Interrupted,
    Absorbed,
    QuestProgress,
    QuestCompleted,
    Error(ErrorCode),
}

/// Event kinds without a payload, with their scenario names.
const PLAIN_EVENTS: [(EventSpec, &str); 14] = [
    (EventSpec::DamageDealt, "damage_dealt"),
    (EventSpec::DamageTaken, "damage_taken"),
    (EventSpec::Miss, "miss"),
    (EventSpec::Died, "died"),
    (EventSpec::Evade, "evade"),
    (EventSpec::CastStarted, "cast_started"),
    (EventSpec::AbilityUsed, "ability_used"),
    (EventSpec::Healed, "healed"),
    (EventSpec::AuraApplied, "aura_applied"),
    (EventSpec::AuraRemoved, "aura_removed"),
    (EventSpec::Interrupted, "interrupted"),
    (EventSpec::Absorbed, "absorbed"),
    (EventSpec::QuestProgress, "quest_progress"),
    (EventSpec::QuestCompleted, "quest_completed"),
];

impl TryFrom<String> for EventSpec {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        if let Some((spec, _)) = PLAIN_EVENTS.iter().find(|(_, name)| *name == text) {
            return Ok(*spec);
        }
        match text.strip_prefix("error:").and_then(parse_error_code) {
            Some(code) => Ok(Self::Error(code)),
            None => Err(format!(
                "event {text:?} must be damage_dealt, damage_taken, miss, died, evade, cast_started, ability_used, healed, aura_applied, aura_removed, interrupted, absorbed, quest_progress, quest_completed or error:<code>"
            )),
        }
    }
}

impl fmt::Display for EventSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error(code) => write!(formatter, "error:{}", error_code_name(*code)),
            plain => formatter.write_str(
                PLAIN_EVENTS
                    .iter()
                    .find(|(spec, _)| spec == plain)
                    .map_or("", |(_, name)| name),
            ),
        }
    }
}

impl EventSpec {
    /// Whether `event`, received by `viewer`, is of this kind and, when
    /// `unit` is given, concerns that unit.
    pub fn matches(self, event: &ZoneEvent, viewer: EntityRef, unit: Option<EntityRef>) -> bool {
        let kind = match (self, event) {
            (Self::DamageDealt, ZoneEvent::DamageDealt { .. })
            | (Self::DamageTaken, ZoneEvent::DamageTaken { .. })
            | (Self::Miss, ZoneEvent::Miss { .. })
            | (Self::Died, ZoneEvent::Died { .. })
            | (Self::Evade, ZoneEvent::Evade { .. })
            | (Self::CastStarted, ZoneEvent::CastStarted { .. })
            | (Self::AbilityUsed, ZoneEvent::AbilityUsed { .. })
            | (Self::Healed, ZoneEvent::Healed { .. })
            | (Self::AuraApplied, ZoneEvent::AuraApplied { .. })
            | (Self::AuraRemoved, ZoneEvent::AuraRemoved { .. })
            | (Self::Interrupted, ZoneEvent::Interrupted { .. })
            | (Self::Absorbed, ZoneEvent::Absorbed { .. })
            | (Self::QuestProgress, ZoneEvent::QuestProgress { .. })
            | (Self::QuestCompleted, ZoneEvent::QuestCompleted { .. }) => true,
            (Self::Error(expected), ZoneEvent::Error { code, .. }) => expected == *code,
            _ => false,
        };
        kind && unit.is_none_or(|unit| concerned_unit(event, viewer) == Some(unit))
    }
}

/// The unit an event is about, seen from its recipient: whom it hit or who
/// hit it, who died, who evaded, the target an error concerned, the other
/// party of a cast, ability, heal or shield, and the unit an aura or
/// interrupt affected; quest events concern no unit.
pub fn concerned_unit(event: &ZoneEvent, viewer: EntityRef) -> Option<EntityRef> {
    let other = |source: EntityRef, target: Option<EntityRef>| {
        if source == viewer {
            target.or(Some(source))
        } else {
            Some(source)
        }
    };
    match *event {
        ZoneEvent::DamageDealt { target, .. } | ZoneEvent::Evade { target, .. } => Some(target),
        ZoneEvent::DamageTaken { source, .. } => Some(source),
        ZoneEvent::Miss { source, target }
        | ZoneEvent::Healed { source, target, .. }
        | ZoneEvent::Absorbed { source, target, .. } => other(source, Some(target)),
        ZoneEvent::CastStarted { source, target, .. }
        | ZoneEvent::AbilityUsed { source, target, .. } => other(source, target),
        ZoneEvent::Died { entity, .. } => Some(entity),
        ZoneEvent::Error { target, .. } => target,
        ZoneEvent::AuraApplied { target, .. }
        | ZoneEvent::AuraRemoved { target, .. }
        | ZoneEvent::Interrupted { target, .. } => Some(target),
        ZoneEvent::QuestProgress { .. } | ZoneEvent::QuestCompleted { .. } => None,
    }
}

/// A presentation fact about a unit in a bot's projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UnitState {
    /// In the projection and not dead.
    Alive,
    /// In the projection and dead: a corpse or a dead player.
    Dead,
    /// Not in the projection.
    Absent,
    InCombat,
    Evading,
    TargetsViewer,
    TappedByOther,
}

impl UnitState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::Dead => "dead",
            Self::Absent => "absent",
            Self::InCombat => "in_combat",
            Self::Evading => "evading",
            Self::TargetsViewer => "targets_viewer",
            Self::TappedByOther => "tapped_by_other",
        }
    }

    /// Whether a unit's projected record (`None` when absent) shows this state.
    pub fn holds(self, record: Option<&EntitySnapshot>) -> bool {
        let Some(record) = record else {
            return matches!(self, Self::Absent);
        };
        let flags = record.flags;
        match self {
            Self::Alive => !flags.dead,
            Self::Dead => flags.dead,
            Self::Absent => false,
            Self::InCombat => flags.in_combat,
            Self::Evading => flags.evading,
            Self::TargetsViewer => flags.targets_viewer,
            Self::TappedByOther => flags.tapped_by_other,
        }
    }
}

/// Stable snake-case name of an error code.
pub const fn error_code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::NoTarget => "no_target",
        ErrorCode::OutOfRange => "out_of_range",
        ErrorCode::TargetDead => "target_dead",
        ErrorCode::NotAttackable => "not_attackable",
        ErrorCode::YouAreDead => "you_are_dead",
        ErrorCode::NotDead => "not_dead",
        ErrorCode::InvalidTarget => "invalid_target",
        ErrorCode::TooManyIntents => "too_many_intents",
        ErrorCode::InvalidInventoryMove => "invalid_inventory_move",
        ErrorCode::InvalidLoot => "invalid_loot",
        ErrorCode::NotLootOwner => "not_loot_owner",
        ErrorCode::EmptyLoot => "empty_loot",
        ErrorCode::MoneyOverflow => "money_overflow",
        ErrorCode::InventoryFull => "inventory_full",
        ErrorCode::NoClass => "no_class",
        ErrorCode::NotLearned => "not_learned",
        ErrorCode::NotReady => "not_ready",
        ErrorCode::NotEnoughResource => "not_enough_resource",
        ErrorCode::Stunned => "stunned",
        ErrorCode::AlreadyCasting => "already_casting",
        ErrorCode::InvalidClass => "invalid_class",
        ErrorCode::NotEquippable => "not_equippable",
        ErrorCode::InvalidVendor => "invalid_vendor",
        ErrorCode::NotEnoughMoney => "not_enough_money",
        ErrorCode::ChatThrottled => "chat_throttled",
        ErrorCode::InvalidQuest => "invalid_quest",
        ErrorCode::QuestLogFull => "quest_log_full",
        ErrorCode::QuestIncomplete => "quest_incomplete",
    }
}

fn parse_error_code(name: &str) -> Option<ErrorCode> {
    Some(match name {
        "no_target" => ErrorCode::NoTarget,
        "out_of_range" => ErrorCode::OutOfRange,
        "target_dead" => ErrorCode::TargetDead,
        "not_attackable" => ErrorCode::NotAttackable,
        "you_are_dead" => ErrorCode::YouAreDead,
        "not_dead" => ErrorCode::NotDead,
        "invalid_target" => ErrorCode::InvalidTarget,
        "too_many_intents" => ErrorCode::TooManyIntents,
        "invalid_inventory_move" => ErrorCode::InvalidInventoryMove,
        "invalid_loot" => ErrorCode::InvalidLoot,
        "not_loot_owner" => ErrorCode::NotLootOwner,
        "empty_loot" => ErrorCode::EmptyLoot,
        "money_overflow" => ErrorCode::MoneyOverflow,
        "inventory_full" => ErrorCode::InventoryFull,
        "no_class" => ErrorCode::NoClass,
        "not_learned" => ErrorCode::NotLearned,
        "not_ready" => ErrorCode::NotReady,
        "not_enough_resource" => ErrorCode::NotEnoughResource,
        "stunned" => ErrorCode::Stunned,
        "already_casting" => ErrorCode::AlreadyCasting,
        "invalid_class" => ErrorCode::InvalidClass,
        "not_equippable" => ErrorCode::NotEquippable,
        "invalid_vendor" => ErrorCode::InvalidVendor,
        "not_enough_money" => ErrorCode::NotEnoughMoney,
        "chat_throttled" => ErrorCode::ChatThrottled,
        "invalid_quest" => ErrorCode::InvalidQuest,
        "quest_log_full" => ErrorCode::QuestLogFull,
        "quest_incomplete" => ErrorCode::QuestIncomplete,
        _ => return None,
    })
}

/// A compact digest token for one event; `name` renders units.
pub fn event_token(event: &ZoneEvent, name: impl Fn(EntityRef) -> String) -> String {
    let critical = |critical: bool| if critical { "!" } else { "" };
    match *event {
        ZoneEvent::DamageDealt {
            target,
            amount,
            critical: crit,
            ..
        } => format!("dealt:{}:{amount}{}", name(target), critical(crit)),
        ZoneEvent::DamageTaken {
            source,
            amount,
            critical: crit,
            ..
        } => format!("taken:{}:{amount}{}", name(source), critical(crit)),
        ZoneEvent::Miss { source, target } => format!("miss:{}>{}", name(source), name(target)),
        ZoneEvent::Died { entity, .. } => format!("died:{}", name(entity)),
        ZoneEvent::Evade { target, .. } => format!("evade:{}", name(target)),
        ZoneEvent::Error { code, .. } => format!("error:{}", error_code_name(code)),
        ZoneEvent::CastStarted {
            source, ability, ..
        } => format!("cast:{}:{}", name(source), ability.get()),
        ZoneEvent::AbilityUsed {
            source, ability, ..
        } => format!("used:{}:{}", name(source), ability.get()),
        ZoneEvent::Healed { target, amount, .. } => format!("healed:{}:{amount}", name(target)),
        ZoneEvent::AuraApplied {
            target, ability, ..
        } => format!("aura+:{}:{}", name(target), ability.get()),
        ZoneEvent::AuraRemoved {
            target, ability, ..
        } => format!("aura-:{}:{}", name(target), ability.get()),
        ZoneEvent::Interrupted {
            target, ability, ..
        } => format!("interrupted:{}:{}", name(target), ability.get()),
        ZoneEvent::Absorbed { target, amount, .. } => format!("absorbed:{}:{amount}", name(target)),
        ZoneEvent::QuestProgress {
            quest,
            objective,
            count,
        } => format!("quest:{}:{objective}={count}", quest.get()),
        ZoneEvent::QuestCompleted { quest } => format!("quest_done:{}", quest.get()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWER: EntityRef = EntityRef::Player(1);
    const WOLF: EntityRef = EntityRef::Creature(CreatureId::new(108));

    #[test]
    fn unit_specs_round_trip_and_reject_malformed_names() {
        for text in ["none", "bot:alice", "creature:108", "npc:8"] {
            let spec = UnitSpec::try_from(text.to_owned()).unwrap();
            assert_eq!(spec.to_string(), text);
        }
        for text in ["", "bot:", "wolf:1", "creature:x", "creature:-1", "npc"] {
            assert!(UnitSpec::try_from(text.to_owned()).is_err(), "{text}");
        }
        let player_of = |name: &str| (name == "alice").then_some(4);
        assert_eq!(
            UnitSpec::Bot("alice".into()).resolve(player_of),
            Ok(Some(EntityRef::Player(4)))
        );
        assert_eq!(
            UnitSpec::Bot("bob".into()).resolve(player_of),
            Err("bob".into())
        );
        assert_eq!(UnitSpec::None.resolve(player_of), Ok(None));
    }

    #[test]
    fn event_specs_match_kind_and_the_concerned_unit() {
        for code in [
            ErrorCode::NoTarget,
            ErrorCode::OutOfRange,
            ErrorCode::TargetDead,
            ErrorCode::NotAttackable,
            ErrorCode::YouAreDead,
            ErrorCode::NotDead,
            ErrorCode::InvalidTarget,
            ErrorCode::TooManyIntents,
            ErrorCode::NoClass,
            ErrorCode::NotLearned,
            ErrorCode::NotReady,
            ErrorCode::NotEnoughResource,
            ErrorCode::Stunned,
            ErrorCode::AlreadyCasting,
            ErrorCode::InvalidClass,
            ErrorCode::InvalidQuest,
            ErrorCode::QuestLogFull,
            ErrorCode::QuestIncomplete,
        ] {
            let spec = EventSpec::Error(code);
            assert_eq!(EventSpec::try_from(spec.to_string()), Ok(spec));
        }
        for (spec, name) in PLAIN_EVENTS {
            assert_eq!(spec.to_string(), name);
            assert_eq!(EventSpec::try_from(name.to_owned()), Ok(spec));
        }
        assert!(EventSpec::try_from("error:bogus".to_owned()).is_err());
        let missed = ZoneEvent::Miss {
            source: WOLF,
            target: VIEWER,
        };
        assert!(EventSpec::Miss.matches(&missed, VIEWER, Some(WOLF)));
        assert!(!EventSpec::Miss.matches(&missed, VIEWER, Some(VIEWER)));
        assert!(!EventSpec::Died.matches(&missed, VIEWER, None));
        let died = ZoneEvent::Died {
            entity: WOLF,
            killer: Some(VIEWER),
        };
        assert!(EventSpec::Died.matches(&died, VIEWER, Some(WOLF)));
        let name = |entity: EntityRef| format!("{entity:?}");
        assert_eq!(
            event_token(
                &ZoneEvent::DamageDealt {
                    source: VIEWER,
                    target: WOLF,
                    amount: 7,
                    critical: true,
                },
                name
            ),
            "dealt:Creature(CreatureId(108)):7!"
        );
    }
}
