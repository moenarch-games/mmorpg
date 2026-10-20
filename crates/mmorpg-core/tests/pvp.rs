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
        let content =
            Arc::new(Arc::unwrap_or_clone(greyhaven_vale::content()).with_player_combat(policy));
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
        for player in [1, 2] {
            zone.add_player(player).unwrap();
            zone.apply_command(player, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
                .unwrap();
        }
        zone.advance_tick().unwrap();
        let mut state = zone.snapshot().unwrap();
        for (index, player) in state.players.iter_mut().enumerate() {
            player.position = mmorpg_core::physics::from_units([index as i32 * 100, 90, -2000]);
            player.combat.abilities.resource = 40;
        }
        let mut zone = ZoneSimulation::from_snapshot(state, content).unwrap();
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
