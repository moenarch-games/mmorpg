//! Independent pre-presentation gameplay oracle: observing attacks must not draw
//! extra RNG or alter health, timers, threat or physics. This fixture was captured
//! against 6548f85 before the #27 implementation, with no new cue API involved.
mod support;
use mmorpg_core::{CreatureId, EntityRef, ZoneCommand, ZoneId, ZoneSimulation};
#[test]
fn presentation_preserves_the_prechange_combat_trace() {
    let content = support::arena::arena(
        vec![support::arena::wolf(1000, [1, 2])],
        vec![support::arena::spawn(1, support::arena::WOLF, [-180, 0])],
        vec![],
    );
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), content).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(
        1,
        1,
        ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(1)))),
    )
    .unwrap();
    zone.apply_command(1, 2, ZoneCommand::StartAttack).unwrap();
    let mut trace = String::new();
    for tick in 1..=240 {
        zone.advance_tick().unwrap();
        if [1, 60, 61, 120, 121, 180, 240].contains(&tick) {
            let state = zone.snapshot().unwrap();
            let player = &state.players[0];
            let creature = &state.creatures[0];
            trace.push_str(&format!(
                "{} {} {} {} {} {} {:?} {:?} {:?}\n",
                tick,
                state.rng_state,
                state.loot_rng_state,
                player.combat.health,
                creature.health,
                player.combat.swing_timer,
                player.position,
                creature.position,
                creature.threat
            ));
        }
    }
    assert_eq!(trace, include_str!("fixtures/presentation-gameplay.txt"));
}
