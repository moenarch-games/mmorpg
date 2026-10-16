use mmorpg_client::{
    camera::OrbitCamera,
    presentation::{MAX_SCENE_BOXES, Presentation},
};
use mmorpg_core::{
    CreatureId, EntityFlags, EntityKind, EntityRef, EntitySnapshot, NpcId, SNAPSHOT_SCHEMA_VERSION,
    TargetDetail, ViewerState, ZoneId, ZoneSnapshot, greyhaven_vale,
};
use mmorpg_scenery::greyhaven_vale_scenery;
use std::{
    f32::consts::{FRAC_PI_2, TAU},
    time::{Duration, Instant},
};

fn snapshot(tick: u64, position: [i32; 3]) -> ZoneSnapshot {
    facing_snapshot(tick, position, 0)
}

fn facing_snapshot(tick: u64, position: [i32; 3], facing: u16) -> ZoneSnapshot {
    ZoneSnapshot {
        loot: None,
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        zone_id: ZoneId::new(1),
        tick,
        content_revision: greyhaven_vale::REVISION,
        acknowledged_sequence: 1,
        viewer_id: 1,
        inventory_revision: 1,
        inventory: None,
        equipment: None,
        quests: None,
        viewer: ViewerState {
            experience: 0,
            experience_to_next_level: 100,
            health: 50,
            max_health: 50,
            level: 1,
            ..ViewerState::default()
        },
        target_of_target: None,
        target_detail: TargetDetail::default(),
        cooldowns: Vec::new(),
        auras: Vec::new(),
        events: Vec::new(),
        chat: Vec::new(),
        entities: vec![EntitySnapshot {
            kind: EntityKind::Player,
            id: 1,
            appearance: 0,
            position,
            velocity: [21, 0, 0],
            facing,
            level: 1,
            health_percent: 100,
            flags: EntityFlags::default(),
        }],
    }
}

fn presentation(now: Instant) -> Presentation {
    Presentation::new(1, greyhaven_vale_scenery(), greyhaven_vale::content(), now).unwrap()
}

// Samples below move along the Hollow Road (x = 0), where relief is flat.

#[test]
fn interpolation_preserves_large_ticks_holds_on_loss_and_ignores_old_packets() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    presentation
        .push(snapshot(u64::MAX - 4, [0, 50, 0]), now)
        .unwrap();
    presentation
        .push(snapshot(u64::MAX, [0, 50, 400]), now)
        .unwrap();
    assert_eq!(presentation.camera_target(now), [0.0, 0.5, 2.0]);
    assert_eq!(
        presentation.camera_target(now + Duration::from_secs(1)),
        [0.0, 0.5, 4.0]
    );
    assert!(
        !presentation
            .push(snapshot(u64::MAX - 1, [0, 0, -100]), now)
            .unwrap()
    );
    assert_eq!(presentation.camera_target(now), [0.0, 0.5, 2.0]);
}

#[test]
fn units_render_on_the_shared_relief_with_entity_dimensions() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    // A meadow south of the outpost, off every road and structure.
    let scenery = greyhaven_vale_scenery();
    let (x, z, relief) = (2_000..11_000)
        .step_by(100)
        .map(|x| (x, 9_000, scenery.height_at(x, 9_000)))
        .find(|(_, _, relief)| *relief != 0)
        .expect("meadows roll");
    presentation
        .push(facing_snapshot(1, [x, 90, z], 16_384), now)
        .unwrap();
    let scene = presentation.scene(
        now,
        OrbitCamera::default().view(presentation.camera_target(now)),
    );
    // One 16-box humanoid and two health-bar boxes; scenery is static.
    assert_eq!(scene.len(), 18);
    let model = &scene[..16];
    let centre = [
        x as f32 / 100.0,
        0.9 + relief as f32 / 100.0,
        z as f32 / 100.0,
    ];
    let feet = model
        .iter()
        .map(|item| item.position[1] - item.size[1] / 2.0)
        .fold(f32::MAX, f32::min);
    assert!(
        (feet - (centre[1] - 0.9)).abs() < 1e-4,
        "feet rest on the relief"
    );
    assert!(model.iter().all(|item| (item.yaw - FRAC_PI_2).abs() < 1e-6));
    assert_eq!(presentation.camera_target(now), centre);
    // A quarter turn faces +X: the dark face plate sits in front of the head.
    let face = model
        .iter()
        .find(|item| item.color == [0.2, 0.15, 0.13])
        .expect("a face plate");
    assert!(face.position[0] > centre[0] + 0.1);
    assert!((face.position[2] - centre[2]).abs() < 1e-4);
    assert!(face.position[1] > centre[1], "at head height");
}

#[test]
fn facing_interpolates_along_the_shorter_arc() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    presentation
        .push(facing_snapshot(10, [0, 90, 0], 65_000), now)
        .unwrap();
    presentation
        .push(facing_snapshot(14, [0, 90, 0], 500), now)
        .unwrap();
    // Two ticks of delay place the sample halfway: 65 000 + 518 steps.
    let halfway = presentation.players(now)[&1].yaw;
    let expected = 65_518.0 / 65_536.0 * TAU;
    assert!((halfway - expected).abs() < 1e-4, "{halfway} vs {expected}");
    let latest = presentation.players(now + Duration::from_secs(1))[&1].yaw;
    assert!((latest - 500.0 / 65_536.0 * TAU).abs() < 1e-4, "{latest}");
}

