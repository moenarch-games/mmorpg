use mmorpg_client::{
    presentation::SceneBox,
    unit_models::{
        AnimalKind, CORPSE_HEIGHT, Gait, HumanoidKind, MODEL_BOX_BUDGET, Placement, UnitModel,
        append_unit,
    },
};
use mmorpg_core::{CreatureFamily, EntityKind, NpcRole, PLAYER_HALF_EXTENTS_UNITS, greyhaven_vale};

const COLOR: [f32; 3] = [0.7, 0.3, 0.2];

fn metres(units: [i32; 3]) -> [f32; 3] {
    units.map(|value| value as f32 / 100.0)
}

fn draw(model: UnitModel, half: [f32; 3], gait: Gait, corpse: bool) -> Vec<SceneBox> {
    let mut boxes = Vec::new();
    let placement = Placement {
        centre: [0.0, half[1], 0.0],
        half,
        yaw: 0.0,
    };
    append_unit(&mut boxes, model, COLOR, placement, gait, corpse);
    boxes
}

fn every_model() -> Vec<(UnitModel, [f32; 3])> {
    let character = metres(PLAYER_HALF_EXTENTS_UNITS);
    let content = greyhaven_vale::content();
    let mut models: Vec<_> = [
        HumanoidKind::Player,
        HumanoidKind::Guard,
        HumanoidKind::Vendor,
        HumanoidKind::QuestGiver,
        HumanoidKind::SpiritHealer,
    ]
    .into_iter()
    .map(|kind| (UnitModel::Humanoid(kind), character))
    .collect();
    for template in content.creature_templates() {
        let model = UnitModel::choose(
            EntityKind::Creature,
            Some(template.family),
            template.elite,
            None,
        );
        models.push((model, metres(template.half_extents)));
    }
    models
}

#[test]
fn model_choice_follows_kind_family_role_and_elite() {
    use CreatureFamily as F;
    let choose = UnitModel::choose;
    assert_eq!(
        choose(EntityKind::Player, None, false, None),
        UnitModel::Humanoid(HumanoidKind::Player)
    );
    for (family, expected) in [
        (F::Wolf, UnitModel::Quadruped(AnimalKind::Wolf)),
        (F::Boar, UnitModel::Quadruped(AnimalKind::Boar)),
        (F::Vermin, UnitModel::Quadruped(AnimalKind::Vermin)),
        (F::Marauder, UnitModel::Humanoid(HumanoidKind::Marauder)),
        (F::Mirefin, UnitModel::Humanoid(HumanoidKind::Mirefin)),
        (F::Redbrand, UnitModel::Humanoid(HumanoidKind::Redbrand)),
    ] {
        assert_eq!(
            choose(EntityKind::Creature, Some(family), false, None),
            expected
        );
    }
    assert_eq!(
        choose(EntityKind::Creature, Some(F::Redbrand), true, None),
        UnitModel::Humanoid(HumanoidKind::Garrick)
    );
    assert_eq!(
        choose(EntityKind::Creature, None, false, None),
        UnitModel::Placeholder
    );
    for (role, expected) in [
        (NpcRole::Guard, HumanoidKind::Guard),
        (NpcRole::Vendor, HumanoidKind::Vendor),
        (NpcRole::QuestGiver, HumanoidKind::QuestGiver),
        (NpcRole::SpiritHealer, HumanoidKind::SpiritHealer),
    ] {
        assert_eq!(
            choose(EntityKind::Npc, None, false, Some(role)),
            UnitModel::Humanoid(expected)
        );
    }
}

#[test]
fn models_fit_their_box_within_ten_percent_and_the_box_budget() {
    for (model, half) in every_model() {
        let boxes = draw(model, half, Gait::IDLE, false);
        assert!(
            (10..=MODEL_BOX_BUDGET).contains(&boxes.len()),
            "{model:?}: {}",
            boxes.len()
        );
        for item in &boxes {
            for axis in 0..3 {
                let centre = if axis == 1 { half[1] } else { 0.0 };
                let low = item.position[axis] - item.size[axis] / 2.0 - centre;
                let high = item.position[axis] + item.size[axis] / 2.0 - centre;
                let bound = half[axis] * 1.1 + 1e-4;
                assert!(
                    low >= -bound && high <= bound,
                    "{model:?} axis {axis}: {low}..{high} vs {bound}"
                );
            }
        }
        let feet = boxes
            .iter()
            .map(|item| item.position[1] - item.size[1] / 2.0)
            .fold(f32::MAX, f32::min);
        assert!(feet.abs() < 1e-4, "{model:?} stands on the ground: {feet}");
    }
}

