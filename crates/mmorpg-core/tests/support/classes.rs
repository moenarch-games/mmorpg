//! Class-ability fixtures shared by core and protocol tests: an arena whose
//! content binds Muck Bolt to lurkers and Crude Bandage to bandits, and a
//! way to start a player at a given level for scripted class fights.
#![allow(dead_code)]

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::unit::player_max_health;
use mmorpg_core::{
    ABILITY_CATALOG_REVISION, CreatureBehaviour, CreatureFamily, CreatureSpawn, CreatureTemplate,
    CreatureTemplateId, ZoneContent, ZoneSimulation,
};

use super::arena;

pub const LURKER: CreatureTemplateId = CreatureTemplateId::new(5);
pub const BANDIT: CreatureTemplateId = CreatureTemplateId::new(6);

/// A level-3 Mirefin Lurker with the given health and melee damage.
pub fn lurker(health: u32, damage: [u16; 2]) -> CreatureTemplate {
    CreatureTemplate {
        id: LURKER,
        name: "Test Lurker".into(),
        family: CreatureFamily::Mirefin,
        min_level: 3,
        max_level: 3,
        ..arena::wolf(health, damage)
    }
}

/// A level-3 Redbrand Bandit with the given health and melee damage.
pub fn bandit(health: u32, damage: [u16; 2]) -> CreatureTemplate {
    CreatureTemplate {
        id: BANDIT,
        name: "Test Bandit".into(),
        family: CreatureFamily::Redbrand,
        behaviour: CreatureBehaviour::Aggressive,
        min_level: 3,
        max_level: 3,
        ..arena::wolf(health, damage)
    }
}

/// An arena whose content binds the creature abilities of revision 6.
pub fn arena(templates: Vec<CreatureTemplate>, spawns: Vec<CreatureSpawn>) -> Arc<ZoneContent> {
    let content = Arc::unwrap_or_clone(arena::arena(templates, spawns, Vec::new()));
    let bindings = [(LURKER, ids::MUCK_BOLT), (BANDIT, ids::CRUDE_BANDAGE)]
        .into_iter()
        .filter(|(template, _)| content.creature_template(*template).is_some())
        .collect();
    Arc::new(
        content
            .with_creature_abilities(ABILITY_CATALOG_REVISION, bindings)
            .expect("test abilities are valid"),
    )
}

/// The same zone with `player` at `level`, full health and no XP, restored
/// through its canonical snapshot (tests only; levels are earned in play).
pub fn at_level(zone: ZoneSimulation, player: u32, level: u8) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    let record = state
        .players
        .iter_mut()
        .find(|record| record.player_id == player)
        .unwrap();
    record.combat.level = level;
    record.combat.experience = 0;
    record.combat.health = player_max_health(level);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}