#[test]
fn incompatible_content_zones_and_duplicate_entities_fail_closed() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    presentation.push(snapshot(1, [0; 3]), now).unwrap();
    let mut wrong_content = snapshot(2, [0; 3]);
    wrong_content.content_revision += 1;
    assert!(presentation.push(wrong_content, now).is_err());
    let mut wrong_zone = snapshot(2, [0; 3]);
    wrong_zone.zone_id = ZoneId::new(2);
    assert!(presentation.push(wrong_zone, now).is_err());
    let mut duplicate = snapshot(2, [0; 3]);
    duplicate.entities.push(duplicate.entities[0].clone());
    assert!(presentation.push(duplicate, now).is_err());
    let mut other_viewer = snapshot(2, [0; 3]);
    other_viewer.viewer_id = 2;
    assert!(presentation.push(other_viewer, now).is_err());
}

#[test]
fn a_resumed_connection_discards_old_interpolation_even_when_ticks_regress() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    presentation.push(snapshot(100, [0, 50, 0]), now).unwrap();
    presentation.push(snapshot(104, [0, 50, 400]), now).unwrap();
    presentation.reset(now);
    assert!(presentation.players(now).is_empty());
    assert!(presentation.push(snapshot(2, [0, 50, 900]), now).unwrap());
    assert_eq!(presentation.camera_target(now), [0.0, 0.5, 9.0]);
}

fn creature(id: u32, template: u16, position: [i32; 3], flags: EntityFlags) -> EntitySnapshot {
    EntitySnapshot {
        kind: EntityKind::Creature,
        id,
        appearance: template,
        position,
        velocity: [0; 3],
        facing: 0,
        level: 2,
        health_percent: if flags.dead { 0 } else { 43 },
        flags,
    }
}

#[test]
fn creatures_and_npcs_render_by_size_disposition_and_state() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    let hostile = EntityFlags {
        hostile: true,
        attackable: true,
        ..EntityFlags::default()
    };
    let mut snapshot = snapshot(1, [0, 90, -2_000]);
    let wolf = EntityRef::Creature(CreatureId::new(108));
    snapshot.viewer.target = Some(wolf);
    snapshot.viewer.auto_attacking = true;
    snapshot.viewer.in_combat = true;
    snapshot.viewer.health = 38;
    snapshot.entities.extend([
        creature(108, 1, [0, 45, -1_800], hostile),
        creature(
            120,
            2,
            [300, 45, -2_000],
            EntityFlags {
                attackable: true,
                ..EntityFlags::default()
            },
        ),
        creature(
            109,
            1,
            [-300, 45, -2_000],
            EntityFlags {
                dead: true,
                hostile: true,
                ..EntityFlags::default()
            },
        ),
        EntitySnapshot {
            kind: EntityKind::Npc,
            id: 6,
            appearance: 6,
            position: [0, 90, -2_600],
            velocity: [0; 3],
            facing: 0,
            level: 10,
            health_percent: 100,
            flags: EntityFlags::default(),
        },
    ]);
    presentation.push(snapshot, now).unwrap();
    let scene = presentation.scene(
        now,
        OrbitCamera::default().view(presentation.camera_target(now)),
    );
    // Units sort by kind then id: the player (humanoid + two bar boxes), the
    // targeted wolf (marker + 10-box quadruped + bars), the wolf corpse (10
    // flat boxes), the boar (11 boxes + bars) and the guard (humanoid, no bars).
    assert_eq!(scene.len(), 18 + 13 + 10 + 13 + 16);
    let wolf_boxes = &scene[18..31];
    assert!(wolf_boxes[0].size[1] < 0.05, "the target marker is flat");
    let wolf_torso = wolf_boxes[5];
    assert!(
        wolf_torso.color[0] > wolf_torso.color[1] + 0.3,
        "hostile is red-ish"
    );
    let corpse = &scene[31..41];
    assert!(
        corpse.iter().all(|item| item.size[1] < 0.2),
        "a corpse lies flat"
    );
    let boar_torso = scene[41 + 4];
    assert!(
        boar_torso.color[0] > 0.6 && boar_torso.color[1] > 0.5,
        "neutral is yellow-ish"
    );
    let guard = &scene[54..70];
    assert!(
        guard.iter().any(|item| item.color[1] > item.color[0] + 0.2),
        "friendly is green-ish"
    );

    assert_eq!(
        presentation.status().unwrap(),
        "HP 38/50 · in combat · target Timber Wolf (L2, 43%) · attacking"
    );
    let mut caster = presentation.latest().unwrap().clone();
    caster.tick += 1;
    caster.viewer.resource = Some(mmorpg_core::ResourceView {
        kind: mmorpg_core::ResourceKind::Mana,
        value: 85,
        max: 110,
    });
    caster.viewer.cast = Some(mmorpg_core::CastView {
        ability: mmorpg_core::AbilityId::new(9),
        elapsed: 20,
        total: 60,
        channel: false,
    });
    presentation.push(caster, now).unwrap();
    assert_eq!(
        presentation.status().unwrap(),
        "HP 38/50 · mana 85/110 · casting Firebolt 20/60 · in combat · target Timber Wolf (L2, 43%) · attacking"
    );
    assert_eq!(
        presentation.unit_name(EntityRef::Npc(NpcId::new(6))),
        "Greyhaven Guard"
    );
    // Tab cycles through living attackable creatures, nearest first.
    assert_eq!(
        presentation.next_tab_target(),
        Some(EntityRef::Creature(CreatureId::new(120)))
    );
}