#[test]
fn units_are_idle_at_zero_speed_and_move_their_legs_when_running() {
    assert_eq!(Gait::new(0.0, 12.3), Gait::IDLE);
    assert_eq!(Gait::new(0.01, 12.3), Gait::IDLE);
    assert_eq!(Gait::new(6.3, 1.0), Gait::new(6.3, 1.0));
    for (model, half) in every_model() {
        let rest = draw(model, half, Gait::IDLE, false);
        assert_eq!(rest, draw(model, half, Gait::new(0.0, 3.0), false));
        let running = draw(model, half, Gait::new(6.3, 0.2), false);
        assert_eq!(rest.len(), running.len());
        let moved = rest.iter().zip(&running).filter(|(a, b)| a != b).count();
        assert!(moved >= 4, "{model:?} moved {moved} boxes");
    }
}

#[test]
fn corpses_lie_flat_and_darkened() {
    for (model, half) in every_model() {
        let boxes = draw(model, half, Gait::IDLE, true);
        assert!(!boxes.is_empty());
        let tallest = boxes
            .iter()
            .map(|item| item.position[1] + item.size[1] / 2.0)
            .fold(0.0, f32::max);
        assert!(tallest <= CORPSE_HEIGHT + 1e-4, "{model:?}: {tallest}");
        assert!(
            boxes
                .iter()
                .all(|item| item.position[1] - item.size[1] / 2.0 >= -1e-4),
            "{model:?} stays above the ground"
        );
        assert!(
            boxes
                .iter()
                .all(|item| item.color.iter().all(|channel| *channel <= 0.45)),
            "{model:?} is darkened"
        );
    }
}

#[test]
fn unknown_templates_keep_the_single_box_with_a_nose() {
    let half = [0.4, 0.45, 0.4];
    let boxes = draw(UnitModel::Placeholder, half, Gait::new(6.3, 1.0), false);
    assert_eq!(boxes.len(), 2);
    assert_eq!(boxes[0].size, [0.8, 0.9, 0.8]);
    assert!(boxes[1].position[2] > 0.4, "the nose is in front");
    let corpse = draw(UnitModel::Placeholder, half, Gait::IDLE, true);
    assert_eq!(corpse.len(), 1);
    assert!(corpse[0].size[1] < 0.2);
}

#[test]
fn walk_phase_is_continuous_across_speed_changes() {
    use std::f32::consts::{PI, TAU};
    let dt = 1.0 / 60.0;
    let wrapped = |a: f32, b: f32| ((a - b + PI).rem_euclid(TAU) - PI).abs();
    let mut phase = 1.0;
    for speed in [2.0_f32, 2.0, 7.0, 7.0, 2.0, 0.3, 7.0] {
        let next = Gait::advance(phase, speed, dt);
        // The step is proportional to the new speed, never a jump to a
        // different time-times-speed product.
        let step = wrapped(next, phase);
        assert!(step < 0.7, "phase jumped at speed {speed}: {step}");
        assert!((step - Gait::advance(0.0, speed, dt)).abs() < 1e-4);
        phase = next;
    }
    // Standing still holds the phase.
    assert_eq!(Gait::advance(phase, 0.0, dt), phase);
}

#[test]
fn swing_eases_at_a_bounded_rate_and_settles_to_rest() {
    use mmorpg_client::unit_models::SWING_RATE;
    let dt = 1.0 / 60.0;
    let mut gait = Gait {
        phase: 1.0,
        swing: 0.0,
    };
    let run = |gait: &mut Gait, speed: f32, frames: usize| {
        for _ in 0..frames {
            let next = gait.step(speed, dt);
            assert!((next.swing - gait.swing).abs() <= SWING_RATE * dt + 1e-6);
            assert!((0.0..=1.0).contains(&next.swing));
            *gait = next;
        }
    };
    run(&mut gait, 6.3, 5);
    assert!(gait.swing > 0.0 && gait.swing < 1.0);
    run(&mut gait, 6.3, 30);
    assert_eq!(gait.swing, 1.0);
    run(&mut gait, 0.0, 5);
    assert!(gait.swing > 0.0 && gait.swing < 1.0);
    let phase = gait.phase;
    run(&mut gait, 0.0, 1);
    assert_ne!(gait.phase, phase, "phase keeps advancing while settling");
    run(&mut gait, 0.0, 30);
    assert_eq!(gait.swing, 0.0);
    let held = gait.phase;
    run(&mut gait, 0.0, 10);
    assert_eq!(gait.phase, held);
}
