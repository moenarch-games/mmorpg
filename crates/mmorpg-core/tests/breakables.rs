//! Breakable props (#288): a crate a player smashes vanishes into debris that flies, settles
//! and despawns, gives nothing, survives recovery exactly and stands again after its respawn
//! time once its spot is clear; powder barrels explode like Fire Blast and chain-react.
mod support;

use std::sync::Arc;

use mmorpg_core::physics::Backend;
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureBehaviour, CreatureFamily, CreatureId, CreatureLife,
    CreatureTemplate, CreatureTemplateId, DEBRIS_TICKS, EntityRef, PropKind, ZoneCommand,
    ZoneContent, ZoneId, ZoneSimulation,
};
use support::arena;

const CRATE: CreatureTemplateId = CreatureTemplateId::new(9);
const POWDER: CreatureTemplateId = CreatureTemplateId::new(11);
const BOAR: CreatureTemplateId = arena::BOAR;

fn prop(
    id: CreatureTemplateId,
    kind: PropKind,
    health: u32,
    half_extents: [i32; 3],
) -> CreatureTemplate {
    CreatureTemplate {
        id,
        name: "Test Prop".into(),
        family: CreatureFamily::Prop,
        behaviour: CreatureBehaviour::Prop(kind),
        damage: [1, 1],
        respawn_ticks: mmorpg_core::golden::t30(9_000) as _,
        half_extents,
        ..arena::wolf(health, [1, 1])
    }
}

/// A crate at spawn 1; powder barrels 2–4 in a row; a sturdy boar 5 beside them.
fn content() -> Arc<ZoneContent> {
    arena::arena(
        vec![
            CreatureTemplate {
                health: 2_000,
                ..arena::boar()
            },
            prop(CRATE, PropKind::Crate, 12, [35, 35, 35]),
            prop(POWDER, PropKind::ExplosiveBarrel, 10, [30, 45, 30]),
        ],
        vec![
            arena::spawn(1, CRATE, [0, -300]),
            arena::spawn(2, POWDER, [-2_000, -1_000]),
            arena::spawn(3, POWDER, [-1_930, -1_000]),
            arena::spawn(4, POWDER, [-1_860, -1_000]),
            arena::spawn(5, BOAR, [-1_760, -900]),
        ],
        Vec::new(),
    )
}

fn creature(id: u32) -> EntityRef {
    EntityRef::Creature(CreatureId::new(id))
}

struct Smasher {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Smasher {
    /// Player 1 standing at `feet` (XZ).
    fn at(feet: [f32; 2]) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(3), content()).unwrap();
        zone.add_player(1).unwrap();
        let mut state = zone.snapshot().unwrap();
        state.players[0].position = [feet[0], 90.0, feet[1]];
        Self {
            zone: ZoneSimulation::from_snapshot(state, content()).unwrap(),
            sequence: 0,
        }
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn state(&self) -> CanonicalZoneSnapshot {
        self.zone.snapshot().unwrap()
    }

    fn life(&self, id: u32) -> CreatureLife {
        self.state().creatures[usize::try_from(id - 1).unwrap()].life
    }

    fn restore(&mut self, state: CanonicalZoneSnapshot) {
        self.zone = ZoneSimulation::from_snapshot(state, content()).unwrap();
    }

    /// Attacks `target` until it breaks.
    fn smash(&mut self, target: u32) {
        self.send(ZoneCommand::SelectTarget(Some(creature(target))));
        for _ in 0..mmorpg_core::golden::t30(900) {
            // Swing at the prop whenever it is in reach (#517).
            if let Some(swing) = arena::swing_at(&self.zone, 1, creature(target)) {
                self.send(swing);
            }
            self.zone.advance_tick().unwrap();
            if self.life(target) != CreatureLife::Alive {
                return;
            }
        }
        panic!("prop {target} never broke");
    }
}

