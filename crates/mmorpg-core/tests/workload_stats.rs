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
    // Breakable props (#288) stand on fixed bodies and decide nothing; night raiders (#530) have
    // no body until an incident commits them.
    let props = content
        .creature_spawns()
        .iter()
        .filter(|spawn| {
            content
                .creature_template(spawn.template)
                .is_some_and(|template| template.prop().is_some())
        })
        .count();
    let raiders = content.raid().map_or(0, |site| site.raiders().count());
    // Far-away idle creatures sleep and near ones decide on their staggered tick (#814).
    let thinking = content.creature_spawns().len() - props - raiders;
    assert!(0 < work.creature_ai_evaluations && work.creature_ai_evaluations < thinking);
    let physics = work.physics.unwrap();
    assert_eq!(
        physics.bodies,
        content.definition().colliders().len()
            + 6
            + content.npcs().len()
            + content.creature_spawns().len()
            - raiders
            + content.doors().len()
            + content.platforms().len()
            + content.ramps().len()
            + content.pieces().len()
            + 1
    );
    assert!(physics.moved_bodies <= content.creature_spawns().len() + 1);

    let checkpoint = zone.snapshot().unwrap();
    let mut recovered = ZoneSimulation::from_snapshot(checkpoint.clone(), content).unwrap();
    assert_eq!(recovered.snapshot().unwrap(), checkpoint);
    assert!(recovered.tick_work().physics.is_none());
    let mut decisions = 0;
    for _ in 0..mmorpg_core::IDLE_DECISION_INTERVAL_TICKS {
        zone.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        // Counters describe one tick rather than accumulating or surviving a save, and the
        // recovered zone paces its creatures exactly as the original (#814).
        let work = zone.tick_work().creature_ai_evaluations;
        assert_eq!(work, recovered.tick_work().creature_ai_evaluations);
        decisions += work;
        assert_eq!(zone.snapshot().unwrap(), recovered.snapshot().unwrap());
    }
    // Swimmers still decide every tick; the others at most once an interval, or not at all
    // while asleep.
    let every_tick = thinking * usize::from(mmorpg_core::IDLE_DECISION_INTERVAL_TICKS);
    assert!(
        decisions * 2 < every_tick,
        "{decisions} decisions in one interval"
    );
}
