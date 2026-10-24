//! Redbrand Hollow's public outer cavern (#160): the Greyfang cave garrisoned by Redbrands,
//! the Redbrand Brute's shield and telegraphed Overhead Smash, the sealed threshold and
//! *Deeper into the Hollow*.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{
    self, cave::GREYFANG, ids as collider_ids, quests, units::templates,
};
use mmorpg_core::{
    CreatureBehaviour, CreatureId, CreatureTemplate, EntityRef, ItemId, QuestObjective,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation, item_template,
};
use support::arena::{self, NORTHWARD, WOLF};
use support::classes;

const EAST: u16 = 16_384;
const SOUTHWARD: u16 = 32_768;
const BRUTE: EntityRef = EntityRef::Creature(CreatureId::new(1));

#[test]
fn the_cavern_holds_brutes_and_level_4_to_5_redbrands_on_its_floor() {
    let content = greyhaven_vale::content();
    let brute = content
        .creature_template(templates::REDBRAND_BRUTE)
        .unwrap();
    let bandit = content
        .creature_template(templates::REDBRAND_BANDIT)
        .unwrap();
    assert_eq!([brute.min_level, brute.max_level], [5, 5]);
    // About twice a level 5 bandit's health, with heavier and slower blows.
    let ratio = f64::from(brute.max_health(5)) / f64::from(bandit.max_health(5));
    assert!((1.9..=2.1).contains(&ratio), "{ratio}");
    assert!(brute.swing_ticks > bandit.swing_ticks && !brute.is_large());
    assert_eq!(
        content.creature_ability(templates::REDBRAND_BRUTE),
        Some(ids::OVERHEAD_SMASH)
    );
    assert!(content.is_shielded(templates::REDBRAND_BRUTE));
    assert!(!content.is_shielded(templates::REDBRAND_BANDIT));

    let cavern: Vec<_> = content
        .creature_spawns()
        .iter()
        .filter(|spawn| GREYFANG.flattens(spawn.position[0], spawn.position[1]))
        .collect();
    let brutes = cavern
        .iter()
        .filter(|spawn| spawn.template == templates::REDBRAND_BRUTE)
        .count();
    assert_eq!(brutes, 2);
    let regulars: Vec<_> = cavern
        .iter()
        .filter(|spawn| spawn.template != templates::REDBRAND_BRUTE)
        .collect();
    assert_eq!(regulars.len(), 4);
    for spawn in regulars {
        assert!(matches!(
            spawn.template,
            templates::REDBRAND_BANDIT | templates::REDBRAND_CROSSBOWMAN
        ));
        assert_eq!(spawn.levels, Some([4, 5]));
    }

    // Every cavern Redbrand spawns at level 4 or 5.
    let zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    let state = zone.snapshot().unwrap();
    for spawn in &cavern {
        let creature = state
            .creatures
            .iter()
            .find(|creature| creature.creature_id == spawn.id)
            .unwrap();
        assert!((4..=5).contains(&creature.level), "{:?}", spawn.id);
    }
}

#[test]
fn the_sealed_threshold_stands_in_the_inner_chamber() {
    let content = greyhaven_vale::content();
    let threshold = content
        .definition()
        .colliders()
        .iter()
        .find(|collider| collider.id == collider_ids::CAVE_THRESHOLD)
        .expect("the threshold is a collider");
    let [x, _, z] = threshold.position;
    // Inside the inner chamber, against its east wall: nothing walks through it.
    assert!((7_200..8_000).contains(&x) && (-14_400..-13_200).contains(&z));
    assert!(GREYFANG.flattens(x, z));
    assert_eq!(threshold.position[1] - threshold.half_extents[1], 0);
}

#[test]
fn deeper_into_the_hollow_follows_return_to_greyhaven() {
    let content = greyhaven_vale::content();
    let quest = content.quest(quests::DEEPER_INTO_THE_HOLLOW).unwrap();
    assert_eq!(quest.name, "Deeper into the Hollow");
    assert_eq!(quest.giver, quests::MARSHAL);
    assert_eq!(quest.prerequisite, Some(quests::RETURN_TO_GREYHAVEN));
    assert_eq!(
        quest.objectives,
        vec![QuestObjective::Kill {
            template: templates::REDBRAND_BRUTE,
            count: 2
        }]
    );
    let choices: Vec<_> = quest
        .rewards
        .choices
        .iter()
        .map(|&item| item_template(item).unwrap().name)
        .collect();
    assert_eq!(
        choices,
        [
            "Brute-Breaker Mace",
            "Greyfang Scout Jerkin",
            "Hollow-Delver's Cowl"
        ]
    );
    for item in [12, 13, 14] {
        assert!(item_template(ItemId::new(item)).unwrap().slot.is_some());
    }
}

