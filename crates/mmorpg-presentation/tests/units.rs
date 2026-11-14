//! Shared unit presentation (#184): every actor plays its own attempts, confirmed outcomes
//! flash, the viewer's guard shows, and a witnessed death falls.
use std::sync::Arc;

use mmorpg_core::{
    AbilityId, ChatLine, ChatMessage, CreatureFamily, DoorView, Emote, EntityFlags, EntityKind,
    EntityRef, EntitySnapshot, ErrorCode, LootClaim, LootRewards, LootView, NpcId, PostureView,
    StrikeAim, StrikeHeight, StrikeSide, ZoneEvent, ZoneId, ZoneSimulation, ZoneSnapshot,
    greyhaven_vale,
};
use mmorpg_presentation::{
    Draw,
    units::{
        FrameInput, Interaction, PREPARATION_ANSWER_SECONDS, SampledUnit, UnitFrame, UnitPresenter,
        Viewer, imported::Kind,
    },
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
        appearance: spawn.template.get(),
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
            target: Some(viewer),
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

#[test]
fn projected_arrows_draw_along_their_flight() {
    let mut scene = scene();
    let at = scene.snapshot.entities[0].position;
    scene.snapshot.projectiles = vec![mmorpg_core::ProjectileView {
        id: 7,
        kind: mmorpg_core::ProjectileKind::Arrow,
        position: [at[0], at[1] + 30, at[2]],
        yaw: 16_384,
        pitch: 0,
        stuck: false,
        burst: false,
    }];
    let frame = frame(&mut scene, 1.);
    let shaft = frame
        .draws
        .iter()
        .find(|draw| draw.id == "projectile-7-shaft")
        .expect("the arrow's shaft is drawn");
    // Heading a quarter turn: the shaft lies behind the tip along −X.
    let tip = scene.presenter.rendered([at[0], at[1] + 30, at[2]]);
    assert!(
        (shaft.model[12] - (tip[0] - 0.4)).abs() < 1e-3,
        "{:?}",
        shaft.model
    );
    assert!((shaft.model[14] - tip[2]).abs() < 1e-3);
    assert!(
        frame
            .draws
            .iter()
            .any(|draw| draw.id == "projectile-7-head")
    );
}

#[test]
fn thrown_bombs_rest_with_a_fuse_and_burst_into_a_glow() {
    let mut scene = scene();
    let at = scene.snapshot.entities[0].position;
    let bomb = mmorpg_core::ProjectileView {
        id: 9,
        kind: mmorpg_core::ProjectileKind::Bomb,
        position: [at[0] + 200, at[1], at[2]],
        yaw: 0,
        pitch: 0,
        stuck: false,
        burst: false,
    };
    scene.snapshot.projectiles = vec![bomb];
    let has = |frame: &UnitFrame, id: &str| frame.draws.iter().any(|draw| draw.id == id);
    let resting = frame(&mut scene, 1.);
    assert!(has(&resting, "projectile-9-body") && has(&resting, "projectile-9-spark"));
    scene.snapshot.projectiles = vec![mmorpg_core::ProjectileView {
        burst: true,
        ..bomb
    }];
    let burst = frame(&mut scene, 1.);
    assert!(has(&burst, "projectile-9-burst"));
    assert!(!has(&burst, "projectile-9-body"));
}

#[test]
fn props_stand_as_their_pieces_and_debris_flies_with_a_powder_flash() {
    let mut scene = scene();
    let at = scene.snapshot.entities[0].position;
    scene.snapshot.entities.push(EntitySnapshot {
        kind: EntityKind::Creature,
        id: 900,
        appearance: greyhaven_vale::units::templates::CRATE.get(),
        position: [at[0] - 200, 35, at[2]],
        velocity: [0; 3],
        facing: 0,
        level: 1,
        health_percent: 100,
        flags: EntityFlags {
            attackable: true,
            ..EntityFlags::default()
        },
    });
    scene.snapshot.debris = vec![mmorpg_core::DebrisView {
        id: 4,
        kind: mmorpg_core::PropKind::ExplosiveBarrel,
        piece: 0,
        position: [at[0], 60, at[2] + 200],
        rotation: [0, 0, 0, 32_767],
        age: 2,
    }];
    let frame = frame(&mut scene, 1.);
    let crate_body = body(
        &frame,
        EntityRef::Creature(mmorpg_core::CreatureId::new(900)),
    );
    assert_eq!(
        crate_body
            .iter()
            .filter(|(id, _)| id.contains("-piece-"))
            .count(),
        6,
        "a crate is its six panels"
    );
    for id in ["debris-4-piece", "debris-4-flash"] {
        assert!(frame.draws.iter().any(|draw| draw.id == id), "{id}");
    }
}

#[test]
fn the_mill_hoist_is_drawn_where_its_schedule_puts_it() {
    // Content and the projection's tick place the hoist (#56); its top rises with the tick.
    let hoist = greyhaven_vale::MILL_HOIST;
    let mut scene = scene();
    let top_y = |scene: &mut Scene| {
        let frame = frame(scene, 0.);
        let top = frame
            .draws
            .iter()
            .find(|d| d.id == "platform-1-top")
            .expect("the hoist is drawn");
        top.model[13]
    };
    scene.snapshot.tick = 0;
    let low = top_y(&mut scene);
    scene.snapshot.tick = u64::from(hoist.dwell + hoist.travel_ticks());
    let high = top_y(&mut scene);
    assert!((high - low - 8.0).abs() < 0.01, "{low} -> {high}");
}

/// The viewer's imported model's upper-body clip and time this frame.
fn viewer_upper(frame: &UnitFrame) -> (&'static str, f32) {
    frame
        .imported
        .iter()
        .find(|unit| unit.entity == EntityRef::Player(1))
        .unwrap()
        .animation
        .upper
}

#[test]
fn timed_sword_motion_and_trail_follow_the_accepted_aim_until_a_clash() {
    let mut scene = scene();
    scene.snapshot.viewer.class = Some(mmorpg_core::ClassChoice {
        class: mmorpg_core::PlayerClass::Warden,
        sex: mmorpg_core::Sex::Male,
    });
    scene.presenter.model_ready(
        Kind::Ranger,
        ["Idle_Loop", "Sword_Slash_Mid_C"].map(|clip| (clip.to_owned(), 0.6)),
    );
    let me = EntityRef::Player(1);
    scene
        .snapshot
        .entities
        .iter_mut()
        .find(|record| record.entity() == me)
        .unwrap()
        .facing = 0; // A turn after beginning the accepted eastward swing.
    scene.snapshot.events = vec![ZoneEvent::SwingBegan {
        source: me,
        ability: AbilityId::new(1),
        facing: 16384,
        aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
        startup: 18,
        active: 9,
    }];
    scene.presenter.push(&scene.snapshot, 0.);
    let startup = frame(&mut scene, 0.2);
    assert!(
        startup
            .draws
            .iter()
            .all(|draw| !draw.id.ends_with("-sword-trail"))
    );
    let active = frame(&mut scene, 0.4);
    let unit = active
        .imported
        .iter()
        .find(|unit| unit.entity == me)
        .unwrap();
    assert!(
        (unit.root.q[1] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6,
        "the accepted eastward heading must survive a later northward turn"
    );
    let pose = unit
        .animation
        .layers
        .iter()
        .find(|layer| layer.clip == "TimedSword_Mid_C")
        .unwrap();
    assert!((pose.time - 2. / 3.).abs() < 1e-5);
    assert!(
        active
            .draws
            .iter()
            .any(|draw| draw.id.ends_with("-sword-trail"))
    );
    scene.snapshot.tick += 1;
    scene.snapshot.events = vec![ZoneEvent::Clashed {
        source: me,
        target: scene.wolf,
        kind: mmorpg_core::ClashKind::Swings,
    }];
    scene.presenter.push(&scene.snapshot, 0.41);
    let clashed = frame(&mut scene, 0.42);
    assert!(
        clashed
            .draws
            .iter()
            .all(|draw| !draw.id.ends_with("-sword-trail"))
    );
    assert!(clashed.imported.iter().all(|unit| {
        unit.animation
            .layers
            .iter()
            .all(|layer| !layer.clip.starts_with("TimedSword_"))
    }));
}

#[test]
fn the_local_wind_up_waits_for_the_zone_to_commit_or_refuse_it() {
    let heroic = AbilityId::new(1);
    let mut scene = scene();
    scene.presenter.model_ready(
        Kind::Ranger,
        [
            ("Idle_Loop".to_owned(), 1.),
            ("Sword_Attack".to_owned(), 1.5),
        ],
    );
    scene.presenter.push(&scene.snapshot, 0.);
    frame(&mut scene, 0.);
    // The press winds up at once, before any answer, and holds short of the swing.
    scene.presenter.prepare(heroic, 0.);
    scene.presenter.prepared_sent(0.);
    assert_eq!(viewer_upper(&frame(&mut scene, 0.1)).0, "Sword_Attack");
    assert_eq!(
        viewer_upper(&frame(&mut scene, 0.6)),
        (
            "Sword_Attack",
            mmorpg_presentation::units::imported::WINDUP_HOLD_SECONDS
        )
    );
    // A refusal lets it go: no swing is shown.
    scene.snapshot.tick += 1;
    scene.snapshot.events = vec![ZoneEvent::Error {
        code: ErrorCode::NotReady,
        target: None,
    }];
    scene.presenter.push(&scene.snapshot, 0.65);
    assert_eq!(scene.presenter.preparing(), None);
    frame(&mut scene, 0.7);
    assert_eq!(viewer_upper(&frame(&mut scene, 2.)).0, "Idle_Loop");

    // Only the zone's accepted action commits the swing, continuing the wind-up.
    scene.presenter.prepare(heroic, 2.);
    scene.presenter.prepared_sent(2.);
    frame(&mut scene, 2.1);
    scene.snapshot.tick += 1;
    scene.snapshot.events = vec![ZoneEvent::AbilityUsed {
        source: EntityRef::Player(1),
        target: Some(scene.wolf),
        ability: heroic,
    }];
    scene.presenter.push(&scene.snapshot, 2.2);
    assert_eq!(scene.presenter.preparing(), None);
    let swing = frame(&mut scene, 2.3);
    let (clip, at) = viewer_upper(&swing);
    assert_eq!(clip, "Sword_Attack");
    assert!(at > 0.25, "the swing continues past the wind-up: {at}");

    // An unanswered wind-up lets go after its bound; a cast-time ability never prepares.
    scene.snapshot.events.clear();
    scene.presenter.prepare(heroic, 5.);
    scene.presenter.prepared_sent(5.);
    frame(&mut scene, 5.5);
    assert_eq!(scene.presenter.preparing(), Some(heroic));
    frame(&mut scene, 5. + PREPARATION_ANSWER_SECONDS + 0.01);
    assert_eq!(scene.presenter.preparing(), None);
    scene.presenter.prepare(AbilityId::new(9), 7.);
    assert_eq!(scene.presenter.preparing(), None);
}

#[test]
fn a_twin_dagger_rogue_alternates_hands_once_per_swing() {
    let mut scene = scene();
    scene.snapshot.viewer.class = Some(mmorpg_core::ClassChoice {
        class: mmorpg_core::PlayerClass::Rogue,
        sex: mmorpg_core::Sex::Male,
    });
    scene.presenter.model_ready(
        Kind::Ranger,
        [
            "Idle_Loop",
            "Dagger_Idle_Loop",
            "Dagger_Stab_R",
            "Dagger_Slash_L",
        ]
        .map(|clip| (clip.to_owned(), 0.4)),
    );
    let me = EntityRef::Player(1);
    let swing = |scene: &mut Scene, now: f64| {
        // One swing covering two units reports an attack on each (#517).
        scene.snapshot.tick += 1;
        scene.snapshot.events = vec![
            ZoneEvent::AttackPerformed {
                source: me,
                target: Some(scene.wolf),
            },
            ZoneEvent::AttackPerformed {
                source: me,
                target: None,
            },
        ];
        scene.presenter.push(&scene.snapshot, now);
        viewer_upper(&frame(scene, now + 0.1)).0
    };
    assert_eq!(viewer_upper(&frame(&mut scene, 0.)).0, "Dagger_Idle_Loop");
    assert_eq!(swing(&mut scene, 1.), "Dagger_Stab_R");
    assert_eq!(swing(&mut scene, 2.), "Dagger_Slash_L");
    assert_eq!(swing(&mut scene, 3.), "Dagger_Stab_R");
}

#[test]
fn a_ranger_power_draws_aimed_shot_and_looses_rapidly_under_rapid_fire() {
    use mmorpg_core::ability::ids;
    let mut scene = scene();
    scene.snapshot.viewer.class = Some(mmorpg_core::ClassChoice {
        class: mmorpg_core::PlayerClass::Ranger,
        sex: mmorpg_core::Sex::Male,
    });
    scene.presenter.model_ready(
        Kind::Ranger,
        [
            ("Idle_Loop", 2.),
            ("Bow_Aim_Loop", 2.5),
            ("Bow_Release", 0.47),
            ("Bow_Recovery", 1.1),
            ("Bow_Aim_Upper_Loop", 2.5),
            ("Bow_Release_Upper", 0.47),
            ("Bow_Recovery_Upper", 1.1),
            ("Bow_Rapid_Release", 0.33),
            ("Bow_Power_Draw", 1.),
            ("Flourish_Haste", 0.7),
        ]
        .map(|(clip, seconds)| (clip.to_owned(), seconds)),
    );
    let me = EntityRef::Player(1);
    let shoot = |scene: &mut Scene, event: ZoneEvent, now: f64| {
        scene.snapshot.tick += 1;
        scene.snapshot.events = vec![event];
        scene.presenter.push(&scene.snapshot, now);
        viewer_upper(&frame(scene, now + 0.05)).0
    };
    frame(&mut scene, 0.);
    // Aimed Shot winds up with the power draw, held drawn while the key is down: a refusal of
    // something else in the meantime does not let an unsent draw go …
    scene.presenter.prepare(ids::AIMED_SHOT, 0.);
    assert_eq!(viewer_upper(&frame(&mut scene, 0.3)).0, "Bow_Power_Draw");
    scene.snapshot.tick += 1;
    scene.snapshot.events = vec![ZoneEvent::Error {
        code: mmorpg_core::ErrorCode::OutOfRange,
        target: None,
    }];
    scene.presenter.push(&scene.snapshot, 0.32);
    assert_eq!(viewer_upper(&frame(&mut scene, 0.34)).0, "Bow_Power_Draw");
    scene.presenter.prepared_sent(0.35);
    let used = ZoneEvent::AbilityUsed {
        source: me,
        target: Some(scene.wolf),
        ability: ids::AIMED_SHOT,
    };
    // … and looses with the upper-body release, then recovers with its twin.
    assert_eq!(shoot(&mut scene, used, 0.4), "Bow_Release_Upper");
    assert_eq!(viewer_upper(&frame(&mut scene, 1.)).0, "Bow_Recovery_Upper");
    // An ordinary shot standing keeps the full-body release.
    let attack = ZoneEvent::AttackPerformed {
        source: me,
        target: Some(scene.wolf),
    };
    assert_eq!(shoot(&mut scene, attack, 3.), "Bow_Release");
    // Under Rapid Fire every shot is the rapid release, back to back.
    scene.snapshot.auras = vec![mmorpg_core::AuraView {
        ability: ids::RAPID_FIRE,
        kind: mmorpg_core::AuraKind::Haste,
        remaining: 300,
        amount: 40,
    }];
    assert_eq!(shoot(&mut scene, attack, 5.), "Bow_Rapid_Release");
    assert_eq!(shoot(&mut scene, attack, 5.4), "Bow_Rapid_Release");
}

/// The caster clips (#609, asset-tooling #420) by name and length.
const CASTER_CLIPS: [(&str, f32); 23] = [
    ("Idle_Loop", 2.),
    ("Grip_Staff_R", 0.03),
    ("Grip_Staff_L", 0.03),
    ("Grip_Book_L", 0.03),
    ("Cast_Charge_Loop", 1.),
    ("Cast_Release_Push", 0.5),
    ("Cast_Release_Overhand", 0.6),
    ("Cast_Channel_Beam_Loop", 1.5),
    ("Cast_Channel_Drain_Loop", 1.5),
    ("Cast_Channel_Sky_Loop", 2.),
    ("Cast_Nova", 0.8),
    ("Cast_Cone", 0.9),
    ("Cast_Curse_Point", 0.5),
    ("Cast_Ground_Sweep", 0.7),
    ("Cast_Self_Ward", 0.9),
    ("Cast_Ally", 0.7),
    ("Warcry", 1.),
    ("Cast_Summon_Raise", 1.5),
    ("Cast_Summon_Call", 1.2),
    ("Staff_Strike_A", 0.7),
    ("Staff_Strike_B", 0.7),
    ("Cast_Interrupted", 0.4),
    ("Spell_Simple_Shoot", 0.5),
];

fn caster_scene(class: mmorpg_core::PlayerClass) -> Scene {
    let mut scene = scene();
    scene.snapshot.viewer.class = Some(mmorpg_core::ClassChoice {
        class,
        sex: mmorpg_core::Sex::Male,
    });
    scene.presenter.model_ready(
        Kind::Ranger,
        CASTER_CLIPS.map(|(clip, seconds)| (clip.to_owned(), seconds)),
    );
    scene
}

fn play(scene: &mut Scene, events: Vec<ZoneEvent>, now: f64) -> &'static str {
    scene.snapshot.tick += 1;
    scene.snapshot.events = events;
    scene.presenter.push(&scene.snapshot, now);
    viewer_upper(&frame(scene, now + 0.05)).0
}

fn viewer_grips(frame: &UnitFrame) -> Vec<&'static str> {
    let viewer = frame
        .imported
        .iter()
        .find(|unit| unit.entity == EntityRef::Player(1))
        .unwrap();
    viewer
        .animation
        .layers
        .iter()
        .filter(|layer| layer.clip.starts_with("Grip_") && layer.weight > 0.5)
        .map(|layer| layer.clip)
        .collect()
}

