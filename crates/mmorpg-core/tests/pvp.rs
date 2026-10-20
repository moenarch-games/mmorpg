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
