//! A mode schedules its hazard, while the shared zone owns health and death.
use mmorpg_core::{
    ClassChoice, EntityRef, GuardState, PlayerClass, Sex, StrikeHeight, ZoneCommand, ZoneEvent,
    ZoneId, ZoneSimulation, greyhaven_vale,
};

#[test]
fn host_damage_continues_exactly_and_uses_normal_death_cleanup_without_a_killer() {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(100), content.clone()).unwrap();
    zone.create_character(
        1,
        ClassChoice {
            class: PlayerClass::Warden,
            sex: Sex::Male,
        },
    )
    .unwrap();
    zone.apply_command(
        1,
        1,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 0,
        },
    )
    .unwrap();
    zone.apply_command(
        1,
        2,
        ZoneCommand::Guard {
            lane: Some(StrikeHeight::Middle),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let original = zone.snapshot().unwrap();
    let health = original.players[0].combat.health;
    assert_eq!(zone.apply_environmental_damage(1, 2).unwrap(), 2);
    let damaged = zone.snapshot().unwrap();
    assert_eq!(damaged.players[0].combat.health, health - 2);
    assert_eq!(
        damaged.players[0].combat.abilities.resource,
        original.players[0].combat.abilities.resource
    );
    assert_eq!(damaged.players[0].combat.calm_ticks, 0);
    assert!(damaged.players[0].combat.combat_timer > 0);
    let mut restored = ZoneSimulation::from_snapshot(damaged, content).unwrap();
    for _ in 0..10 {
        assert_eq!(
            zone.apply_environmental_damage(1, 1).unwrap(),
            restored.apply_environmental_damage(1, 1).unwrap()
        );
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    let remaining = zone.snapshot().unwrap().players[0].combat.health;
    assert_eq!(
        zone.apply_environmental_damage(1, u32::MAX).unwrap(),
        remaining
    );
    assert_eq!(
        restored.apply_environmental_damage(1, u32::MAX).unwrap(),
        remaining
    );
    let dead = zone.snapshot().unwrap();
    let player = &dead.players[0];
    assert_eq!(player.combat.health, 0);
    assert_eq!((player.forward, player.strafe), (0, 0));
    assert!(!player.jump_pending);
    assert!(!player.combat.swing_queued);
    assert!(player.combat.abilities.cast.is_none());
    assert!(player.combat.abilities.auras.is_empty());
    assert_eq!(player.combat.abilities.guard, GuardState::default());
    assert!(player.combat.events.contains(&ZoneEvent::Died {
        entity: EntityRef::Player(1),
        killer: None
    }));
    assert_eq!(dead, restored.snapshot().unwrap());
    ZoneSimulation::from_snapshot(dead.clone(), greyhaven_vale::content()).unwrap();
    assert_eq!(zone.apply_environmental_damage(1, 1).unwrap(), 0);
    assert_eq!(zone.snapshot().unwrap(), dead);
}

#[test]
fn unknown_and_zero_damage_leave_state_unchanged() {
    let mut zone =
        ZoneSimulation::with_content(ZoneId::new(100), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let before = zone.snapshot().unwrap();
    assert!(zone.apply_environmental_damage(2, 5).is_err());
    assert_eq!(zone.apply_environmental_damage(1, 0).unwrap(), 0);
    assert_eq!(zone.snapshot().unwrap(), before);
}