#[test]
fn an_arcanist_charges_releases_channels_and_flinches_out_of_an_interrupt() {
    use mmorpg_core::ability::ids;
    let mut scene = caster_scene(mmorpg_core::PlayerClass::Arcanist);
    let wolf = scene.wolf;
    let me = EntityRef::Player(1);
    let cast = |ability, ticks| ZoneEvent::CastStarted {
        source: me,
        target: None,
        ability,
        ticks,
    };
    let used = |ability, target| ZoneEvent::AbilityUsed {
        source: me,
        target,
        ability,
    };
    frame(&mut scene, 0.);
    // Firebolt holds the charge through its cast bar, then pushes the bolt out.
    assert_eq!(
        play(&mut scene, vec![cast(ids::FIREBOLT, 150)], 1.),
        "Cast_Charge_Loop"
    );
    assert_eq!(viewer_upper(&frame(&mut scene, 2.)).0, "Cast_Charge_Loop");
    assert_eq!(
        play(&mut scene, vec![used(ids::FIREBOLT, Some(wolf))], 2.5),
        "Cast_Release_Push"
    );
    // The tome rides the back of the hand: no left-hand grip over a caster clip.
    assert_eq!(viewer_grips(&frame(&mut scene, 2.8)), ["Grip_Staff_R"]);
    // Back at rest, the hand closes round the tome again.
    for step in 0..10 {
        frame(&mut scene, 4. + f64::from(step) * 0.05);
    }
    assert_eq!(
        viewer_grips(&frame(&mut scene, 4.5)),
        ["Grip_Book_L", "Grip_Staff_R"]
    );
    // Fire Blast throws overhand; the instants play their own styles.
    assert_eq!(
        play(&mut scene, vec![used(ids::FIRE_BLAST, Some(wolf))], 5.),
        "Cast_Release_Overhand"
    );
    assert_eq!(
        play(&mut scene, vec![used(ids::FROST_NOVA, None)], 7.),
        "Cast_Nova"
    );
    assert_eq!(
        play(&mut scene, vec![used(ids::FROST_BREATH, None)], 9.),
        "Cast_Cone"
    );
    assert_eq!(
        play(&mut scene, vec![used(ids::ARCANE_BARRIER, None)], 11.),
        "Cast_Self_Ward"
    );
    // Lightning Beam loops while channelling; an interrupt flinches out of it.
    assert_eq!(
        play(&mut scene, vec![cast(ids::LIGHTNING_BEAM, 180)], 13.),
        "Cast_Channel_Beam_Loop"
    );
    assert_eq!(
        viewer_upper(&frame(&mut scene, 15.)).0,
        "Cast_Channel_Beam_Loop"
    );
    let interrupted = ZoneEvent::Interrupted {
        source: Some(scene.wolf),
        target: me,
        ability: ids::LIGHTNING_BEAM,
    };
    assert_eq!(
        play(&mut scene, vec![interrupted], 15.5),
        "Cast_Interrupted"
    );
    assert_eq!(
        play(&mut scene, vec![cast(ids::BLIZZARD, 360)], 17.),
        "Cast_Channel_Sky_Loop"
    );
    // The staff's auto-attack alternates the jab and the sweep, both hands on the staff.
    let swing = ZoneEvent::AttackPerformed {
        source: me,
        target: Some(scene.wolf),
    };
    assert_eq!(play(&mut scene, vec![swing], 30.), "Staff_Strike_A");
    for step in 0..5 {
        frame(&mut scene, 30.05 + f64::from(step) * 0.05);
    }
    assert_eq!(
        viewer_grips(&frame(&mut scene, 30.3)),
        ["Grip_Staff_L", "Grip_Staff_R"]
    );
    assert_eq!(play(&mut scene, vec![swing], 32.), "Staff_Strike_B");
}