#[test]
fn scenery_and_content_of_different_revisions_are_refused() {
    let mut scenery = greyhaven_vale_scenery();
    scenery.content_revision += 1;
    assert!(Presentation::new(1, scenery, greyhaven_vale::content(), Instant::now()).is_err());
}

#[test]
fn health_bars_use_projected_percent_and_face_the_camera_above_the_body() {
    let now = Instant::now();
    for (percent, expected_width) in [(100, 1.0), (43, 0.43), (0, 0.0)] {
        let mut presentation = presentation(now);
        let mut sample = snapshot(1, [0, 90, 0]);
        sample.entities[0].health_percent = percent;
        // Exact viewer health must not override the entity's quantized record.
        sample.viewer.health = 1;
        presentation.push(sample, now).unwrap();
        let mut camera = OrbitCamera::default();
        camera.orbit(150.0, 0.0);
        let view = camera.view(presentation.camera_target(now));
        let scene = presentation.scene(now, view);
        let body = scene[5];
        let background = scene[16];
        assert!(background.position[1] > body.position[1] + body.size[1] / 2.0);
        let yaw = (view.target[0] - view.eye[0]).atan2(view.target[2] - view.eye[2]);
        assert!((background.yaw - yaw).abs() < 1e-5);
        if percent == 0 {
            assert_eq!(scene.len(), 17, "zero health has no fill geometry");
        } else {
            assert_eq!(scene.len(), 18);
            let fill = scene[17];
            assert!((fill.size[0] - expected_width).abs() < 1e-5);
            assert_eq!(fill.yaw, background.yaw);
            let dx = fill.position[0] - background.position[0];
            let dz = fill.position[2] - background.position[2];
            let local_x = dx * yaw.cos() - dz * yaw.sin();
            let local_z = dx * yaw.sin() + dz * yaw.cos();
            assert!((local_x + expected_width / 2.0 - 0.5).abs() < 1e-5);
            assert!(local_z < 0.0, "the fill sits on the eye side");
        }
    }
}

#[test]
fn old_packets_cannot_restore_health_and_reset_discards_bars() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    presentation.push(snapshot(10, [0, 90, 0]), now).unwrap();
    let mut damaged = snapshot(12, [0, 90, 0]);
    damaged.entities[0].health_percent = 20;
    presentation.push(damaged, now).unwrap();
    assert!(!presentation.push(snapshot(11, [0, 90, 0]), now).unwrap());
    let sampled = now + Duration::from_secs(1);
    let view = OrbitCamera::default().view(presentation.camera_target(sampled));
    assert!((presentation.scene(sampled, view)[17].size[0] - 0.2).abs() < 1e-5);
    presentation.reset(sampled);
    assert!(presentation.scene(sampled, view).is_empty());
    presentation.push(snapshot(1, [0, 90, 0]), sampled).unwrap();
    assert_eq!(presentation.scene(sampled, view)[17].size[0], 1.0);
    let mut dead = snapshot(2, [0, 90, 0]);
    dead.entities[0].flags.dead = true;
    dead.entities[0].health_percent = 0;
    presentation.push(dead, sampled).unwrap();
    assert_eq!(
        presentation
            .scene(sampled + Duration::from_secs(1), view)
            .len(),
        16,
        "a dead player stands as before, without a health bar"
    );
}

#[test]
fn maximum_projection_fits_the_instance_budget_and_larger_input_is_refused() {
    let now = Instant::now();
    let mut presentation = presentation(now);
    let mut sample = snapshot(1, [0, 90, 0]);
    for id in 2..=u32::try_from(mmorpg_core::MAX_VISIBLE_ENTITIES).unwrap() {
        let mut player = sample.entities[0].clone();
        player.id = id;
        sample.entities.push(player);
    }
    sample.viewer.target = Some(EntityRef::Player(2));
    presentation.push(sample.clone(), now).unwrap();
    let view = OrbitCamera::default().view(presentation.camera_target(now));
    assert_eq!(presentation.scene(now, view).len(), MAX_SCENE_BOXES);
    sample.tick += 1;
    let mut extra = sample.entities[0].clone();
    extra.id = 100;
    sample.entities.push(extra);
    assert!(presentation.push(sample, now).is_err());
    assert_eq!(presentation.latest().unwrap().tick, 1);
}
