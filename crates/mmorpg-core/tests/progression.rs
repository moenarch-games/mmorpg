mod support;
use mmorpg_core::{CreatureId, CreatureLife, EntityRef, ZoneCommand, ZoneId, ZoneSimulation};
use std::sync::Arc;
use support::arena::{self, WOLF};

fn fixture() -> ZoneSimulation {
    let content = arena::arena(
        vec![arena::wolf(1, [1, 1])],
        vec![
            arena::spawn(1, WOLF, [-180, 0]),
            arena::spawn(2, WOLF, [-180, -160]),
        ],
        vec![],
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    for creature in &mut state.creatures {
        creature.health = 1;
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn kill(zone: &mut ZoneSimulation, id: u32, sequence: u32) {
    zone.apply_command(
        1,
        sequence,
        ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(id)))),
    )
    .unwrap();
    zone.apply_command(1, sequence + 1, ZoneCommand::StartAttack)
        .unwrap();
    for _ in 0..180 {
        zone.advance_tick().unwrap();
        if matches!(
            zone.snapshot().unwrap().creatures[(id - 1) as usize].life,
            CreatureLife::Corpse { .. }
        ) {
            return;
        }
    }
    panic!("kill did not finish");
}

#[test]
fn kills_level_a_player_once_and_recovery_continues_mid_progression() {
    let mut zone = fixture();
    kill(&mut zone, 1, 1);
    assert_eq!(zone.snapshot_for_player(1).unwrap().viewer.experience, 50);
    assert!(zone.apply_command(1, 2, ZoneCommand::StartAttack).is_err());
    let checkpoint = zone.snapshot().unwrap();
    let mut recovered =
        ZoneSimulation::from_snapshot(checkpoint, Arc::clone(zone.content())).unwrap();
    for simulation in [&mut zone, &mut recovered] {
        kill(simulation, 2, 3);
    }
    assert_eq!(zone.snapshot().unwrap(), recovered.snapshot().unwrap());
    let viewer = zone.snapshot_for_player(1).unwrap().viewer;
    assert_eq!(
        (
            viewer.level,
            viewer.experience,
            viewer.experience_to_next_level,
            viewer.max_health
        ),
        (2, 0, 200, 65)
    );
    for _ in 0..100 {
        zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), recovered.snapshot().unwrap());
    }
    assert_eq!(zone.snapshot_for_player(1).unwrap().viewer.experience, 0);
}

#[test]
fn only_the_living_nearby_tapper_receives_experience() {
    for (health, position, expected) in [
        (50, [500, 90, 0], 50),
        (0, [500, 90, 0], 0),
        (50, [5000, 90, 0], 0),
    ] {
        let mut zone = fixture();
        zone.add_player(2).unwrap();
        let mut state = zone.snapshot().unwrap();
        state.players[1].combat.health = health;
        state.players[1].position = position;
        state.creatures[0].tapped_by = Some(2);
        let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        kill(&mut zone, 1, 1);
        assert_eq!(zone.snapshot_for_player(1).unwrap().viewer.experience, 0);
        assert_eq!(
            zone.snapshot_for_player(2).unwrap().viewer.experience,
            expected
        );
    }
}

#[test]
fn a_reused_player_id_cannot_inherit_an_old_tap_reward() {
    let mut zone = fixture();
    zone.add_player(2).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.creatures[0].tapped_by = Some(2);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    assert!(zone.remove_player(2));
    zone.add_player(2).unwrap();
    kill(&mut zone, 1, 1);
    assert_eq!(zone.snapshot_for_player(2).unwrap().viewer.experience, 0);
    assert_eq!(zone.snapshot_for_player(1).unwrap().viewer.experience, 50);
}

#[test]
fn invalid_current_level_xp_and_capped_xp_fail_restore() {
    let zone = fixture();
    for (level, experience) in [(1, 100), (2, 200), (10, 1), (11, 0)] {
        let mut state = zone.snapshot().unwrap();
        state.players[0].combat.level = level;
        state.players[0].combat.experience = experience;
        assert!(ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).is_err());
    }
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.level = 10;
    state.players[0].combat.experience = 0;
    let mut capped = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    kill(&mut capped, 1, 1);
    assert_eq!(capped.snapshot_for_player(1).unwrap().viewer.experience, 0);
}

#[test]
fn final_level_discards_overflow_and_gray_kills_grant_nothing() {
    let mut template = arena::wolf(1, [1, 1]);
    template.min_level = 9;
    template.max_level = 9;
    let content = arena::arena(
        vec![template],
        vec![arena::spawn(1, WOLF, [-180, 0])],
        vec![],
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.level = 9;
    state.players[0].combat.experience = 899;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    kill(&mut zone, 1, 1);
    let viewer = zone.snapshot_for_player(1).unwrap().viewer;
    assert_eq!(
        (
            viewer.level,
            viewer.experience,
            viewer.experience_to_next_level,
            viewer.max_health
        ),
        (10, 0, 0, 185)
    );
    let zone = fixture();
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.level = 6;
    let mut gray = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    kill(&mut gray, 1, 1);
    assert_eq!(gray.snapshot_for_player(1).unwrap().viewer.experience, 0);
}

#[test]
fn level_up_preserves_missing_health_instead_of_fully_healing() {
    let zone = fixture();
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.health = 20;
    let mut baseline =
        ZoneSimulation::from_snapshot(state.clone(), Arc::clone(zone.content())).unwrap();
    state.players[0].combat.experience = 50;
    let mut leveling = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    kill(&mut baseline, 1, 1);
    kill(&mut leveling, 1, 1);
    let baseline = baseline.snapshot_for_player(1).unwrap().viewer;
    let leveling = leveling.snapshot_for_player(1).unwrap().viewer;
    assert_eq!(leveling.level, 2);
    assert_eq!(
        leveling.max_health - leveling.health,
        baseline.max_health - baseline.health
    );
    assert_eq!(leveling.health - baseline.health, 15);
}
