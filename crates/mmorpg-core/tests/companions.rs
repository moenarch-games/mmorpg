//! The Druid's pet companion (#296, Atlas 5.2.85) through the public zone API: one persistent
//! wolf that follows, fights on orders for its owner's kill, outlives its owner's death,
//! recovers exactly and leaves with its owner.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::golden::t30;
use mmorpg_core::{
    CreatureId, EntityKind, EntityRef, MinionOrder, ZoneCommand, ZoneContent, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::arena::{self, WOLF};
use support::classes;

const WOLF_UNIT: EntityRef = EntityRef::Creature(CreatureId::new(1));

struct Druid {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<ZoneEvent>,
}

impl Druid {
    /// A level-6 Druid at the origin facing north.
    fn new(content: Arc<ZoneContent>) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let mut druid = Self {
            zone: classes::at_level(zone, 1, 6),
            sequence: 0,
            events: Vec::new(),
        };
        druid.send(ZoneCommand::ChooseClass { class: 6, sex: 1 });
        druid.tick();
        druid
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

    fn ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Casts Call Companion and waits out its cast and cooldown.
    fn call(&mut self) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::CALL_COMPANION.get(),
            target: None,
        });
        self.ticks(t30(330));
    }

    fn pets(&self) -> Vec<mmorpg_core::CanonicalMinion> {
        self.zone.snapshot().unwrap().minions
    }

    /// The projected companion's XZ position.
    fn seen(&self) -> Vec<[i32; 2]> {
        self.zone
            .snapshot_for_player(1)
            .unwrap()
            .entities
            .iter()
            .filter(|entity| {
                entity.kind == EntityKind::Minion
                    && entity.appearance == u16::from(ids::CALL_COMPANION.get())
            })
            .map(|entity| [entity.position[0], entity.position[2]])
            .collect()
    }
}

fn distance([x, z]: [i32; 2]) -> f64 {
    f64::from(x).hypot(f64::from(z))
}

#[test]
fn one_companion_stays_untimed_behind_its_owner() {
    let content = classes::arena(
        vec![arena::wolf(400, [1, 1])],
        vec![arena::spawn(1, WOLF, [0, 6_000])],
    );
    let mut druid = Druid::new(content);
    druid.call();
    druid.call();
    let pets = druid.pets();
    assert_eq!(pets.len(), 1, "one companion per owner");
    assert_eq!(pets[0].minion.remaining, 0, "it is not timed");
    // Far longer than a skeleton's minute: it stays, behind its owner.
    druid.ticks(t30(2_400));
    let [at] = druid.seen()[..] else {
        panic!("the companion stays");
    };
    assert!(at[1] < 0 && distance(at) < 350.0, "{at:?}");
}

#[test]
fn an_ordered_companion_kills_a_wolf_for_its_owner() {
    // A level-6 wolf 20 m north, beyond its aggro radius, that bites weakly.
    let wolf = mmorpg_core::CreatureTemplate {
        min_level: 6,
        max_level: 6,
        ..arena::wolf(120, [1, 1])
    };
    let content = classes::arena(vec![wolf], vec![arena::spawn(1, WOLF, [0, 2_000])]);
    let mut druid = Druid::new(content);
    druid.call();
    let experience = druid.zone.snapshot().unwrap().players[0].combat.experience;

    druid.send(ZoneCommand::CommandMinions(MinionOrder::Attack(WOLF_UNIT)));
    for _ in 0..t30(1_200) {
        druid.tick();
        if druid.zone.snapshot().unwrap().creatures[0].health == 0 {
            break;
        }
    }
    let snapshot = druid.zone.snapshot().unwrap();
    assert_eq!(
        snapshot.creatures[0].health, 0,
        "the companion killed the wolf"
    );
    assert_eq!(snapshot.creatures[0].tapped_by, Some(1));
    assert!(
        snapshot.players[0].combat.experience > experience,
        "the owner earned the kill"
    );
}

#[test]
fn the_companion_outlives_its_owner_recovers_and_leaves_with_it() {
    // A level-6 wolf 20 m north that bites hard.
    let wolf = mmorpg_core::CreatureTemplate {
        min_level: 6,
        max_level: 6,
        ..arena::wolf(400, [20, 20])
    };
    let content = classes::arena(vec![wolf], vec![arena::spawn(1, WOLF, [0, 2_000])]);
    let mut druid = Druid::new(content);
    druid.call();

    // At 1 health the owner provokes the wolf with Wrath and falls; the companion fights on.
    let mut state = druid.zone.snapshot().unwrap();
    state.players[0].combat.health = 1;
    druid.zone = ZoneSimulation::from_snapshot(state, Arc::clone(druid.zone.content())).unwrap();
    druid.send(ZoneCommand::UseAbility {
        ability: ids::WRATH.get(),
        target: Some(WOLF_UNIT),
    });
    druid.ticks(t30(300));
    let snapshot = druid.zone.snapshot().unwrap();
    assert!(
        druid.events.iter().any(|event| matches!(
            event,
            ZoneEvent::Died {
                entity: EntityRef::Player(1),
                ..
            }
        )),
        "the wolf killed the owner"
    );
    assert_eq!(
        snapshot.minions.len(),
        1,
        "the companion outlived its owner"
    );
    assert!(
        snapshot.creatures[0]
            .threat
            .iter()
            .any(|entry| matches!(entry.entity, EntityRef::Minion(_))),
        "the wolf fights the companion"
    );

    // Recovery continues exactly, a dead owner's companion included.
    let mut restored =
        ZoneSimulation::from_snapshot(snapshot, Arc::clone(druid.zone.content())).unwrap();
    for _ in 0..t30(60) {
        druid.zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
    }
    assert_eq!(druid.zone.snapshot().unwrap(), restored.snapshot().unwrap());

    assert!(druid.zone.remove_player(1));
    assert!(druid.zone.snapshot().unwrap().minions.is_empty());
}