#[test]
fn a_smashed_crate_flies_apart_settles_vanishes_and_returns() {
    let mut player = Smasher::at([0.0, -180.0]);
    assert_eq!(player.zone.physics_backend(), Backend::Engine);
    player.smash(1);
    let broken = player.state();
    assert!(matches!(player.life(1), CreatureLife::Despawned { .. }));
    assert_eq!(broken.debris.len(), 6, "six panels");
    assert_eq!(broken.players[0].combat.experience, 0, "props give nothing");
    assert_eq!(broken.players[0].combat.target, None);
    let view = player.zone.snapshot_for_player(1).unwrap();
    assert_eq!(view.debris.len(), 6);
    assert!(
        view.entities
            .iter()
            .all(|entity| entity.entity() != creature(1))
    );

    // Recovery mid-flight continues exactly.
    for _ in 0..mmorpg_core::golden::t30(5) {
        player.zone.advance_tick().unwrap();
    }
    let mut copy = ZoneSimulation::from_snapshot(player.state(), content()).unwrap();
    for _ in 0..mmorpg_core::golden::t30(30) {
        player.zone.advance_tick().unwrap();
        copy.advance_tick().unwrap();
    }
    assert_eq!(copy.snapshot().unwrap(), player.state());

    // The pieces flew, came down and settled on the ground.
    let flying = player.state();
    assert!(
        flying
            .debris
            .iter()
            .zip(&broken.debris)
            .any(|(now, then)| now.body.position != then.body.position)
    );
    for _ in 0..mmorpg_core::golden::t30(80) {
        player.zone.advance_tick().unwrap();
    }
    let lying = player.state();
    assert!(
        lying.debris.iter().all(|piece| piece.body.resting),
        "{:?}",
        lying.debris
    );
    assert!(
        lying
            .debris
            .iter()
            .all(|piece| (-2.0..40.0).contains(&piece.body.position[1])),
        "{:?}",
        lying.debris
    );
    for _ in 0..usize::from(DEBRIS_TICKS) {
        player.zone.advance_tick().unwrap();
    }
    assert!(player.state().debris.is_empty(), "pieces despawn");

    // Five minutes later the crate stands again, but not on a player.
    let mut state = player.state();
    let CreatureLife::Despawned { died_at } = state.creatures[0].life else {
        panic!("the crate is gone");
    };
    state.tick = died_at + mmorpg_core::golden::t30(9_000);
    state.players[0].position = [0.0, 90.0, -300.0];
    player.restore(state);
    player.zone.advance_tick().unwrap();
    assert!(matches!(player.life(1), CreatureLife::Despawned { .. }));
    let mut state = player.state();
    state.players[0].position = [0.0, 90.0, -100.0];
    player.restore(state);
    player.zone.advance_tick().unwrap();
    assert_eq!(player.life(1), CreatureLife::Alive);
}

#[test]
fn powder_barrels_chain_react_hurting_and_hurling_everything_near() {
    let mut player = Smasher::at([-2_000.0, -1_160.0]);
    let before = player.state();
    player.smash(2);
    let after = player.state();
    for barrel in 2..=4 {
        assert!(
            matches!(player.life(barrel), CreatureLife::Despawned { .. }),
            "barrel {barrel} went up"
        );
    }
    assert_eq!(after.debris.len(), 24, "eight pieces per barrel");
    // The breaker is hurt and hurled back, the boar too.
    let hero = &after.players[0];
    assert!(hero.combat.health < 50, "{}", hero.combat.health);
    let boar = &after.creatures[4];
    assert!(boar.health < 2_000);
    assert!(!after.forced.is_empty(), "knockbacks are under way");
    for _ in 0..mmorpg_core::golden::t30(12) {
        player.zone.advance_tick().unwrap();
    }
    let later = player.state();
    assert!(later.players[0].position[2] < before.players[0].position[2] - 50.0);
    assert!(later.creatures[4].position[0] > before.creatures[4].position[0] + 50.0);
}

