//! Production backend routing, with Rapier still selectable for unsupported geometry.

use std::sync::Arc;

use mmorpg_core::physics::{Backend, Vec3i, World};
use mmorpg_core::{ZoneContent, ZoneDefinition, ZoneId, ZoneSimulation, greyhaven_vale};

#[test]
fn engine_is_default_and_rapier_remains_selectable() {
    assert_eq!(World::new(Vec3i::ZERO).backend(), Backend::Engine);
    assert_eq!(
        World::with_backend(Vec3i::ZERO, Backend::Rapier).backend(),
        Backend::Rapier
    );
}

#[test]
fn supported_zone_and_recovery_use_engine() {
    let content = Arc::new(ZoneContent::from_definition(ZoneDefinition::default()));
    let zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
    assert_eq!(zone.physics_backend(), Backend::Engine);
    let restored = ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), content).unwrap();
    assert_eq!(restored.physics_backend(), Backend::Engine);
}

#[test]
fn authored_rock_hulls_and_cave_meshes_select_engine_across_recovery() {
    let content = greyhaven_vale::content();
    assert!(
        !content.rocks().is_empty(),
        "Greyhaven authors convex rock proxies"
    );
    assert!(
        !content.definition().static_meshes().is_empty(),
        "Redbrand Cave authors static triangle meshes"
    );
    let zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
    assert_eq!(zone.physics_backend(), Backend::Engine);
    let restored = ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), content).unwrap();
    assert_eq!(restored.physics_backend(), Backend::Engine);
}
