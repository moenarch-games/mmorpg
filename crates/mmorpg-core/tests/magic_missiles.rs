//! Magic missiles (#289): Firebolt carries a missile that ignores gravity. Cast at a target it
//! homes on it and follows it; cast without a target it flies straight along the facing, so it
//! hits what stands in that line, misses what does not, and a wall in the way stops it.
mod support;

/// [`t30`] as a `usize` tick count.
fn t30u(ticks: u64) -> usize {
    usize::try_from(mmorpg_core::golden::t30(ticks)).unwrap()
}

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureId, EntityRef, ProjectileKind, SpawnGrid, StaticCollider, ZoneAreas, ZoneCommand,
    ZoneContent, ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, NORTHWARD, WOLF};

const ARCANIST: u8 = 2;
/// A quarter turn: facing +X.
const EASTWARD: u16 = 16_384;

fn creature(id: u32) -> EntityRef {
    EntityRef::Creature(CreatureId::new(id))
}

/// Flat ground plus `walls`, with sturdy neutral boars at `spawns`.
fn content(walls: Vec<StaticCollider>, spawns: &[[i32; 2]]) -> Arc<ZoneContent> {
    let mut colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    colliders.extend(walls);
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [-5_000, -5_000],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    let spawns = spawns
        .iter()
        .zip(1..)
        .map(|(&at, id)| arena::spawn(id, WOLF, at))
        .collect();
    Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![mmorpg_core::CreatureTemplate {
                behaviour: mmorpg_core::CreatureBehaviour::Neutral,
                ..arena::wolf(2_000, [1, 1])
            }],
            spawns,
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Vec::new())
        .unwrap(),
    )
}

struct Caster {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<(u64, ZoneEvent)>,
}

impl Caster {
    fn new(content: Arc<ZoneContent>, facing: u16) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let zone = support::classes::at_level(zone, 1, 6);
        let mut state = zone.snapshot().unwrap();
        state.players[0].position = [0.0, 90.0, 0.0];
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut caster = Self {
            zone,
            sequence: 0,
            events: Vec::new(),
        };
        caster.send(ZoneCommand::ChooseClass {
            class: ARCANIST,
            sex: 0,
        });
        caster.send(arena::stand(facing));
        caster.tick();
        caster
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn tick(&mut self) {
        self.zone.advance_tick().unwrap();
        let now = self.zone.current_tick();
        let events = self.zone.snapshot_for_player(1).unwrap().events;
        self.events
            .extend(events.into_iter().map(|event| (now, event)));
    }

    fn ticks(&mut self, count: usize) {
        for _ in 0..count {
            self.tick();
        }
    }

    fn firebolt(&mut self, target: Option<EntityRef>) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::FIREBOLT.get(),
            target,
        });
    }

    /// Damage the player dealt to `unit`, with the tick it landed.
    fn hits(&self, unit: EntityRef) -> Vec<u64> {
        self.events
            .iter()
            .filter(|(_, event)| {
                matches!(event, ZoneEvent::DamageDealt { target, .. } if *target == unit)
            })
            .map(|(tick, _)| *tick)
            .collect()
    }

    fn missiles(&self) -> Vec<mmorpg_core::Projectile> {
        self.zone
            .snapshot()
            .unwrap()
            .projectiles
            .into_iter()
            .filter(|projectile| projectile.kind == ProjectileKind::Missile)
            .collect()
    }

    fn creature_at(&self, id: u32) -> [f32; 3] {
        let state = self.zone.snapshot().unwrap();
        state.creatures[usize::try_from(id - 1).unwrap()].position
    }
}

#[test]
fn a_targeted_firebolt_homes_level_and_lands_after_its_flight() {
    let mut arcanist = Caster::new(content(Vec::new(), &[[0, 2_000]]), NORTHWARD);
    arcanist.firebolt(Some(creature(1)));
    arcanist.ticks(t30u(60));
    let missiles = arcanist.missiles();
    assert_eq!(missiles.len(), 1, "the cast released a missile");
    let missile = missiles[0];
    // Straight at the boar's centre, 30 m/s, no drop.
    let speed = missile.velocity.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!(
        (speed - mmorpg_core::MISSILE_SPEED_UNITS).abs() < 0.01,
        "speed {speed}"
    );
    assert!(missile.velocity[1].abs() < 5.0, "{:?}", missile.velocity);
    assert!(arcanist.hits(creature(1)).is_empty(), "not yet");
    arcanist.ticks(t30u(30));
    let hits = arcanist.hits(creature(1));
    assert_eq!(hits.len(), 1, "{:?}", arcanist.events);
    assert!(arcanist.missiles().iter().all(|missile| missile.is_stuck()));
}