/// A neutral level 5 brute at (−20 m, 15 m), facing `facing`, clear of the spawn slots.
fn brute_arena(facing: u16, shielded: bool) -> Arc<ZoneContent> {
    let template = CreatureTemplate {
        name: "Test Brute".into(),
        behaviour: CreatureBehaviour::Neutral,
        min_level: 5,
        max_level: 5,
        swing_ticks: 300,
        half_extents: [38, 96, 38],
        ..arena::wolf(2_000, [1, 1])
    };
    let mut spawn = arena::spawn(1, WOLF, [-2_000, 1_500]);
    spawn.facing = facing;
    let content = Arc::unwrap_or_clone(arena::arena(vec![template], vec![spawn], Vec::new()))
        .with_creature_abilities(vec![(WOLF, ids::OVERHEAD_SMASH)])
        .unwrap();
    Arc::new(if shielded {
        content.with_shielded(vec![WOLF]).unwrap()
    } else {
        content
    })
}

struct Fight {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<ZoneEvent>,
}

impl Fight {
    fn new(content: Arc<ZoneContent>, class: u8, at: [f32; 2]) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let zone = classes::at_level(zone, 1, 6);
        let mut state = zone.snapshot().unwrap();
        state.players[0].position = [at[0], 90.0, at[1]];
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut fight = Self {
            zone,
            sequence: 0,
            events: Vec::new(),
        };
        fight.send(ZoneCommand::ChooseClass { class, sex: 0 });
        fight.send(arena::stand(NORTHWARD));
        fight.tick();
        fight
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) {
        self.zone.advance_tick().unwrap();
        let events = self.zone.snapshot_for_player(1).unwrap().events;
        self.events.extend(events);
    }

    fn ticks(&mut self, count: usize) {
        for _ in 0..count {
            self.tick();
        }
    }

    fn damage_to_brute(&self) -> u32 {
        self.events
            .iter()
            .filter_map(|event| match *event {
                ZoneEvent::DamageDealt { target, amount, .. } if target == BRUTE => {
                    Some(u32::from(amount))
                }
                _ => None,
            })
            .sum()
    }

    fn blocked(&self) -> bool {
        self.events
            .iter()
            .any(|event| matches!(event, ZoneEvent::Blocked { target, .. } if *target == BRUTE))
    }

    fn aimed_shot(&mut self) {
        self.send(ZoneCommand::SelectTarget(Some(BRUTE)));
        self.send(ZoneCommand::UseAbility {
            ability: ids::AIMED_SHOT.get(),
            target: Some(BRUTE),
        });
        self.ticks(30);
    }

    /// The smash hit the player: the brute's melee blows deal 1, the smash at least 16.
    fn struck(&self) -> bool {
        self.events.iter().any(|event| {
            matches!(event, ZoneEvent::DamageTaken { source, amount, .. }
                if *source == BRUTE && *amount >= 16)
        })
    }

    fn smashed(&self) -> bool {
        self.events.iter().any(|event| {
            matches!(event, ZoneEvent::AbilityUsed { ability, .. } if *ability == ids::OVERHEAD_SMASH)
        })
    }
}

#[test]
fn the_brutes_shield_stops_arrows_at_its_front_but_not_its_back() {
    // Facing the ranger at the origin: the arrow stops on the shield.
    let mut front = Fight::new(brute_arena(SOUTHWARD, true), 1, [-2_000.0, 0.0]);
    front.aimed_shot();
    assert!(front.blocked(), "the shield blocks");
    assert_eq!(front.damage_to_brute(), 0);
    // Turned away: the arrow lands.
    let mut back = Fight::new(brute_arena(NORTHWARD, true), 1, [-2_000.0, 0.0]);
    back.aimed_shot();
    assert!(!back.blocked());
    assert!(back.damage_to_brute() > 0);
    // Without a shield, the front takes it too.
    let mut bare = Fight::new(brute_arena(SOUTHWARD, false), 1, [-2_000.0, 0.0]);
    bare.aimed_shot();
    assert!(!bare.blocked() && bare.damage_to_brute() > 0);
}

/// A Warden 2 m south of the brute attacks it and waits for the smash's wind-up.
fn wind_up() -> Fight {
    let mut fight = Fight::new(brute_arena(SOUTHWARD, false), 0, [-2_000.0, 1_300.0]);
    fight.send(ZoneCommand::SelectTarget(Some(BRUTE)));
    fight.send(ZoneCommand::StartAttack);
    for _ in 0..200 {
        if fight.events.iter().any(|event| {
            matches!(event, ZoneEvent::CastStarted { ability, ticks: 60, .. } if *ability == ids::OVERHEAD_SMASH)
        }) {
            return fight;
        }
        fight.tick();
    }
    panic!("the brute never wound up its smash");
}

#[test]
fn standing_in_front_of_the_smash_hurts() {
    let mut fight = wind_up();
    for _ in 0..70 {
        if fight.smashed() {
            break;
        }
        fight.tick();
    }
    assert!(fight.smashed());
    assert!(fight.struck(), "the smash hits whoever stood in front");
}

#[test]
fn stepping_aside_during_the_wind_up_dodges_the_smash() {
    let mut fight = wind_up();
    // The brute keeps the facing it wound up with, so a sidestep leaves its arc.
    fight.send(arena::walk(EAST));
    for _ in 0..70 {
        if fight.smashed() {
            break;
        }
        fight.tick();
    }
    assert!(fight.smashed());
    assert!(!fight.struck(), "the sidestep left the arc");
}
