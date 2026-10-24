//! Movable props (#58): a Shield Bash slides a heavy crate, which stops at a wall or a unit in
//! its way and comes to rest; a resting crate stays and wakes on the next push; a restored zone
//! continues a slide exactly; a broken crate leaves the world and returns at rest at its spawn.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureBehaviour, CreatureFamily, CreatureId, CreatureLife, CreatureTemplate,
    CreatureTemplateId, EntityRef, PropKind, StaticCollider, ZoneAreas, ZoneCommand, ZoneContent,
    ZoneDefinition, ZoneId, ZoneSimulation,
};
use support::arena;

const CRATE: CreatureTemplateId = CreatureTemplateId::new(5);
const WARDEN: u8 = 0;

fn heavy_crate() -> CreatureTemplate {
    CreatureTemplate {
        id: CRATE,
        name: "Test Heavy Crate".into(),
        family: CreatureFamily::Prop,
        behaviour: CreatureBehaviour::Prop(PropKind::HeavyCrate),
        min_level: 1,
        max_level: 1,
        health: 500,
        half_extents: [35, 35, 35],
        respawn_ticks: mmorpg_core::golden::t30(9_000) as _,
        ..arena::wolf(500, [1, 1])
    }
}

/// Flat ground, `walls`, a heavy crate 2 m north of the Warden and maybe a boar beyond it.
fn content(walls: Vec<StaticCollider>, boar: bool) -> Arc<ZoneContent> {
    let mut colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    colliders.extend(walls);
    let definition = ZoneDefinition::with_spawn_grid(
        77,
        mmorpg_core::STANDARD_GRAVITY,
        mmorpg_core::SpawnGrid {
            origin: [-5_000, -5_000],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![heavy_crate(), arena::boar()],
            if boar {
                vec![
                    arena::spawn(1, CRATE, [0, 200]),
                    arena::spawn(2, arena::BOAR, [0, 400]),
                ]
            } else {
                vec![arena::spawn(1, CRATE, [0, 200])]
            },
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(mmorpg_core::ABILITY_CATALOG_REVISION, Vec::new())
        .unwrap(),
    )
}

struct Warden {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Warden {
    fn new(content: Arc<ZoneContent>) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let zone = support::classes::at_level(zone, 1, 6);
        let mut state = zone.snapshot().unwrap();
        state.players[0].position = [0.0, 90.0, 0.0];
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut warden = Self { zone, sequence: 0 };
        warden.send(ZoneCommand::ChooseClass {
            class: WARDEN,
            sex: 0,
        });
        warden.send(arena::stand(arena::NORTHWARD));
        warden.zone.advance_tick().unwrap();
        let mut state = warden.zone.snapshot().unwrap();
        state.players[0].combat.abilities.resource = 100;
        warden.zone =
            ZoneSimulation::from_snapshot(state, Arc::clone(warden.zone.content())).unwrap();
        warden
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn bash(&mut self) {
        self.send(ZoneCommand::UseAbility {
            ability: ids::SHIELD_BASH.get(),
            target: Some(EntityRef::Creature(CreatureId::new(1))),
        });
    }

    fn crate_at(&self) -> ([f32; 3], [f32; 3]) {
        let state = self.zone.snapshot().unwrap();
        let record = state
            .creatures
            .iter()
            .find(|creature| creature.creature_id == CreatureId::new(1))
            .unwrap();
        (record.position, record.velocity)
    }

    /// Restores the zone with the Warden 2 m south of the crate and full rage.
    fn step_up(&mut self) {
        let (crate_at, _) = self.crate_at();
        let mut state = self.zone.snapshot().unwrap();
        state.players[0].position = [crate_at[0], 90.0, crate_at[2] - 200.0];
        state.players[0].combat.abilities.resource = 100;
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(self.zone.content())).unwrap();
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.zone.advance_tick().unwrap();
        }
    }
}

#[test]
fn a_bash_slides_the_crate_until_it_comes_to_rest() {
    let mut warden = Warden::new(content(Vec::new(), false));
    let (start, _) = warden.crate_at();
    warden.ticks(mmorpg_core::golden::t30(20) as _);
    assert_eq!(warden.crate_at().0, start, "a resting crate stays put");
    warden.bash();
    warden.ticks(mmorpg_core::golden::t30(4) as _);
    let (sliding, velocity) = warden.crate_at();
    assert!(
        sliding[2] > start[2] + 10.0 && velocity[2] > 0.0,
        "sliding north: {sliding:?}"
    );
    warden.ticks(mmorpg_core::golden::t30(120) as _);
    let (rest, velocity) = warden.crate_at();
    assert_eq!(velocity, [0.0; 3], "it came to rest");
    assert!(rest[2] > start[2] + 200.0, "it slid well away: {rest:?}");
    assert!(
        (rest[1] - start[1]).abs() < 1.0,
        "upright on the ground: {rest:?}"
    );
    // Units meet it where it rests.
    let view = warden.zone.snapshot_for_player(1).unwrap();
    let record = view
        .entities
        .iter()
        .find(|entity| entity.entity() == EntityRef::Creature(CreatureId::new(1)))
        .unwrap();
    #[allow(clippy::cast_possible_truncation)]
    let rounded = rest.map(|axis| axis.round() as i32);
    assert_eq!(
        record.position, rounded,
        "projections show it where it rests"
    );
}

