//! Physical projectiles (#286): arrows fly, stick in walls, track targets, and a raised
//! Warden shield stops a crossbow bolt.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    ABILITY_CATALOG_REVISION, CreatureId, EntityRef, Flight, ProjectileKind, SpawnGrid,
    StaticCollider, StrikeHeight, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneEvent,
    ZoneId, ZoneSimulation,
};
use support::arena::{self, NORTHWARD, WOLF};
use support::classes::{self, BANDIT};

const WOLF_ONE: EntityRef = EntityRef::Creature(CreatureId::new(1));

/// Flat ground, optionally a 10 m wide, 4 m tall wall 10 m north of the origin.
fn content(wall: bool, spawns: Vec<mmorpg_core::CreatureSpawn>) -> Arc<ZoneContent> {
    let mut colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    if wall {
        colliders.push(StaticCollider {
            id: 2,
            position: [0, 200, 1_000],
            half_extents: [500, 200, 50],
        });
    }
    let definition = ZoneDefinition::with_spawn_grid(
        77,
        [0, -1, 0],
        SpawnGrid {
            origin: [-5_000, -5_000],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    let content = ZoneContent::new(
        definition,
        ZoneAreas::default(),
        vec![arena::wolf(400, [1, 1]), classes::bandit(400, [1, 1])],
        spawns,
        Vec::new(),
        arena::GRAVEYARD,
    )
    .unwrap();
    Arc::new(
        content
            .with_creature_abilities(ABILITY_CATALOG_REVISION, vec![(BANDIT, ids::CROSSBOW_BOLT)])
            .unwrap(),
    )
}

struct Shooter {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<ZoneEvent>,
}

impl Shooter {
    fn new(content: Arc<ZoneContent>, class: u8) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let zone = classes::at_level(zone, 1, 6);
        // Stand at the origin, facing the creatures to the north.
        let mut state = zone.snapshot().unwrap();
        state.players[0].position = [0.0, 90.0, 0.0];
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut shooter = Self {
            zone,
            sequence: 0,
            events: Vec::new(),
        };
        shooter.send(ZoneCommand::ChooseClass { class, sex: 0 });
        shooter.send(arena::stand(NORTHWARD));
        shooter.tick();
        shooter
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

    fn damage_to(&self, unit: EntityRef) -> u32 {
        self.events
            .iter()
            .filter_map(|event| match *event {
                ZoneEvent::DamageDealt { target, amount, .. } if target == unit => {
                    Some(u32::from(amount))
                }
                _ => None,
            })
            .sum()
    }

    fn flights(&self) -> Vec<Flight> {
        self.zone
            .snapshot()
            .unwrap()
            .projectiles
            .iter()
            .map(|projectile| projectile.flight)
            .collect()
    }
}

#[test]
fn a_free_aimed_shot_flies_along_the_facing_and_hits_what_it_meets() {
    let mut ranger = Shooter::new(content(false, vec![arena::spawn(1, WOLF, [0, 1_500])]), 1);
    ranger.send(ZoneCommand::UseAbility {
        ability: ids::AIMED_SHOT.get(),
        target: None,
    });
    ranger.tick();
    let view = ranger.zone.snapshot_for_player(1).unwrap();
    assert_eq!(view.projectiles.len(), 1);
    assert_eq!(view.projectiles[0].kind, ProjectileKind::Arrow);
    assert!(!view.projectiles[0].stuck);
    for _ in 0..20 {
        ranger.tick();
    }
    assert!(ranger.damage_to(WOLF_ONE) > 0, "the arrow struck the wolf");
    assert!(matches!(
        ranger.flights()[..],
        [Flight::Stuck {
            unit: Some(WOLF_ONE),
            ..
        }]
    ));
}

#[test]
fn a_wall_stops_a_free_shot_and_holds_the_arrow() {
    let mut ranger = Shooter::new(content(true, vec![arena::spawn(1, WOLF, [0, 1_500])]), 1);
    ranger.send(ZoneCommand::UseAbility {
        ability: ids::AIMED_SHOT.get(),
        target: None,
    });
    for _ in 0..20 {
        ranger.tick();
    }
    assert_eq!(ranger.damage_to(WOLF_ONE), 0, "the wall took the arrow");
    let projectile = ranger.zone.snapshot().unwrap().projectiles[0];
    assert!(matches!(
        projectile.flight,
        Flight::Stuck { unit: None, .. }
    ));
    assert!(
        (projectile.position[2] - 950.0).abs() < 1.0,
        "{projectile:?}"
    );
    // It stays a while, then goes.
    for _ in 0..mmorpg_core::STUCK_TICKS {
        ranger.tick();
    }
    assert!(ranger.flights().is_empty());
}

#[test]
fn a_raised_shield_stops_a_crossbow_bolt() {
    let bandit = EntityRef::Creature(CreatureId::new(1));
    let mut warden = Shooter::new(content(false, vec![arena::spawn(1, BANDIT, [0, 450])]), 0);
    warden.send(ZoneCommand::Guard {
        lane: Some(StrikeHeight::Middle),
    });
    let mut bolts = 0;
    for _ in 0..90 {
        warden.tick();
        bolts += warden
            .zone
            .snapshot()
            .unwrap()
            .projectiles
            .iter()
            .filter(|projectile| projectile.kind == ProjectileKind::Bolt && projectile.age == 1)
            .count();
    }
    assert!(bolts > 0, "the crossbowman shot");
    assert!(
        warden.events.iter().any(|event| matches!(
            event,
            ZoneEvent::Blocked { source, amount: 0, .. } if *source == bandit
        )),
        "the shield took the bolt: {:?}",
        warden.events
    );
}
