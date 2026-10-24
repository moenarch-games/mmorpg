//! The Action Lab sandbox (#381): a configurable ephemeral character in a
//! fresh local world, and JSON views of `mmorpg-core`'s action graph.
//!
//! The lab owns no rules. The character is built from the ordinary creation
//! path and validated catalog equipment (`Equipment::from_slots`), the world
//! is the hosted content, and the inspector reads the authoritative
//! [`ZoneSimulation::action_inspection`] and
//! [`ZoneSimulation::arbitrate_input`]. Nothing is stored.

use std::sync::Arc;

use mmorpg_core::action::{
    self, ActionInput, Anchor, BlockReason, Candidate, Condition, EdgeStatus, Outcome, Verdict,
};
use mmorpg_core::{
    CharacterRecord, ClassChoice, EQUIPMENT_SLOTS, EphemeralCharacterBuild, Equipment, ItemId,
    MAX_PLAYER_LEVEL, PlayerClass, PlayerId, Sex, ZoneContent, ZoneSimulation, item_template,
};
use serde_json::{Value, json};

use crate::host::LocalZoneError;
use crate::scenario::{fresh_record, restore_health};

/// The lab's training spot: the eastern edge of Wolfrun Woods, facing the
/// wolf pack (west), where every class has something to fight.
pub const LAB_LOCATION: ([i32; 2], u16) = ([-4_000, -2_200], 49_152);

/// Every input the graph arbitrates, for the inspector's verdict table.
const INPUTS: [ActionInput; 3] = [
    ActionInput::WardenMelee,
    ActionInput::ShieldBash,
    ActionInput::GuardProtection,
];

/// Which validated catalog gear the lab character wears.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gear {
    /// What character creation hands out.
    Starter,
    /// The class weapon plus the catalog's armor (`EphemeralCharacterBuild::standard`).
    Standard,
    /// Nothing equipped.
    None,
}

impl Gear {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "starter" => Some(Self::Starter),
            "standard" => Some(Self::Standard),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// A validated lab character setup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LabSetup {
    pub class: PlayerClass,
    pub level: u8,
    pub gear: Gear,
    /// A main-hand catalog item replacing the gear's; `None` keeps it.
    pub main_hand: Option<ItemId>,
    /// The class resource; `None` keeps the level's starting value. Clamped to its maximum.
    pub resource: Option<u16>,
}

impl LabSetup {
    /// Parses the page's choices. `main_hand` 0 keeps the gear's weapon.
    pub fn parse(
        class: &str,
        level: u8,
        gear: &str,
        main_hand: u16,
        resource: Option<u16>,
    ) -> Result<Self, String> {
        let class = PlayerClass::ALL
            .into_iter()
            .find(|candidate| candidate.name() == class)
            .ok_or_else(|| format!("unknown class {class:?}"))?;
        if !(1..=MAX_PLAYER_LEVEL).contains(&level) {
            return Err(format!("level {level} is outside 1..={MAX_PLAYER_LEVEL}"));
        }
        let gear = Gear::from_name(gear).ok_or_else(|| format!("unknown gear {gear:?}"))?;
        let main_hand = (main_hand != 0).then_some(ItemId::new(main_hand));
        Ok(Self {
            class,
            level,
            gear,
            main_hand,
            resource,
        })
    }

    const fn choice(self) -> ClassChoice {
        ClassChoice {
            class: self.class,
            sex: Sex::Female,
        }
    }

    /// The lab character's record at [`LAB_LOCATION`].
    pub fn record(self, content: &Arc<ZoneContent>) -> Result<CharacterRecord, LocalZoneError> {
        let invalid = |error| LocalZoneError::Zone(mmorpg_core::ZoneError::new(error));
        let mut record = fresh_record(
            content,
            self.choice(),
            self.level,
            &format!("action-lab-{}", self.class.name()),
            LAB_LOCATION,
        )?;
        let mut slots: [Option<ItemId>; EQUIPMENT_SLOTS] = match self.gear {
            Gear::Starter => *record.equipment.slots(),
            Gear::Standard => *EphemeralCharacterBuild::standard(self.choice())
                .map_err(|_| invalid("the standard gear is invalid"))?
                .equipment()
                .slots(),
            Gear::None => [None; EQUIPMENT_SLOTS],
        };
        if let Some(item) = self.main_hand {
            slots[0] = Some(item);
        }
        record.equipment = Equipment::from_slots(slots)
            .map_err(|_| invalid("that item is not a main-hand catalog item"))?;
        let resource = self.class.resource();
        record.resource = self.resource.map_or(resource.start(self.level), |value| {
            value.min(resource.max(self.level))
        });
        restore_health(&mut record);
        Ok(record)
    }
}