#[test]
fn summons_kneel_and_whistle_through_their_casts_and_shields_touch_an_ally() {
    use mmorpg_core::ability::ids;
    let me = EntityRef::Player(1);
    let mut scene = caster_scene(mmorpg_core::PlayerClass::Necromancer);
    frame(&mut scene, 0.);
    let raise = ZoneEvent::CastStarted {
        source: me,
        target: None,
        ability: ids::RAISE_SKELETON,
        ticks: 90,
    };
    assert_eq!(play(&mut scene, vec![raise], 1.), "Cast_Summon_Raise");
    // Its completion adds nothing: the kneel plays on to its end.
    let raised = ZoneEvent::AbilityUsed {
        source: me,
        target: None,
        ability: ids::RAISE_SKELETON,
    };
    let (clip, time) = {
        scene.snapshot.tick += 1;
        scene.snapshot.events = vec![raised];
        scene.presenter.push(&scene.snapshot, 2.45);
        viewer_upper(&frame(&mut scene, 2.45))
    };
    assert_eq!(clip, "Cast_Summon_Raise");
    assert!(time > 1.3, "{time}");

    // Call Companion charges, then whistles to end with its 2 s cast.
    let mut scene = caster_scene(mmorpg_core::PlayerClass::Druid);
    frame(&mut scene, 0.);
    let call = ZoneEvent::CastStarted {
        source: me,
        target: None,
        ability: ids::CALL_COMPANION,
        ticks: 120,
    };
    assert_eq!(play(&mut scene, vec![call], 1.), "Cast_Charge_Loop");
    assert_eq!(viewer_upper(&frame(&mut scene, 2.)).0, "Cast_Summon_Call");

    // Divine Shield wards oneself, or touches another player.
    let mut scene = caster_scene(mmorpg_core::PlayerClass::Cleric);
    frame(&mut scene, 0.);
    let shield = |target| ZoneEvent::AbilityUsed {
        source: me,
        target: Some(target),
        ability: ids::DIVINE_SHIELD,
    };
    assert_eq!(play(&mut scene, vec![shield(me)], 1.), "Cast_Self_Ward");
    assert_eq!(
        play(&mut scene, vec![shield(EntityRef::Player(2))], 3.),
        "Cast_Ally"
    );
}

