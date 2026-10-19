use std::sync::Arc;

use mmorpg_core::{ZoneId, ZoneSimulation, greyhaven_vale};

#[test]
fn tick_work_counts_real_ai_and_engine_work_without_entering_recovery() {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    assert_eq!(zone.tick_work().creature_ai_evaluations, 0);
    assert!(zone.tick_work().physics.is_none());

    zone.advance_tick().unwrap();
    let work = zone.tick_work();
    // Breakable props (#288) stand on fixed bodies and decide nothing.
    let props = content
        .creature_spawns()
        .iter()
        .filter(|spawn| {
            content
                .creature_template(spawn.template)
                .is_some_and(|template| template.prop().is_some())
        })
        .count();
    assert_eq!(
        work.creature_ai_evaluations,
        content.creature_spawns().len() - props
    );
    let physics = work.physics.unwrap();
    assert_eq!(
        physics.bodies,
        content.definition().colliders().len()
            + 6
            + content.npcs().len()
            + content.creature_spawns().len()
            + content.doors().len()
            + content.platforms().len()
            + 1
    );
    assert!(physics.moved_bodies <= content.creature_spawns().len() + 1);

    let checkpoint = zone.snapshot().unwrap();
    let mut recovered = ZoneSimulation::from_snapshot(checkpoint.clone(), content).unwrap();
    assert_eq!(recovered.snapshot().unwrap(), checkpoint);
    assert!(recovered.tick_work().physics.is_none());
    for _ in 0..10 {
        zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        // Counters describe one tick rather than accumulating or surviving a save.
        assert_eq!(zone.tick_work().creature_ai_evaluations, 75);
        assert_eq!(zone.snapshot().unwrap(), recovered.snapshot().unwrap());
    }
}
