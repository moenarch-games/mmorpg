//! Explicit arena policy stays outside ordinary safe-world combat.
use mmorpg_core::{PlayerCombatPolicy, ZoneContent, ZoneId, ZoneSimulation, greyhaven_vale};
use std::sync::Arc;

#[test]
fn arena_policy_is_explicit_and_bound_to_recovery_content() {
    let safe = greyhaven_vale::content();
    assert_eq!(safe.player_combat_policy(), PlayerCombatPolicy::Safe);
    let arena = Arc::new(
        Arc::unwrap_or_clone(Arc::clone(&safe)).with_player_combat(PlayerCombatPolicy::FreeForAll),
    );
    assert_ne!(safe.fingerprint(), arena.fingerprint());
    let zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&arena)).unwrap();
    let snapshot = zone.snapshot().unwrap();
    assert!(ZoneSimulation::from_snapshot(snapshot.clone(), Arc::clone(&safe)).is_err());
    ZoneSimulation::from_snapshot(snapshot, arena).unwrap();
    let restored_safe: ZoneContent = Arc::unwrap_or_clone(safe);
    assert_eq!(
        restored_safe
            .clone()
            .with_player_combat(PlayerCombatPolicy::FreeForAll)
            .with_player_combat(PlayerCombatPolicy::Safe),
        restored_safe
    );
}

