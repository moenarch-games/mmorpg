//! Forced movement (#287): Shield Bash knocks back, Fire Blast hurls every creature it can see
//! away from its centre while walls shield those behind them, and a Warden's Charge rushes at its
//! target and knocks it back on arrival.
mod support;

/// [`mmorpg_core::golden::t30`] as a `usize` tick count.
fn t30u(ticks: u64) -> usize {
    usize::try_from(mmorpg_core::golden::t30(ticks)).unwrap()
}

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureId, EntityRef, SpawnGrid, StaticCollider, ZoneAreas, ZoneCommand, ZoneContent,
    ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, NORTHWARD, WOLF};

const WARDEN: u8 = 0;
const ARCANIST: u8 = 2;

fn creature(id: u32) -> EntityRef {
    EntityRef::Creature(CreatureId::new(id))
}

/// Flat ground plus `walls`, with sturdy wolves at `spawns`.
fn content(walls: Vec<StaticCollider>, spawns: &[[i32; 2]]) -> Arc<ZoneContent> {
    let mut colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    colliders.extend(walls);
    let definition = ZoneDefinition::with_spawn_grid(
        77,
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
            vec![arena::wolf(2_000, [1, 1])],
            spawns,
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(mmorpg_core::ABILITY_CATALOG_REVISION, Vec::new())
        .unwrap(),
    )
}

struct Caster {
    zone: ZoneSimulation,
    sequence: u32,
    events: Vec<ZoneEvent>,
}