#[test]
fn a_bundle_without_the_caster_clips_keeps_the_simple_spell() {
    use mmorpg_core::ability::ids;
    let mut scene = scene();
    scene.snapshot.viewer.class = Some(mmorpg_core::ClassChoice {
        class: mmorpg_core::PlayerClass::Arcanist,
        sex: mmorpg_core::Sex::Male,
    });
    scene.presenter.model_ready(
        Kind::Ranger,
        [
            ("Idle_Loop", 2.),
            ("Spell_Simple_Idle_Loop", 1.),
            ("Spell_Simple_Shoot", 0.5),
            ("Spell_Simple_Exit", 0.4),
        ]
        .map(|(clip, seconds)| (clip.to_owned(), seconds)),
    );
    let me = EntityRef::Player(1);
    frame(&mut scene, 0.);
    let cast = ZoneEvent::CastStarted {
        source: me,
        target: None,
        ability: ids::FIREBOLT,
        ticks: 150,
    };
    assert_eq!(play(&mut scene, vec![cast], 1.), "Spell_Simple_Idle_Loop");
    let interrupted = ZoneEvent::Interrupted {
        source: None,
        target: me,
        ability: ids::FIREBOLT,
    };
    assert_eq!(
        play(&mut scene, vec![interrupted], 1.5),
        "Spell_Simple_Exit"
    );
    let swing = ZoneEvent::AttackPerformed {
        source: me,
        target: Some(scene.wolf),
    };
    assert_eq!(play(&mut scene, vec![swing], 3.), "Spell_Simple_Shoot");
}

