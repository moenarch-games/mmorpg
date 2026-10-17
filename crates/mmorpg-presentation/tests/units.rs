//! Shared unit presentation (#184): every actor plays its own attempts, confirmed outcomes
//! flash, the viewer's guard shows, and a witnessed death falls.
use std::sync::Arc;

use mmorpg_core::{
    AbilityId, CreatureFamily, EntityFlags, EntityKind, EntityRef, EntitySnapshot, StrikeAim,
    StrikeHeight, StrikeSide, ZoneEvent, ZoneId, ZoneSimulation, ZoneSnapshot, greyhaven_vale,
};
use mmorpg_presentation::{
    Draw,
    units::{FrameInput, SampledUnit, UnitFrame, UnitPresenter, Viewer},
};

struct Scene {
    presenter: UnitPresenter,
    snapshot: ZoneSnapshot,
    wolf: EntityRef,
}

fn scene() -> Scene {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let mut snapshot = zone.snapshot_for_player(1).unwrap();
    let spawn = content
        .creature_spawns()
        .iter()
        .find(|s| content.creature_template(s.template).unwrap().family == CreatureFamily::Marauder)
        .unwrap();
    let viewer = snapshot.entities[0].clone();
    snapshot.entities.push(EntitySnapshot {
        kind: EntityKind::Creature,
        id: spawn.id.get(),
        appearance: u16::try_from(spawn.template.get()).unwrap(),
        position: [
            viewer.position[0] + 150,
            viewer.position[1],
            viewer.position[2],
        ],
        velocity: [0; 3],
        facing: 0,
        level: 2,
        health_percent: 100,
        flags: EntityFlags::default(),
    });
    let wolf = snapshot.entities.last().unwrap().entity();
    let scenery = Arc::new(mmorpg_scenery::greyhaven_vale_scenery());
    Scene {
        presenter: UnitPresenter::new(1, content, scenery),
        snapshot,
        wolf,
    }
}

fn frame(scene: &mut Scene, seconds: f64) -> UnitFrame {
    let units: Vec<_> = scene
        .snapshot
        .entities
        .iter()
        .map(|record| SampledUnit {
            record: record.clone(),
            centre: scene.presenter.rendered(record.position),
            yaw: 0.,
        })
        .collect();
    scene.presenter.frame(&FrameInput {
        snapshot: &scene.snapshot,
        units: &units,
        viewer: Viewer::default(),
        seconds,
        dt: 0.05,
        reduced_motion: false,
        bar_yaw: 0.,
    })
}

fn body(frame: &UnitFrame, entity: EntityRef) -> Vec<(String, [f32; 16])> {
    let view = frame.units.iter().find(|u| u.entity == entity).unwrap();
    frame.draws[view.body.clone()]
        .iter()
        .map(|d: &Draw| (d.id.clone(), d.model))
        .collect()
}

fn effects(frame: &UnitFrame) -> Vec<&str> {
    frame
        .draws
        .iter()
        .filter(|d| d.id.starts_with("fx-"))
        .map(|d| d.id.as_str())
        .collect()
}

#[test]
fn another_actor_plays_its_own_attack_and_confirmed_outcomes_flash() {
    // The same frames without any event are the rest pose to compare against.
    let mut idle = scene();
    idle.presenter.push(&idle.snapshot, 0.);
    frame(&mut idle, 0.);
    idle.snapshot.tick += 1;
    idle.presenter.push(&idle.snapshot, 1.);
    let rest = body(&frame(&mut idle, 1.1), idle.wolf);
    let mut scene = scene();
    scene.presenter.push(&scene.snapshot, 0.);
    frame(&mut scene, 0.);
    scene.snapshot.tick += 1;
    let viewer = EntityRef::Player(1);
    scene.snapshot.events = vec![
        ZoneEvent::AttackPerformed {
            source: scene.wolf,
            target: viewer,
        },
        ZoneEvent::DamageTaken {
            source: scene.wolf,
            target: viewer,
            amount: 4,
            critical: false,
        },
        ZoneEvent::Blocked {
            source: scene.wolf,
            target: viewer,
            lane: StrikeHeight::Middle,
            amount: 3,
        },
        ZoneEvent::StrikeSwept {
            source: viewer,
            target: Some(scene.wolf),
            ability: AbilityId::new(1),
            aim: StrikeAim {
                side: StrikeSide::Left,
                height: StrikeHeight::High,
            },
        },
    ];
    scene.snapshot.viewer.guard.raised = Some(StrikeHeight::Middle);
    scene.snapshot.viewer.guard.protecting = true;
    scene.presenter.push(&scene.snapshot, 1.);
    let acting = frame(&mut scene, 1.1);
    assert_ne!(
        body(&acting, scene.wolf),
        rest,
        "the attacker swings its own model"
    );
    let fx = effects(&acting);
    for suffix in ["-flash", "-plate", "-sweep-0", "-shield"] {
        assert!(
            fx.iter().any(|id| id.ends_with(suffix)),
            "{suffix} in {fx:?}"
        );
    }
    // Timed visuals expire on presentation time; the raised guard stays while it is held.
    let later = frame(&mut scene, 5.);
    assert_eq!(effects(&later).len(), 1);
    assert!(effects(&later)[0].ends_with("-shield"));
}

#[test]
fn a_witnessed_death_falls_while_a_corpse_seen_dead_lies_settled() {
    let mut scene = scene();
    scene.presenter.push(&scene.snapshot, 0.);
    frame(&mut scene, 0.);
    scene.snapshot.tick += 1;
    scene.snapshot.entities.last_mut().unwrap().flags.dead = true;
    scene.presenter.push(&scene.snapshot, 1.);
    let falling = body(&frame(&mut scene, 1.), scene.wolf);
    let fallen = body(&frame(&mut scene, 1.05), scene.wolf);
    assert_ne!(falling, fallen, "the body is still falling");
    for step in 0..20 {
        frame(&mut scene, 1.1 + f64::from(step) * 0.05);
    }
    let settled = body(&frame(&mut scene, 3.), scene.wolf);
    let mut fresh = self::scene();
    fresh.snapshot.entities.last_mut().unwrap().flags.dead = true;
    fresh.presenter.push(&fresh.snapshot, 0.);
    assert_eq!(body(&frame(&mut fresh, 0.), fresh.wolf), settled);
}