#[test]
fn a_targeted_firebolt_follows_a_moving_target() {
    let mut arcanist = Caster::new(content(Vec::new(), &[[0, 2_500]]), NORTHWARD);
    arcanist.firebolt(Some(creature(1)));
    arcanist.ticks(t30u(58));
    // The boar is pushed 6 m east just as the missile leaves.
    let mut state = arcanist.zone.snapshot().unwrap();
    state.creatures[0].position[0] += 600.0;
    arcanist.zone =
        ZoneSimulation::from_snapshot(state, Arc::clone(arcanist.zone.content())).unwrap();
    arcanist.ticks(t30u(40));
    assert_eq!(arcanist.hits(creature(1)).len(), 1, "it homed on the boar");
}

#[test]
fn a_free_firebolt_flies_straight_hits_what_is_in_line_and_misses_the_rest() {
    // Boar 1 straight east, boar 2 north-east, off the line.
    let mut arcanist = Caster::new(content(Vec::new(), &[[2_000, 0], [1_500, 1_500]]), EASTWARD);
    arcanist.firebolt(None);
    arcanist.ticks(t30u(60));
    let missile = arcanist.missiles()[0];
    assert!(
        missile.velocity[0] > mmorpg_core::MISSILE_SPEED_UNITS - 1.0 && missile.velocity[1] == 0.0
    );
    arcanist.ticks(t30u(40));
    assert_eq!(arcanist.hits(creature(1)).len(), 1);
    assert!(arcanist.hits(creature(2)).is_empty());

    // Turned toward where boar 2 stood, but it stepped aside meanwhile: the missile misses.
    let mut arcanist = Caster::new(content(Vec::new(), &[[1_500, 1_500]]), 8_192);
    arcanist.firebolt(None);
    arcanist.ticks(t30u(58));
    let mut state = arcanist.zone.snapshot().unwrap();
    state.creatures[0].position[0] += 300.0;
    state.creatures[0].position[2] -= 300.0;
    arcanist.zone =
        ZoneSimulation::from_snapshot(state, Arc::clone(arcanist.zone.content())).unwrap();
    arcanist.ticks(t30u(80));
    assert!(arcanist.hits(creature(1)).is_empty(), "dodged");
    assert!(arcanist.missiles().is_empty(), "it flew on and vanished");
}

#[test]
fn a_wall_stops_a_free_firebolt() {
    let wall = StaticCollider {
        id: 2,
        position: [1_000, 200, 0],
        half_extents: [30, 300, 500],
    };
    let mut arcanist = Caster::new(content(vec![wall], &[[2_000, 0]]), EASTWARD);
    arcanist.firebolt(None);
    arcanist.ticks(t30u(100));
    assert!(arcanist.hits(creature(1)).is_empty());
    let missiles = arcanist.missiles();
    assert_eq!(missiles.len(), 1);
    assert!(missiles[0].is_stuck());
    assert!(
        (missiles[0].position[0] - 970.0).abs() < 0.5,
        "stuck in the wall's face: {:?}",
        missiles[0].position
    );
    assert_eq!(arcanist.creature_at(1)[0], 2_000.0);
}

#[test]
fn a_restored_zone_continues_a_missile_exactly() {
    let mut arcanist = Caster::new(content(Vec::new(), &[[0, 2_500]]), NORTHWARD);
    arcanist.firebolt(Some(creature(1)));
    arcanist.ticks(t30u(65));
    assert_eq!(arcanist.missiles().len(), 1);
    let mut restored = ZoneSimulation::from_snapshot(
        arcanist.zone.snapshot().unwrap(),
        Arc::clone(arcanist.zone.content()),
    )
    .unwrap();
    for _ in 0..t30u(40) {
        arcanist.zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(
            arcanist.zone.snapshot().unwrap(),
            restored.snapshot().unwrap()
        );
    }
}