impl Caster {
    fn new(content: Arc<ZoneContent>, class: u8) -> Self {
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
        caster.send(ZoneCommand::ChooseClass { class, sex: 0 });
        caster.send(arena::stand(NORTHWARD));
        caster.tick();
        // Enough rage or mana for every test ability.
        let mut state = caster.zone.snapshot().unwrap();
        state.players[0].combat.abilities.resource = 100;
        caster.zone =
            ZoneSimulation::from_snapshot(state, Arc::clone(caster.zone.content())).unwrap();
        caster
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

    fn cast(&mut self, ability: mmorpg_core::AbilityId, target: EntityRef) {
        self.send(ZoneCommand::UseAbility {
            ability: ability.get(),
            target: Some(target),
        });
    }

    fn refusals(&self) -> Vec<&ZoneEvent> {
        self.events
            .iter()
            .filter(|event| matches!(event, ZoneEvent::Error { .. }))
            .collect()
    }

    fn damaged(&self, unit: EntityRef) -> bool {
        self.events
            .iter()
            .any(|event| matches!(*event, ZoneEvent::DamageDealt { target, .. } if target == unit))
    }

    /// A creature's XZ body position.
    fn creature_at(&self, id: u32) -> [f32; 2] {
        let state = self.zone.snapshot().unwrap();
        let creature = state
            .creatures
            .iter()
            .find(|creature| creature.creature_id == CreatureId::new(id))
            .unwrap();
        [creature.position[0], creature.position[2]]
    }

    fn player_at(&self) -> [f32; 2] {
        let state = self.zone.snapshot().unwrap();
        [state.players[0].position[0], state.players[0].position[2]]
    }
}

#[test]
fn shield_bash_knocks_its_target_back_once_and_grants_immunity() {
    let mut warden = Caster::new(content(Vec::new(), &[[0, 200]]), WARDEN);
    let before = warden.creature_at(1)[1];
    warden.cast(ids::SHIELD_BASH, creature(1));
    warden.ticks(t30u(12));
    let after = warden.creature_at(1)[1];
    assert!(
        after - before > 200.0,
        "knocked back from {before} to {after}: {:?}",
        warden.refusals()
    );
    let forced = warden.zone.snapshot().unwrap().forced;
    assert!(
        matches!(forced[..], [(unit, state)] if unit == creature(1) && state.moving.is_none() && state.immune > 0),
        "{forced:?}"
    );
}

#[test]
fn a_wall_stops_a_knockback() {
    let wall = StaticCollider {
        id: 2,
        position: [0, 200, 330],
        half_extents: [500, 200, 30],
    };
    let mut warden = Caster::new(content(vec![wall], &[[0, 200]]), WARDEN);
    warden.cast(ids::SHIELD_BASH, creature(1));
    warden.ticks(t30u(12));
    assert!(
        warden.creature_at(1)[1] < 300.0,
        "{:?}",
        warden.creature_at(1)
    );
}

#[test]
fn fire_blast_hurls_back_what_it_sees_and_walls_shield_the_rest() {
    // A thin north–south wall at x = 0 splits the blast around wolf 1.
    let wall = StaticCollider {
        id: 2,
        position: [0, 200, 2_000],
        half_extents: [30, 200, 400],
    };
    let mut arcanist = Caster::new(
        content(vec![wall], &[[-300, 2_000], [150, 2_000], [-300, 2_300]]),
        ARCANIST,
    );
    let before = [arcanist.creature_at(1), arcanist.creature_at(3)];
    arcanist.cast(ids::FIRE_BLAST, creature(1));
    arcanist.ticks(t30u(45 + 12));
    assert!(
        arcanist.damaged(creature(1)),
        "the centre burns: {:?}",
        arcanist.refusals()
    );
    assert!(arcanist.damaged(creature(3)), "a visible neighbour burns");
    assert!(!arcanist.damaged(creature(2)), "the wall shields wolf 2");
    let pushed = arcanist.creature_at(3)[1] - before[1][1];
    assert!(
        pushed > 150.0,
        "wolf 3 was hurled away from the centre: {pushed}"
    );
}

#[test]
fn charge_rushes_at_the_target_and_knocks_it_back() {
    let mut warden = Caster::new(content(Vec::new(), &[[0, 1_450]]), WARDEN);
    warden.cast(ids::CHARGE, creature(1));
    warden.ticks(t30u(30));
    let [_, warden_z] = warden.player_at();
    assert!(warden_z > 1_000.0, "the Warden rushed north: {warden_z}");
    let forced = warden.zone.snapshot().unwrap().forced;
    assert!(
        forced.iter().any(|(unit, _)| *unit == creature(1)),
        "the charge knocked the wolf: {forced:?}"
    );
}

/// Two posts across the charge's path at z = 700 leaving `gap` units around x = 0.
fn posts(gap: i32) -> Vec<StaticCollider> {
    [(2, -1), (3, 1)]
        .map(|(id, side): (u32, i32)| StaticCollider {
            id,
            position: [side * (gap / 2 + 100), 100, 700],
            half_extents: [100, 100, 20],
        })
        .to_vec()
}

#[test]
fn a_charge_needs_a_path_as_wide_as_the_warden() {
    // A 40-unit gap: the sight line threads it, the 60-unit-wide Warden does not (#57).
    let run = || {
        let mut narrow = Caster::new(content(posts(40), &[[0, 1_450]]), WARDEN);
        narrow.cast(ids::CHARGE, creature(1));
        narrow.ticks(2);
        assert!(
            narrow.events.iter().any(|event| matches!(
                event,
                ZoneEvent::Error {
                    code: mmorpg_core::ErrorCode::LineOfSight,
                    ..
                }
            )),
            "{:?}",
            narrow.refusals()
        );
        assert!(narrow.player_at()[1].abs() < 1.0, "no charge");
        // A 1 m gap lets the charge through.
        let mut wide = Caster::new(content(posts(100), &[[0, 1_450]]), WARDEN);
        wide.cast(ids::CHARGE, creature(1));
        wide.ticks(t30u(30));
        assert!(wide.refusals().is_empty(), "{:?}", wide.refusals());
        assert!(wide.player_at()[1] > 1_000.0, "{:?}", wide.player_at());
    };
    run();
    #[cfg(feature = "physics-ab")]
    mmorpg_core::physics::using_backend(mmorpg_core::physics::Backend::Engine, run);
}