#[test]
fn a_resting_crate_wakes_on_the_next_push() {
    let mut warden = Warden::new(content(Vec::new(), false));
    warden.bash();
    warden.ticks(mmorpg_core::golden::t30(360) as _);
    let (first, _) = warden.crate_at();
    warden.step_up();
    warden.ticks(mmorpg_core::golden::t30(30) as _);
    assert_eq!(warden.crate_at(), (first, [0.0; 3]), "asleep until pushed");
    warden.bash();
    warden.ticks(mmorpg_core::golden::t30(120) as _);
    let (second, velocity) = warden.crate_at();
    assert_eq!(velocity, [0.0; 3]);
    assert!(
        second[2] > first[2] + 200.0,
        "woke and slid again: {second:?}"
    );
}

#[test]
fn a_broken_crate_leaves_and_returns_at_rest_at_its_spawn() {
    let mut warden = Warden::new(content(Vec::new(), false));
    let (spawn, _) = warden.crate_at();
    warden.bash();
    warden.ticks(mmorpg_core::golden::t30(4) as _);
    // Break it mid-slide.
    let mut state = warden.zone.snapshot().unwrap();
    state.creatures[0].health = 1;
    warden.zone = ZoneSimulation::from_snapshot(state, Arc::clone(warden.zone.content())).unwrap();
    warden.step_up();
    warden.send(ZoneCommand::SelectTarget(Some(EntityRef::Creature(
        CreatureId::new(1),
    ))));
    warden.send(ZoneCommand::StartAttack);
    for _ in 0..mmorpg_core::golden::t30(300) {
        warden.zone.advance_tick().unwrap();
        if warden.zone.snapshot().unwrap().creatures[0].life != CreatureLife::Alive {
            break;
        }
    }
    let broken = warden.zone.snapshot().unwrap();
    assert!(matches!(
        broken.creatures[0].life,
        CreatureLife::Despawned { .. }
    ));
    assert_eq!(broken.debris.len(), 6, "it breaks like a crate");
    // Its spot is clear (the Warden stands by where it broke): it stands again at rest.
    warden.ticks(mmorpg_core::golden::t30(9_100) as _);
    let (back, velocity) = warden.crate_at();
    assert_eq!(
        warden.zone.snapshot().unwrap().creatures[0].life,
        CreatureLife::Alive
    );
    assert_eq!((back, velocity), (spawn, [0.0; 3]));
    warden.step_up();
    warden.bash();
    warden.ticks(mmorpg_core::golden::t30(120) as _);
    assert!(
        warden.crate_at().0[2] > spawn[2] + 200.0,
        "and slides again"
    );
}

#[test]
fn a_wall_stops_the_slide() {
    let wall = StaticCollider {
        id: 2,
        position: [0, 100, 400],
        half_extents: [300, 100, 20],
    };
    let mut warden = Warden::new(content(vec![wall], false));
    warden.bash();
    warden.ticks(mmorpg_core::golden::t30(120) as _);
    let (rest, velocity) = warden.crate_at();
    assert_eq!(velocity, [0.0; 3]);
    // The wall's south face is at z = 380; the crate's half-width is 35.
    assert!(
        rest[2] <= 380.0 - 35.0 + 1.0 && rest[2] > 300.0,
        "stopped at the wall: {rest:?}"
    );
}

#[test]
fn a_unit_in_the_way_stops_the_slide() {
    let mut warden = Warden::new(content(Vec::new(), true));
    warden.bash();
    warden.ticks(mmorpg_core::golden::t30(120) as _);
    let (rest, velocity) = warden.crate_at();
    assert_eq!(velocity, [0.0; 3]);
    // Unblocked it slides to about z = 465; the boar stands at z = 400 (radius 40) and the
    // crate is 35 wide, so it stops near z = 325 (Rapier allows a little contact slop).
    assert!(
        (300.0..=330.0).contains(&rest[2]),
        "stopped at the boar: {rest:?}"
    );
}

#[test]
fn a_restored_zone_continues_a_slide_exactly() {
    let mut warden = Warden::new(content(Vec::new(), false));
    warden.bash();
    warden.ticks(mmorpg_core::golden::t30(5) as _);
    let mut restored = ZoneSimulation::from_snapshot(
        warden.zone.snapshot().unwrap(),
        Arc::clone(warden.zone.content()),
    )
    .unwrap();
    for _ in 0..mmorpg_core::golden::t30(80) {
        warden.zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(
            warden.zone.snapshot().unwrap(),
            restored.snapshot().unwrap()
        );
    }
}