/// A scene whose viewer is an imported Ranger with every social clip loaded (#619).
fn social_scene() -> Scene {
    let mut scene = scene();
    scene.presenter.model_ready(
        Kind::Ranger,
        [
            ("Idle_Loop", 1.),
            ("Walk_Loop", 1.),
            ("Hit_Chest", 0.5),
            ("Emote_Wave", 1.6),
            ("Emote_Bow", 1.8),
            ("Door_Push", 0.6),
            ("Door_Pull", 0.6),
            ("Door_Knock", 1.2),
            ("Loot_Kneel_Enter", 0.5),
            ("Loot_Kneel_Loop", 2.),
            ("Loot_Kneel_Exit", 0.5),
        ]
        .map(|(clip, seconds)| (clip.to_owned(), seconds)),
    );
    scene.presenter.push(&scene.snapshot, 0.);
    frame(&mut scene, 0.);
    scene
}

fn viewer_lower(frame: &UnitFrame) -> (&'static str, f32) {
    frame
        .imported
        .iter()
        .find(|unit| unit.entity == EntityRef::Player(1))
        .unwrap()
        .animation
        .lower
}

fn emote(scene: &mut Scene, emote: Emote, seconds: f64) {
    scene.snapshot.tick += 1;
    scene.snapshot.chat = vec![ChatLine {
        speaker: 1,
        message: ChatMessage::Emote(emote),
    }];
    scene.presenter.push(&scene.snapshot, seconds);
    scene.snapshot.chat.clear();
}