#[test]
fn an_explicit_arena_weapon_ability_uses_shared_player_damage_and_safe_world_refuses() {
    use mmorpg_core::{EntityRef, ZoneCommand};
    for policy in [PlayerCombatPolicy::Safe, PlayerCombatPolicy::FreeForAll] {
        let mut zone = two_players(policy);
        let before = zone.snapshot_for_player(2).unwrap().viewer.health;
        assert_eq!(
            zone.players_are_hostile(1, 2),
            policy == PlayerCombatPolicy::FreeForAll
        );
        assert!(!zone.players_are_hostile(1, 1));
        zone.apply_command(
            1,
            2,
            ZoneCommand::UseAbility {
                ability: 1,
                target: Some(EntityRef::Player(2)),
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
        let after = zone.snapshot_for_player(2).unwrap();
        if policy == PlayerCombatPolicy::FreeForAll {
            assert!(after.viewer.health < before);
            assert!(after.events.iter().any(|event| matches!(
                event,
                mmorpg_core::ZoneEvent::DamageTaken {
                    source: EntityRef::Player(1),
                    ..
                }
            )));
        } else {
            assert_eq!(after.viewer.health, before);
            assert_eq!(
                zone.snapshot_for_player(1)
                    .unwrap()
                    .viewer
                    .resource
                    .unwrap()
                    .value,
                40
            );
        }
    }
}

fn two_players(policy: PlayerCombatPolicy) -> ZoneSimulation {
    two_classes(policy, [0, 0])
}

fn two_classes(policy: PlayerCombatPolicy, classes: [u8; 2]) -> ZoneSimulation {
    use mmorpg_core::ZoneCommand;
    let content =
        Arc::new(Arc::unwrap_or_clone(greyhaven_vale::content()).with_player_combat(policy));
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    for player in [1, 2] {
        zone.add_player(player).unwrap();
        zone.apply_command(
            player,
            1,
            ZoneCommand::ChooseClass {
                class: classes[(player - 1) as usize],
                sex: 0,
            },
        )
        .unwrap();
    }
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    for (index, player) in state.players.iter_mut().enumerate() {
        player.position = mmorpg_core::physics::from_units([index as i32 * 100, 90, -2000]);
        player.combat.abilities.resource = 40;
    }
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

#[test]
fn an_arena_weapon_hit_respects_the_existing_raised_shield() {
    use mmorpg_core::{EntityRef, StrikeHeight, ZoneCommand};
    let mut zone = two_players(PlayerCombatPolicy::FreeForAll);
    zone.apply_command(
        2,
        2,
        ZoneCommand::Move {
            forward: 0,
            strafe: 0,
            facing: 49152,
        },
    )
    .unwrap();
    zone.apply_command(
        2,
        3,
        ZoneCommand::Guard {
            lane: Some(StrikeHeight::High),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let before = zone.snapshot_for_player(2).unwrap().viewer.health;
    zone.apply_command(
        1,
        2,
        ZoneCommand::UseAbility {
            ability: 1,
            target: Some(EntityRef::Player(2)),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let after = zone.snapshot_for_player(2).unwrap();
    assert_eq!(after.viewer.health, before);
    assert!(after.events.iter().any(|event| matches!(
        event,
        mmorpg_core::ZoneEvent::Blocked {
            source: EntityRef::Player(1),
            target: EntityRef::Player(2),
            ..
        }
    )));
}

#[test]
fn arena_auto_attacks_use_the_existing_swing_clock() {
    use mmorpg_core::{EntityRef, ZoneCommand};
    for policy in [PlayerCombatPolicy::Safe, PlayerCombatPolicy::FreeForAll] {
        let mut zone = two_players(policy);
        let before = zone.snapshot_for_player(2).unwrap().viewer.health;
        zone.apply_command(1, 2, ZoneCommand::SelectTarget(Some(EntityRef::Player(2))))
            .unwrap();
        zone.apply_command(1, 3, ZoneCommand::StartAttack).unwrap();
        for _ in 0..150 {
            zone.advance_tick().unwrap();
        }
        let after = zone.snapshot_for_player(2).unwrap();
        assert_eq!(
            after.viewer.health < before,
            policy == PlayerCombatPolicy::FreeForAll
        );
    }
}

#[test]
fn an_arena_firebolt_completes_its_cast_and_hits_through_a_real_projectile() {
    use mmorpg_core::{EntityRef, StrikeHeight, ZoneCommand};
    for guarding in [false, true] {
        let mut zone = two_classes(PlayerCombatPolicy::FreeForAll, [2, 0]);
        if guarding {
            zone.apply_command(
                2,
                2,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: 0,
                    facing: 49152,
                },
            )
            .unwrap();
            zone.apply_command(
                2,
                3,
                ZoneCommand::Guard {
                    lane: Some(StrikeHeight::High),
                },
            )
            .unwrap();
            zone.advance_tick().unwrap();
        }
        let before = zone.snapshot_for_player(2).unwrap().viewer.health;
        zone.apply_command(
            1,
            2,
            ZoneCommand::UseAbility {
                ability: 9,
                target: Some(EntityRef::Player(2)),
            },
        )
        .unwrap();
        let mut shot = false;
        let mut damaged = false;
        let mut blocked = false;
        for _ in 0..120 {
            zone.advance_tick().unwrap();
            shot |= !zone.snapshot_for_player(1).unwrap().projectiles.is_empty();
            let victim = zone.snapshot_for_player(2).unwrap();
            damaged |= victim.viewer.health < before;
            blocked |= victim.events.iter().any(|event| {
                matches!(
                    event,
                    mmorpg_core::ZoneEvent::Blocked {
                        source: EntityRef::Player(1),
                        ..
                    }
                )
            });
        }
        assert!(shot, "the projectile, not the cast, carries the hit");
        assert_eq!(damaged, !guarding);
        assert_eq!(blocked, guarding);
    }
}

#[test]
fn an_arena_cast_restores_and_continues_identically() {
    use mmorpg_core::{EntityRef, ZoneCommand};
    let mut zone = two_classes(PlayerCombatPolicy::FreeForAll, [2, 0]);
    zone.apply_command(
        1,
        2,
        ZoneCommand::UseAbility {
            ability: 9,
            target: Some(EntityRef::Player(2)),
        },
    )
    .unwrap();
    for _ in 0..15 {
        zone.advance_tick().unwrap();
    }
    assert!(zone.snapshot_for_player(1).unwrap().viewer.cast.is_some());
    let mut restored =
        ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content()))
            .unwrap();
    for _ in 0..100 {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
}

#[test]
fn an_arena_damage_over_time_aura_restores_and_pulses_identically() {
    use mmorpg_core::{EntityRef, ZoneCommand};
    let zone = two_classes(PlayerCombatPolicy::FreeForAll, [1, 0]);
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.level = 2;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.apply_command(
        1,
        2,
        ZoneCommand::UseAbility {
            ability: 6,
            target: Some(EntityRef::Player(2)),
        },
    )
    .unwrap();
    for _ in 0..30 {
        zone.advance_tick().unwrap();
    }
    let state = zone.snapshot().unwrap();
    assert!(
        state.players[1]
            .combat
            .abilities
            .auras
            .iter()
            .any(|aura| aura.ability.get() == 6)
    );
    let mut restored = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let before = zone.snapshot_for_player(2).unwrap().viewer.health;
    for _ in 0..100 {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert!(zone.snapshot_for_player(2).unwrap().viewer.health < before);
}