/// Main-hand catalog items, for the setup form: `[{id, name}]`.
#[must_use]
pub fn main_hand_items() -> Value {
    (1..=u16::MAX)
        .map(ItemId::new)
        .map_while(|id| item_template(id).map(|template| (id, template)))
        .filter(|(_, template)| template.slot == Some(mmorpg_core::EquipmentSlot::MainHand))
        .map(|(id, template)| json!({ "id": id.get(), "name": template.name }))
        .collect()
}

fn name<T: std::fmt::Debug>(value: T) -> String {
    format!("{value:?}")
}

fn ticks(ms: u32) -> u16 {
    mmorpg_core::ticks_from_ms(ms)
}

fn condition_name(condition: Condition) -> String {
    name(condition)
}

fn anchor_name(anchor: Anchor) -> String {
    name(anchor)
}

fn candidate_name(candidate: Candidate) -> String {
    match candidate {
        Candidate::Edge(id) => name(id),
        Candidate::Direct => "Direct".to_owned(),
    }
}

fn reason(reason: BlockReason) -> Value {
    match reason {
        BlockReason::Condition(condition) => {
            json!({ "kind": "Condition", "condition": condition_name(condition) })
        }
        other => json!({ "kind": name(other) }),
    }
}

fn status(status: EdgeStatus) -> Value {
    match status {
        EdgeStatus::Blocked(why) => json!({ "kind": "Blocked", "reason": reason(why) }),
        other => json!({ "kind": name(other) }),
    }
}

fn verdict(verdict: Verdict) -> Value {
    match verdict {
        Verdict::Won => json!({ "kind": "Won" }),
        Verdict::Holding => json!({ "kind": "Holding" }),
        Verdict::Outranked(by) => json!({ "kind": "Outranked", "by": candidate_name(by) }),
        Verdict::HeldBy(rule) => json!({ "kind": "HeldBy", "rule": name(rule) }),
        Verdict::Blocked(why) => json!({ "kind": "Blocked", "reason": reason(why) }),
    }
}

fn outcome(outcome: Outcome) -> Value {
    match outcome {
        Outcome::Proceed(candidate) => {
            json!({ "kind": "Proceed", "candidate": candidate_name(candidate) })
        }
        Outcome::WaitForRule(rule) => json!({ "kind": "WaitForRule", "rule": name(rule) }),
        Outcome::WaitForEdge(edge) => json!({ "kind": "WaitForEdge", "edge": name(edge) }),
    }
}

/// The authored graph: actions by region, edges with their windows in ms and
/// ticks, recovery modifiers and compatibility rules.
#[must_use]
pub fn graph_json() -> String {
    let actions: Vec<Value> = action::ACTIONS
        .iter()
        .map(|def| {
            json!({
                "id": name(def.id),
                "region": name(def.region),
                "recoveryMs": def.recovery_ms,
                "recoveryTicks": def.recovery_ms.map(ticks),
            })
        })
        .collect();
    let modifiers: Vec<Value> = action::RECOVERY_MODIFIERS
        .iter()
        .map(|modifier| {
            json!({
                "action": name(modifier.action),
                "condition": condition_name(modifier.condition),
                "recoveryMs": modifier.recovery_ms,
                "recoveryTicks": ticks(modifier.recovery_ms),
            })
        })
        .collect();
    let edges: Vec<Value> = action::EDGES
        .iter()
        .map(|def| {
            let window = def.window;
            json!({
                "id": name(def.id),
                "from": name(def.from),
                "input": name(def.input),
                "anchor": anchor_name(def.anchor),
                "openAfterMs": window.open_after_ms,
                "actionableMs": window.actionable_ms,
                "bufferMs": window.buffer_ms,
                "openAfterTicks": window.open_after_ticks(),
                "actionableTicks": window.actionable_ticks(),
                "bufferTicks": window.buffer_ticks(),
                "conditions": def.conditions.iter().copied().map(condition_name).collect::<Vec<_>>(),
                "effects": def.effects.iter().map(|effect| name(effect)).collect::<Vec<_>>(),
                "priority": def.priority,
            })
        })
        .collect();
    let rules: Vec<Value> = action::RULES
        .iter()
        .map(|rule| {
            json!({
                "id": name(rule.id),
                "during": name(rule.during),
                "input": name(rule.input),
                "conditions": rule.conditions.iter().copied().map(condition_name).collect::<Vec<_>>(),
                "interaction": name(rule.interaction),
            })
        })
        .collect();
    json!({
        "tickHz": mmorpg_core::TICK_HZ,
        "regions": ["Movement", "Offense", "Defense", "Reaction"],
        "actions": actions,
        "modifiers": modifiers,
        "edges": edges,
        "rules": rules,
    })
    .to_string()
}