#[test]
fn an_emote_in_the_projection_plays_on_its_speaker_until_a_blow_ends_it() {
    let mut scene = social_scene();
    emote(&mut scene, Emote::Wave, 1.);
    assert_eq!(viewer_upper(&frame(&mut scene, 1.1)).0, "Emote_Wave");
    // The legs are not asked to do anything for an upper-body emote.
    assert_eq!(viewer_lower(&frame(&mut scene, 1.2)).0, "Idle_Loop");
    scene.snapshot.tick += 1;
    scene.snapshot.events = vec![ZoneEvent::DamageTaken {
        source: scene.wolf,
        target: EntityRef::Player(1),
        amount: 1,
        critical: false,
    }];
    scene.presenter.push(&scene.snapshot, 1.3);
    scene.snapshot.events.clear();
    assert_ne!(viewer_upper(&frame(&mut scene, 1.4)).0, "Emote_Wave");
}

#[test]
fn the_whole_body_bow_plays_standing_and_moves_to_the_upper_body_for_a_step() {
    let mut scene = social_scene();
    emote(&mut scene, Emote::Bow, 1.);
    assert_eq!(viewer_lower(&frame(&mut scene, 1.1)).0, "Emote_Bow");
    // Walking away frees the legs at once (Atlas 6.6.i745): the projected velocity moves the
    // unit and the bow finishes on the upper body, at its own clock, over the gait.
    scene.snapshot.entities[0].velocity = [4, 0, 0];
    let walking = frame(&mut scene, 1.2);
    assert_ne!(viewer_lower(&walking).0, "Emote_Bow");
    let (clip, at) = viewer_upper(&walking);
    assert_eq!(clip, "Emote_Bow");
    assert!(at > 0.1, "the bow continues rather than restarting: {at}");
}

#[test]
fn a_remote_players_accepted_emote_plays_on_it_for_a_nearby_observer() {
    // Two players in one zone; the zone accepts player 2's wave and projects it to player 1.
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    zone.add_player(2).unwrap();
    zone.advance_tick().unwrap();
    zone.apply_command(2, 1, mmorpg_core::ZoneCommand::Emote(Emote::Wave))
        .unwrap();
    zone.advance_tick().unwrap();
    let snapshot = zone.snapshot_for_player(1).unwrap();
    let wave = ChatLine {
        speaker: 2,
        message: ChatMessage::Emote(Emote::Wave),
    };
    assert_eq!(snapshot.chat, [wave]);
    let scenery = Arc::new(mmorpg_scenery::greyhaven_vale_scenery());
    let mut scene = Scene {
        presenter: UnitPresenter::new(1, content, scenery),
        snapshot,
        wolf: EntityRef::Player(2),
    };
    scene.presenter.model_ready(
        Kind::Ranger,
        [("Idle_Loop".to_owned(), 1.), ("Emote_Wave".to_owned(), 1.6)],
    );
    scene.presenter.push(&scene.snapshot, 1.);
    let shown = frame(&mut scene, 1.2);
    let remote = shown
        .imported
        .iter()
        .find(|unit| unit.entity == EntityRef::Player(2))
        .expect("the remote player is drawn with the imported model");
    assert_eq!(remote.animation.upper.0, "Emote_Wave");
    assert_eq!(remote.animation.lower.0, "Idle_Loop");
    // The observer itself does not wave.
    assert_ne!(viewer_upper(&shown).0, "Emote_Wave");
}

