//! Shared unit presentation (#184): every actor plays its own attempts, confirmed outcomes
//! flash, the viewer's guard shows, and a witnessed death falls.
use std::sync::Arc;

use mmorpg_core::{
    AbilityId, CreatureFamily, EntityFlags, EntityKind, EntityRef, EntitySnapshot, ErrorCode,
    StrikeAim, StrikeHeight, StrikeSide, ZoneEvent, ZoneId, ZoneSimulation, ZoneSnapshot,
    greyhaven_vale,
};
use mmorpg_presentation::{
    Draw,
    units::{
        FrameInput, PREPARATION_ANSWER_SECONDS, SampledUnit, UnitFrame, UnitPresenter, Viewer,
        imported::Kind,
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
    // Aimed Shot winds up with the power draw, held drawn at its end until the zone answers …
    scene.presenter.prepare(ids::AIMED_SHOT, 0.);
    scene.presenter.prepared_sent(0.);
    assert_eq!(viewer_upper(&frame(&mut scene, 0.3)).0, "Bow_Power_Draw");
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