/// One player's inspection after the last completed tick, with each input's
/// arbitration at the next tick; `None` for an unknown player.
#[must_use]
pub fn inspection_json(zone: &ZoneSimulation, player: PlayerId) -> Option<String> {
    let inspection = zone.action_inspection(player)?;
    let regions: Vec<Value> = inspection
        .regions
        .iter()
        .map(|report| {
            json!({
                "region": name(report.region),
                "action": report.action.map(name),
                "phase": report.phase.map(name),
                "recoveryLeft": report.recovery_left,
            })
        })
        .collect();
    let edges: Vec<Value> = inspection
        .edges
        .iter()
        .map(|report| {
            json!({
                "edge": name(report.edge),
                "priority": report.priority,
                "status": status(report.status),
                "anchorTick": report.anchor_tick,
                "bufferFrom": report.buffer_from,
                "openFrom": report.open_from,
                "openUntil": report.open_until,
            })
        })
        .collect();
    let rules: Vec<Value> = inspection
        .rules
        .iter()
        .map(|report| {
            json!({
                "rule": name(report.rule),
                "input": name(report.input),
                "interaction": name(report.interaction),
                "releasedAt": report.released_at,
            })
        })
        .collect();
    let arbitrations: Vec<Value> = INPUTS
        .into_iter()
        .filter_map(|input| zone.arbitrate_input(player, input))
        .map(|arbitration| {
            json!({
                "input": name(arbitration.input),
                "outcome": outcome(arbitration.outcome),
                "candidates": arbitration
                    .candidates
                    .iter()
                    .map(|&(candidate, v)| json!({ "candidate": candidate_name(candidate), "verdict": verdict(v) }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    Some(
        json!({
            "tick": inspection.tick,
            "regions": regions,
            "edges": edges,
            "rules": rules,
            "arbitrations": arbitrations,
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::LocalZoneHost;

    #[test]
    fn a_configured_lab_character_is_inspectable_every_tick() {
        let setup = LabSetup::parse("warden", 7, "standard", 0, Some(40)).unwrap();
        let (mut host, player) = LocalZoneHost::action_lab(setup, 0).unwrap();
        let id = mmorpg_core::CharacterId::new("lab".to_owned()).unwrap();
        let record = host.zone().capture_character(player, id).unwrap();
        assert_eq!(record.level, 7);
        assert_eq!(record.resource, 40);
        assert!(record.equipment.slots().iter().all(Option::is_some));
        host.tick().unwrap();
        let inspection: Value =
            serde_json::from_str(&inspection_json(host.zone(), player).unwrap()).unwrap();
        assert_eq!(inspection["tick"], 1);
        assert_eq!(inspection["regions"].as_array().unwrap().len(), 4);
        assert_eq!(inspection["arbitrations"].as_array().unwrap().len(), 3);
        assert_eq!(
            inspection["edges"][0]["status"]["reason"]["kind"],
            "SourceInactive"
        );
    }

    #[test]
    fn setups_fail_closed_and_the_graph_exports_its_windows() {
        assert!(LabSetup::parse("bard", 5, "starter", 0, None).is_err());
        assert!(LabSetup::parse("warden", 0, "starter", 0, None).is_err());
        assert!(LabSetup::parse("warden", 5, "gold", 0, None).is_err());
        // A chest piece is not a main-hand item.
        let chest = LabSetup::parse("warden", 5, "none", 7, None).unwrap();
        assert!(LocalZoneHost::action_lab(chest, 0).is_err());
        let graph: Value = serde_json::from_str(&graph_json()).unwrap();
        assert_eq!(graph["edges"][0]["openAfterTicks"], 6);
        assert!(main_hand_items().as_array().unwrap().len() >= 3);
    }
}