#[test]
fn a_door_flip_plays_push_away_from_the_nearest_unit_and_pull_closing_it() {
    let content = greyhaven_vale::content();
    let door = content.doors()[0];
    let [closed, open] = [door.leaf(false).position, door.leaf(true).position];
    let mut scene = social_scene();
    // Stand on the side the leaf swings away from.
    let away = |axis: usize| (closed[axis] - open[axis]).signum() * 100;
    scene.snapshot.entities[0].position = [closed[0] + away(0), closed[1], closed[2] + away(2)];
    let view = |open| {
        vec![DoorView {
            door: door.id,
            open,
        }]
    };
    scene.snapshot.doors = view(false);
    scene.presenter.push(&scene.snapshot, 1.);
    frame(&mut scene, 1.);
    scene.snapshot.tick += 1;
    scene.snapshot.doors = view(true);
    scene.presenter.push(&scene.snapshot, 2.);
    assert_eq!(viewer_upper(&frame(&mut scene, 2.1)).0, "Door_Push");
    scene.snapshot.tick += 1;
    scene.snapshot.doors = view(false);
    scene.presenter.push(&scene.snapshot, 4.);
    assert_eq!(viewer_upper(&frame(&mut scene, 4.1)).0, "Door_Pull");
}

#[test]
fn the_loot_window_kneels_at_a_reachable_corpse_and_walking_stands_the_viewer_up() {
    let mut scene = social_scene();
    scene.snapshot.loot = Some(LootView {
        claim: LootClaim {
            creature: match scene.wolf {
                EntityRef::Creature(creature) => creature,
                _ => unreachable!(),
            },
            died_at: 1,
        },
        rewards: LootRewards {
            money: 3,
            item: None,
            quest_item: None,
        },
    });
    // Without the window nothing kneels.
    assert_eq!(viewer_lower(&frame(&mut scene, 1.)).0, "Idle_Loop");
    scene.presenter.set_interaction(Interaction {
        loot_window: true,
        npc: None,
    });
    assert_eq!(viewer_lower(&frame(&mut scene, 1.1)).0, "Loot_Kneel_Enter");
    assert_eq!(viewer_lower(&frame(&mut scene, 1.7)).0, "Loot_Kneel_Loop");
    // Closing the window stands up by the exit clip.
    scene.presenter.set_interaction(Interaction::default());
    assert_eq!(viewer_lower(&frame(&mut scene, 1.8)).0, "Loot_Kneel_Exit");
    // Reopened and walked away from, the loop never pins the walking unit.
    scene.presenter.set_interaction(Interaction {
        loot_window: true,
        npc: None,
    });
    frame(&mut scene, 2.4);
    frame(&mut scene, 3.);
    scene.snapshot.entities[0].velocity = [4, 0, 0];
    assert_eq!(viewer_lower(&frame(&mut scene, 3.1)).0, "Walk_Loop");
    // Standing again with the window open does not kneel: the kneel stays broken.
    scene.snapshot.entities[0].velocity = [0, 0, 0];
    assert_eq!(viewer_lower(&frame(&mut scene, 3.5)).0, "Idle_Loop");
}

#[test]
fn a_resident_at_its_bed_at_night_lies_on_the_mattress_and_only_then() {
    let content = greyhaven_vale::content();
    let elden = NpcId::new(1);
    let home = *content.routine(elden).unwrap().path.last().unwrap();
    let mut scene = social_scene();
    scene.presenter.residents_ready();
    scene.presenter.model_ready(
        Kind::Ranger,
        [("Sleep_Bed_Enter", 1.5), ("Sleep_Bed_Loop", 4.)]
            .map(|(clip, seconds)| (clip.to_owned(), seconds)),
    );
    // The zone's own Elden, moved to the end of his routine.
    let record = scene
        .snapshot
        .entities
        .iter_mut()
        .find(|record| record.entity() == EntityRef::Npc(elden))
        .unwrap();
    record.position = [home[0], record.position[1], home[1]];
    record.facing = 16_384;
    let shown = |frame: UnitFrame| {
        let unit = frame
            .imported
            .into_iter()
            .find(|unit| unit.entity == EntityRef::Npc(elden))
            .unwrap();
        (unit.animation.lower.0, unit.root.p[0])
    };
    // By day the same spot is just standing there.
    assert_eq!(shown(frame(&mut scene, 1.)).0, "Idle_Loop");
    let set_posture = |scene: &mut Scene, posture| {
        scene
            .snapshot
            .entities
            .iter_mut()
            .find(|record| record.entity() == EntityRef::Npc(elden))
            .unwrap()
            .flags
            .posture = posture;
    };
    set_posture(&mut scene, PostureView::Asleep);
    assert_eq!(shown(frame(&mut scene, 1.1)).0, "Sleep_Bed_Enter");
    let mut last = shown(frame(&mut scene, 1.2));
    for step in 0..30 {
        last = shown(frame(&mut scene, 1.3 + f64::from(step) * 0.1));
    }
    assert_eq!(last.0, "Sleep_Bed_Loop");
    // The drawn root moves off the bedside onto the mattress side; the zone's position stays.
    let bedside = f64::from(home[0]) / 100.;
    assert!(
        (last.1 - (bedside + 0.47)).abs() < 0.03,
        "root at {} for bedside {bedside}",
        last.1
    );
}