#[test]
fn redbrand_cave_supplies_break_against_the_authored_wall_and_recover_exactly() {
    let shipped = mmorpg_core::greyhaven_vale::content();
    let spawns: Vec<_> = shipped
        .creature_spawns()
        .iter()
        .filter(|spawn| (926..=928).contains(&spawn.id.get()))
        .cloned()
        .collect();
    assert_eq!(spawns.len(), 3, "two crates and one ordinary barrel");
    // Keep the real terrain, cave triangles, templates and placements; isolate the supplies
    // from the garrison so combat with enemies does not interrupt the rigid-mesh workload.
    for spawn in spawns {
        let content = Arc::new(
            ZoneContent::new(
                shipped.definition().clone(),
                shipped.areas().clone(),
                shipped.creature_templates().to_vec(),
                vec![spawn.clone()],
                Vec::new(),
                shipped.graveyard(),
            )
            .unwrap(),
        );
        let template = content.creature_template(spawn.template).unwrap();
        let kind = template.prop().unwrap();
        assert!(matches!(kind, PropKind::Crate | PropKind::Barrel));
        let mut zone = ZoneSimulation::with_content(ZoneId::new(3), content.clone()).unwrap();
        zone.add_player(1).unwrap();
        let mut state = zone.snapshot().unwrap();
        let [x, z] = spawn.position.map(|axis| axis as f32);
        state.players[0].position = [x + 150.0, 1190.0, z];
        zone = ZoneSimulation::from_snapshot(state, content.clone()).unwrap();
        let mut sequence = 0;
        for _ in 0..mmorpg_core::ticks_from_ms(30_000) {
            if let Some(swing) = arena::swing_at(&zone, 1, creature(spawn.id.get())) {
                sequence += 1;
                zone.apply_command(1, sequence, swing).unwrap();
            }
            zone.advance_tick().unwrap();
            if zone.snapshot().unwrap().creatures.iter().any(|unit| {
                unit.creature_id == spawn.id && matches!(unit.life, CreatureLife::Despawned { .. })
            }) {
                break;
            }
        }
        let broken = zone.snapshot().unwrap();
        assert_eq!(broken.debris.len(), mmorpg_core::debris_pieces(kind).len());
        assert!(matches!(
            broken
                .creatures
                .iter()
                .find(|unit| unit.creature_id == spawn.id)
                .unwrap()
                .life,
            CreatureLife::Despawned { .. }
        ));
        let mut copy = ZoneSimulation::from_snapshot(broken, content.clone()).unwrap();
        let mut wall_contact = false;
        let mut settled = false;
        for _ in 0..DEBRIS_TICKS {
            let before = zone.snapshot().unwrap();
            zone.advance_tick().unwrap();
            copy.advance_tick().unwrap();
            let after = zone.snapshot().unwrap();
            assert_eq!(
                after,
                copy.snapshot().unwrap(),
                "recovery for {}",
                spawn.id.get()
            );
            for (then, now) in before.debris.iter().zip(&after.debris) {
                // A westbound airborne piece slows or rebounds at the actual west wall
                // (x = 4800), before reaching the floor. Gravity alone cannot do this.
                wall_contact |= then.body.position[0] < 4850.0
                    && then.body.position[1] > 1145.0
                    && then.body.velocity[0] < -0.1
                    && now.body.velocity[0] > then.body.velocity[0] + 0.1;
            }
            settled |=
                !after.debris.is_empty() && after.debris.iter().all(|piece| piece.body.resting);
            assert!(
                after.debris.iter().all(|piece| {
                    piece.body.position[0] >= 4799.0 && piece.body.position[1] >= 1098.0
                }),
                "wall and floor retain the supplies' debris"
            );
        }
        assert!(
            wall_contact,
            "{} debris contacted the cave wall",
            spawn.id.get()
        );
        assert!(
            settled,
            "{} debris settled on the cave floor",
            spawn.id.get()
        );
        assert!(zone.snapshot().unwrap().debris.is_empty());
    }
}