#[test]
fn a_sitting_player_sits_on_the_seat_or_the_ground_the_zone_projects() {
    let content = greyhaven_vale::content();
    let seat = content.seats()[0];
    let mut scene = social_scene();
    scene.presenter.model_ready(
        Kind::Ranger,
        [
            ("Sit_Chair_Enter", 0.8),
            ("Sit_Chair_Loop", 3.),
            ("Sit_Chair_Exit", 0.8),
            ("Sit_Ground_Loop", 3.),
        ]
        .map(|(clip, seconds)| (clip.to_owned(), seconds)),
    );
    scene.snapshot.entities[0].flags.posture = PostureView::Sitting;
    assert_eq!(viewer_lower(&frame(&mut scene, 1.)).0, "Sit_Ground_Loop");
    // Moved to a seat's body spot: the chair chain.
    scene.snapshot.entities[0].flags.posture = PostureView::Free;
    frame(&mut scene, 1.05);
    scene.snapshot.entities[0].position = [seat.stand[0], 90, seat.stand[1]];
    scene.snapshot.entities[0].flags.posture = PostureView::Sitting;
    assert_eq!(viewer_lower(&frame(&mut scene, 1.1)).0, "Sit_Chair_Enter");
    assert_eq!(viewer_lower(&frame(&mut scene, 2.)).0, "Sit_Chair_Loop");
    // Standing up by asking again plays the exit.
    scene.snapshot.entities[0].flags.posture = PostureView::Free;
    assert_eq!(viewer_lower(&frame(&mut scene, 2.1)).0, "Sit_Chair_Exit");
}

#[test]
fn a_resident_getting_up_plays_the_wake_and_a_directing_line_points() {
    let elden = NpcId::new(1);
    let mut scene = social_scene();
    scene.presenter.residents_ready();
    scene.presenter.model_ready(
        Kind::Ranger,
        [
            ("Sleep_Bed_Loop", 4.),
            ("Sleep_Bed_Wake", 1.5),
            ("Talk_Gesture_B", 1.5),
        ]
        .map(|(clip, seconds)| (clip.to_owned(), seconds)),
    );
    let posture = |scene: &mut Scene, posture| {
        scene
            .snapshot
            .entities
            .iter_mut()
            .find(|record| record.entity() == EntityRef::Npc(elden))
            .unwrap()
            .flags
            .posture = posture;
    };
    let lower = |frame: UnitFrame| {
        frame
            .imported
            .into_iter()
            .find(|unit| unit.entity == EntityRef::Npc(elden))
            .unwrap()
            .animation
    };
    posture(&mut scene, PostureView::Asleep);
    frame(&mut scene, 1.);
    posture(&mut scene, PostureView::Waking);
    assert_eq!(lower(frame(&mut scene, 1.1)).lower.0, "Sleep_Bed_Wake");
    // It holds its last frame through the end of the rise instead of looping.
    assert_eq!(lower(frame(&mut scene, 3.)).lower.0, "Sleep_Bed_Wake");
    posture(&mut scene, PostureView::Free);
    frame(&mut scene, 3.05);
    // Accepting a quest whose line names a place: the NPC points.
    let sheet = |entries| mmorpg_core::QuestSheet {
        completed: 0,
        entries,
        markers: Vec::new(),
    };
    scene.snapshot.quests = Some(sheet(Vec::new()));
    scene.presenter.push(&scene.snapshot, 3.1);
    scene.presenter.set_interaction(Interaction {
        loot_window: false,
        npc: Some(elden),
    });
    scene.snapshot.tick += 1;
    scene.snapshot.quests = Some(sheet(vec![mmorpg_core::QuestEntry {
        quest: greyhaven_vale::quests::SCOUT_THE_LAKE,
        progress: [0; mmorpg_core::MAX_QUEST_OBJECTIVES],
    }]));
    scene.presenter.push(&scene.snapshot, 3.2);
    assert_eq!(lower(frame(&mut scene, 3.3)).upper.0, "Talk_Gesture_B");
}
